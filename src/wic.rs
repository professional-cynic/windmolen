//! Image decoding and encoding through the Windows Imaging Component.
//!
//! WIC ships with Windows and is serviced by Windows Update; codecs from
//! the Microsoft Store (HEIF, WebP, AVIF, RAW, JPEG XL) plug in automatically.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;

use windows::Win32::Foundation::{
    GENERIC_READ, GENERIC_WRITE, WINCODEC_ERR_BADHEADER, WINCODEC_ERR_BADIMAGE, WINCODEC_ERR_BADSTREAMDATA, WINCODEC_ERR_COMPONENTNOTFOUND,
    WINCODEC_ERR_STREAMREAD, WINCODEC_ERR_UNKNOWNIMAGEFORMAT,
};
use windows::Win32::Graphics::Imaging::*;
use windows::Win32::System::Com::StructuredStorage::{PROPBAG2, PROPVARIANT, PropVariantClear};
use windows::Win32::System::Com::*;
use windows::Win32::System::Variant::*;
use windows::core::{GUID, HSTRING, IUnknown, Interface, PCWSTR, Result, w};

use crate::edit::Orient;
use crate::meta::{self, Value};
use crate::sys::Poster;

pub const WM_DECODED: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 1;

/// Pixel-count ceiling (512 MP); larger images are refused, not decoded.
const MAX_PIXELS: u64 = 512 * 1024 * 1024;

pub struct Wic(pub IWICImagingFactory);

impl Wic {
    pub fn new() -> Result<Self> {
        // SAFETY: standard in-process COM activation.
        unsafe { CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).map(Self) }
    }

    /// Lower-case extensions (without dot) of every installed decoder.
    pub fn extensions(&self) -> HashSet<String> {
        let mut set = HashSet::new();
        // SAFETY: enumeration of registered components; buffers are locals.
        unsafe {
            let Ok(e) = self.0.CreateComponentEnumerator(WICDecoder.0 as u32, WICComponentEnumerateDefault.0 as u32) else {
                return set;
            };
            loop {
                let mut item = [None::<IUnknown>];
                let mut got = 0;
                if e.Next(&mut item, Some(&mut got)).is_err() || got == 0 {
                    break;
                }
                let Some(Ok(info)) = item[0].take().map(|u| u.cast::<IWICBitmapCodecInfo>()) else { continue };
                let mut len = 0;
                let _ = info.GetFileExtensions(&mut [], &mut len);
                let mut buf = vec![0u16; len as usize];
                if len == 0 || info.GetFileExtensions(&mut buf, &mut len).is_err() {
                    continue;
                }
                let s = String::from_utf16_lossy(&buf[..buf.iter().position(|&c| c == 0).unwrap_or(buf.len())]);
                set.extend(parse_extensions(&s));
            }
        }
        set
    }
}

/// Splits a codec's extension list (".jpg,.jpeg"; some codecs use ';').
fn parse_extensions(s: &str) -> impl Iterator<Item = String> + '_ {
    s.split([',', ';']).map(|x| x.trim().trim_start_matches('.').to_lowercase()).filter(|x| !x.is_empty())
}

/// Owned `PROPVARIANT`, cleared on drop.
struct PropVar(PROPVARIANT);

impl Drop for PropVar {
    fn drop(&mut self) {
        // SAFETY: the variant was initialised by WIC or zeroed.
        unsafe {
            let _ = PropVariantClear(&mut self.0);
        }
    }
}

impl PropVar {
    fn value(&self) -> Option<Value> {
        // SAFETY: each union field is read only for its matching type tag.
        unsafe {
            let v = &self.0.Anonymous.Anonymous;
            let d = &v.Anonymous;
            Some(match v.vt {
                VT_UI1 => Value::Int(i64::from(d.bVal)),
                VT_UI2 => Value::Int(i64::from(d.uiVal)),
                VT_UI4 => Value::Int(i64::from(d.ulVal)),
                VT_I2 => Value::Int(i64::from(d.iVal)),
                VT_I4 => Value::Int(i64::from(d.lVal)),
                VT_R4 => Value::Float(f64::from(d.fltVal)),
                VT_R8 => Value::Float(d.dblVal),
                // WIC stores EXIF rationals as numerator (low) / denominator (high).
                VT_UI8 => Value::Rational(i64::from(d.uhVal as u32), (d.uhVal >> 32) as i64),
                VT_I8 => Value::Rational(i64::from(d.hVal as i32), i64::from((d.hVal >> 32) as i32)),
                VT_LPSTR if !d.pszVal.is_null() => Value::Text(String::from_utf8_lossy(d.pszVal.as_bytes()).into_owned()),
                VT_LPWSTR if !d.pwszVal.is_null() => Value::Text(d.pwszVal.to_string().ok()?),
                _ => return None,
            })
        }
    }
}

fn query(r: &IWICMetadataQueryReader, name: PCWSTR) -> Option<Value> {
    let mut v = PropVar(PROPVARIANT::default());
    // SAFETY: out-pointer to an owned, zeroed variant.
    unsafe { r.GetMetadataByName(name, &mut v.0) }.ok()?;
    v.value()
}

/// Reads an EXIF tag through the container's metadata path, falling back
/// to the container-independent photo policy name.
fn exif_tag(r: &IWICMetadataQueryReader, container: &GUID, tag: u16, sub_ifd: bool, policy: PCWSTR) -> Option<Value> {
    let root = match *container {
        c if c == GUID_ContainerFormatJpeg => Some("/app1/ifd"),
        c if c == GUID_ContainerFormatTiff || c == GUID_ContainerFormatAdng || c == GUID_ContainerFormatHeif => Some("/ifd"),
        _ => None,
    };
    if let Some(root) = root {
        let path = HSTRING::from(format!("{root}{}/{{ushort={tag}}}", if sub_ifd { "/exif" } else { "" }));
        if let Some(v) = query(r, PCWSTR(path.as_ptr())) {
            return Some(v);
        }
    }
    query(r, policy)
}

/// Facts about an image shown in the properties panel and used for saving.
#[derive(Clone, Debug)]
pub struct Meta {
    pub format: String,
    pub container: GUID,
    /// Stored pixel size, before orientation.
    pub width: u32,
    pub height: u32,
    pub orient: Orient,
    pub frames: u32,
    pub profile: Option<String>,
    pub exif: Vec<(&'static str, String)>,
    pub can_encode: bool,
    pub file_size: u64,
    /// Last modification, formatted for the user's locale.
    pub modified: Option<String>,
}

pub struct Decoded {
    /// Pixel size of `pixels` (may be smaller than the source).
    pub w: u32,
    pub h: u32,
    /// Premultiplied BGRA, sRGB.
    pub pixels: Vec<u8>,
    pub meta: Meta,
}

fn container_name(g: &GUID) -> Option<&'static str> {
    const NAMES: [(GUID, &str); 11] = [
        (GUID_ContainerFormatJpeg, "JPEG"),
        (GUID_ContainerFormatPng, "PNG"),
        (GUID_ContainerFormatGif, "GIF"),
        (GUID_ContainerFormatBmp, "BMP"),
        (GUID_ContainerFormatTiff, "TIFF"),
        (GUID_ContainerFormatIco, "ICO"),
        (GUID_ContainerFormatHeif, "HEIF"),
        (GUID_ContainerFormatWebp, "WebP"),
        (GUID_ContainerFormatWmp, "JPEG XR"),
        (GUID_ContainerFormatDds, "DDS"),
        (GUID_ContainerFormatAdng, "DNG"),
    ];
    NAMES.iter().find(|(id, _)| id == g).map(|&(_, n)| n)
}

fn friendly_name(d: &IWICBitmapDecoder) -> Option<String> {
    // SAFETY: two-call buffer pattern with local buffers.
    unsafe {
        let info = d.GetDecoderInfo().ok()?;
        let mut len = 0;
        let _ = info.GetFriendlyName(&mut [], &mut len);
        let mut buf = vec![0u16; len as usize];
        info.GetFriendlyName(&mut buf, &mut len).ok()?;
        let s = String::from_utf16_lossy(&buf[..buf.iter().position(|&c| c == 0).unwrap_or(buf.len())]);
        Some(s.trim_end_matches(" Decoder").trim_end_matches(" Codec").to_owned())
    }
}

fn color_contexts(f: &IWICImagingFactory, frame: &IWICBitmapFrameDecode) -> Vec<IWICColorContext> {
    // SAFETY: WIC fills caller-created contexts; counts are checked.
    unsafe {
        let mut n = 0;
        if frame.GetColorContexts(&mut [], &mut n).is_err() || n == 0 || n > 16 {
            return Vec::new();
        }
        let mut ctx: Vec<Option<IWICColorContext>> = (0..n).map(|_| f.CreateColorContext().ok()).collect();
        if ctx.iter().any(Option::is_none) || frame.GetColorContexts(&mut ctx, &mut n).is_err() {
            return Vec::new();
        }
        ctx.into_iter().take(n as usize).flatten().collect()
    }
}

fn profile_bytes(c: &IWICColorContext) -> Option<Vec<u8>> {
    // SAFETY: two-call buffer pattern; size capped at 16 MB.
    unsafe {
        let mut n = 0;
        c.GetProfileBytes(&mut [], &mut n).ok()?;
        if n == 0 || n > 16 << 20 {
            return None;
        }
        let mut b = vec![0; n as usize];
        c.GetProfileBytes(&mut b, &mut n).ok()?;
        Some(b)
    }
}

/// Converts `src` to sRGB when the frame carries a non-sRGB colour profile.
/// Returns the (possibly converted) source and the profile's name.
fn to_srgb(f: &IWICImagingFactory, frame: &IWICBitmapFrameDecode, src: IWICBitmapSource) -> (IWICBitmapSource, Option<String>) {
    let contexts = color_contexts(f, frame);
    let mut name = None;
    let mut chosen = None;
    for c in &contexts {
        // SAFETY: valid context objects.
        match unsafe { c.GetType() } {
            Ok(t) if t == WICColorContextProfile => {
                name = profile_bytes(c).and_then(|b| meta::icc_description(&b));
                chosen = Some(c.clone());
                break;
            }
            // SAFETY: valid context object.
            Ok(t) if t == WICColorContextExifColorSpace && unsafe { c.GetExifColorSpace() } == Ok(2) => {
                name = Some("Adobe RGB".into());
                chosen = Some(c.clone());
            }
            _ => {}
        }
    }
    let Some(ctx) = chosen else { return (src, name) };
    if name.as_deref().is_some_and(|n| n.starts_with("sRGB")) {
        return (src, name);
    }
    // SAFETY: COM calls on valid objects; GUID pointers are to statics.
    let converted = unsafe {
        (|| -> Result<IWICBitmapSource> {
            let dst = f.CreateColorContext()?;
            dst.InitializeFromExifColorSpace(1)?;
            let t = f.CreateColorTransformer()?;
            if t.Initialize(&src, &ctx, &dst, &GUID_WICPixelFormat32bppBGRA).is_err() {
                // Some transforms only accept 8-bit input.
                let conv = f.CreateFormatConverter()?;
                conv.Initialize(&src, &GUID_WICPixelFormat32bppBGRA, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeCustom)?;
                t.Initialize(&conv, &ctx, &dst, &GUID_WICPixelFormat32bppBGRA)?;
            }
            t.cast()
        })()
    };
    (converted.unwrap_or(src), name)
}

fn read_meta(f: &IWICImagingFactory, dec: &IWICBitmapDecoder, frame: &IWICBitmapFrameDecode, path: &Path) -> Result<Meta> {
    // SAFETY: COM calls on valid objects with local out-pointers.
    unsafe {
        let (mut w, mut h) = (0, 0);
        frame.GetSize(&mut w, &mut h)?;
        let container = dec.GetContainerFormat()?;
        let reader = frame.GetMetadataQueryReader().ok();
        let q = |tag: u16, sub: bool, policy: PCWSTR| reader.as_ref().and_then(|r| exif_tag(r, &container, tag, sub, policy));
        let orient = match q(274, false, w!("System.Photo.Orientation")) {
            Some(Value::Int(v)) => Orient::from_exif(v as u16),
            _ => Orient::IDENTITY,
        };

        let mut exif = Vec::new();
        let make = q(271, false, w!("System.Photo.CameraManufacturer"));
        let model = q(272, false, w!("System.Photo.CameraModel"));
        if let Some(c) = meta::camera(make.as_ref().and_then(Value::as_text), model.as_ref().and_then(Value::as_text)) {
            exif.push(("Camera", c));
        }
        if let Some(l) = q(42036, true, w!("System.Photo.LensModel")).as_ref().and_then(Value::as_text) {
            exif.push(("Lens", l.to_owned()));
        }
        type Field = (&'static str, u16, PCWSTR, fn(&Value) -> Option<String>);
        let fields: [Field; 4] = [
            ("Exposure", 33434, w!("System.Photo.ExposureTime"), meta::exposure),
            ("Aperture", 33437, w!("System.Photo.FNumber"), meta::aperture),
            ("ISO", 34855, w!("System.Photo.ISOSpeed"), meta::iso),
            ("Focal Length", 37386, w!("System.Photo.FocalLength"), meta::focal_length),
        ];
        for (label, tag, policy, fmt) in fields {
            if let Some(s) = q(tag, true, policy).as_ref().and_then(fmt) {
                exif.push((label, s));
            }
        }
        if let Some(d) = q(36867, true, w!("System.Photo.DateTaken")).as_ref().and_then(Value::as_text).and_then(meta::exif_date) {
            exif.push(("Taken", crate::sys::format_date(d)));
        }

        let fs = std::fs::metadata(path).ok();
        let can_encode = f.CreateEncoder(&container, std::ptr::null()).is_ok();
        Ok(Meta {
            format: container_name(&container).map(Into::into).or_else(|| friendly_name(dec)).unwrap_or_else(|| "Image".into()),
            container,
            width: w,
            height: h,
            orient,
            frames: dec.GetFrameCount().unwrap_or(1),
            profile: None,
            exif,
            can_encode,
            file_size: fs.as_ref().map_or(0, |m| m.len()),
            modified: fs.and_then(|m| m.modified().ok()).map(crate::sys::format_time).filter(|s| !s.is_empty()),
        })
    }
}

/// Decodes the first frame of `path` to sRGB premultiplied BGRA, scaling
/// down so that neither edge exceeds `max_edge`.
pub fn decode(f: &IWICImagingFactory, path: &Path, max_edge: u32) -> std::result::Result<Decoded, String> {
    let wide = HSTRING::from(path.as_os_str());
    // SAFETY: COM calls on valid objects; buffers sized from checked maths.
    unsafe {
        let dec = f
            .CreateDecoderFromFilename(&wide, None, GENERIC_READ, WICDecodeMetadataCacheOnDemand)
            .map_err(|e| describe(&e))?;
        let frame = dec.GetFrame(0).map_err(|e| describe(&e))?;
        let mut meta = read_meta(f, &dec, &frame, path).map_err(|e| describe(&e))?;
        let (sw, sh) = (meta.width, meta.height);
        if sw == 0 || sh == 0 || u64::from(sw) * u64::from(sh) > MAX_PIXELS {
            return Err(format!("Unsupported image size ({sw} × {sh})"));
        }

        let mut src: IWICBitmapSource = frame.cast().map_err(|e| describe(&e))?;
        let (mut w, mut h) = (sw, sh);
        let max_edge = max_edge.max(256);
        if w > max_edge || h > max_edge {
            let s = f64::from(max_edge) / f64::from(w.max(h));
            w = ((f64::from(w) * s).round() as u32).clamp(1, max_edge);
            h = ((f64::from(h) * s).round() as u32).clamp(1, max_edge);
            let scaler = f.CreateBitmapScaler().map_err(|e| describe(&e))?;
            scaler.Initialize(&src, w, h, WICBitmapInterpolationModeHighQualityCubic).map_err(|e| describe(&e))?;
            src = scaler.cast().map_err(|e| describe(&e))?;
        }
        let (src, profile) = to_srgb(f, &frame, src);
        meta.profile = profile;

        let conv = f.CreateFormatConverter().map_err(|e| describe(&e))?;
        conv.Initialize(&src, &GUID_WICPixelFormat32bppPBGRA, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeCustom)
            .map_err(|e| describe(&e))?;

        let stride = w.checked_mul(4).ok_or("Image too large")?;
        let len = (stride as usize).checked_mul(h as usize).ok_or("Image too large")?;
        let mut pixels = Vec::new();
        pixels.try_reserve_exact(len).map_err(|_| "Not enough memory to open this image".to_owned())?;
        pixels.resize(len, 0);
        conv.CopyPixels(std::ptr::null(), stride, &mut pixels).map_err(|e| describe(&e))?;
        Ok(Decoded { w, h, pixels, meta })
    }
}

/// A short, human explanation of common WIC failures.
pub fn describe(e: &windows::core::Error) -> String {
    match e.code() {
        WINCODEC_ERR_COMPONENTNOTFOUND | WINCODEC_ERR_UNKNOWNIMAGEFORMAT => {
            "The file is damaged, or its format needs a codec that isn't installed".into()
        }
        WINCODEC_ERR_BADHEADER | WINCODEC_ERR_BADIMAGE | WINCODEC_ERR_BADSTREAMDATA | WINCODEC_ERR_STREAMREAD => {
            "The image is damaged or incomplete".into()
        }
        _ => {
            let m = e.message();
            if m.is_empty() { format!("Error {:#010x}", e.code().0) } else { m.trim().to_owned() }
        }
    }
}

// ------------------------------------------------------------------------
// Background decoding
// ------------------------------------------------------------------------

pub struct Job {
    pub path: PathBuf,
    pub max_edge: u32,
}

pub type Outcome = (PathBuf, std::result::Result<Decoded, String>);

#[derive(Default)]
struct Queue {
    jobs: Vec<Job>,
    /// The image being decoded right now.
    busy: Option<PathBuf>,
    quit: bool,
}

/// One decoding thread fed by a replaceable queue: navigation replaces
/// pending work, so rapid browsing never builds a backlog.
pub struct Decoder {
    shared: Arc<(Mutex<Queue>, Condvar)>,
    thread: Option<JoinHandle<()>>,
}

impl Decoder {
    pub fn new(out: Poster<Outcome>) -> Self {
        let shared = Arc::new((Mutex::new(Queue::default()), Condvar::new()));
        let s = Arc::clone(&shared);
        let thread = std::thread::spawn(move || {
            let _com = crate::sys::ComGuard::mta();
            let Ok(wic) = Wic::new() else { return };
            let (lock, cv) = &*s;
            loop {
                let job = {
                    let mut q = lock.lock().unwrap_or_else(|p| p.into_inner());
                    q.busy = None;
                    while q.jobs.is_empty() && !q.quit {
                        q = cv.wait(q).unwrap_or_else(|p| p.into_inner());
                    }
                    if q.quit {
                        return;
                    }
                    let job = q.jobs.remove(0);
                    q.busy = Some(job.path.clone());
                    job
                };
                let result = decode(&wic.0, &job.path, job.max_edge);
                if !out.send((job.path, result)) {
                    return;
                }
            }
        });
        Self { shared, thread: Some(thread) }
    }

    /// Replaces all pending jobs, first one first. The image being decoded
    /// right now is not queued again: its result is on the way.
    pub fn request(&self, mut jobs: Vec<Job>) {
        let (lock, cv) = &*self.shared;
        let mut q = lock.lock().unwrap_or_else(|p| p.into_inner());
        jobs.retain(|j| q.busy.as_ref() != Some(&j.path));
        q.jobs = jobs;
        cv.notify_one();
    }
}

impl Drop for Decoder {
    fn drop(&mut self) {
        let (lock, cv) = &*self.shared;
        lock.lock().unwrap_or_else(|p| p.into_inner()).quit = true;
        cv.notify_one();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

// ------------------------------------------------------------------------
// Saving
// ------------------------------------------------------------------------

pub struct SaveJob {
    pub src: PathBuf,
    pub dst: PathBuf,
    pub container: GUID,
    /// Orientation of the result relative to the stored pixels.
    pub orient: Orient,
    /// Crop in oriented pixels: (x, y, w, h).
    pub crop: Option<(u32, u32, u32, u32)>,
}

/// Writes the edited image to `job.dst`, a temporary file owned by the
/// caller (who also removes it on failure).
///
/// A pure rotation of a JPEG or TIFF only rewrites the orientation tag
/// (lossless); anything else is re-encoded from the original pixels.
pub fn save(f: &IWICImagingFactory, job: &SaveJob) -> std::result::Result<(), String> {
    let lossless_container = job.container == GUID_ContainerFormatJpeg || job.container == GUID_ContainerFormatTiff;
    if job.crop.is_none() && lossless_container && retag(f, job).is_ok() {
        return Ok(());
    }
    encode(f, job)
}

/// Adds the failing step to an error message.
trait Ctx<T> {
    fn ctx(self, step: &str) -> std::result::Result<T, String>;
}

impl<T> Ctx<T> for Result<T> {
    fn ctx(self, step: &str) -> std::result::Result<T, String> {
        self.map_err(|e| format!("{} ({step})", describe(&e)))
    }
}

fn orientation_variant(v: u16) -> PropVar {
    let mut p = PropVar(PROPVARIANT::default());
    // SAFETY: writing the matching union member for the tag we set.
    unsafe {
        let a = &mut p.0.Anonymous.Anonymous;
        a.vt = VT_UI2;
        a.Anonymous.uiVal = v;
    }
    p
}

/// Copies the file and changes only its orientation tag in place.
fn retag(f: &IWICImagingFactory, job: &SaveJob) -> Result<()> {
    std::fs::copy(&job.src, &job.dst).map_err(|_| windows::core::Error::empty())?;
    let wide = HSTRING::from(job.dst.as_os_str());
    // SAFETY: COM calls on valid objects; the variant outlives the call.
    unsafe {
        (|| -> Result<()> {
            let dec = f.CreateDecoderFromFilename(&wide, None, GENERIC_READ | GENERIC_WRITE, WICDecodeMetadataCacheOnDemand)?;
            let frame = dec.GetFrame(0)?;
            let enc = f.CreateFastMetadataEncoderFromFrameDecode(&frame)?;
            let writer = enc.GetMetadataQueryWriter()?;
            let v = orientation_variant(job.orient.to_exif());
            writer.SetMetadataByName(w!("System.Photo.Orientation"), &v.0)?;
            enc.Commit()
        })()
    }
}

/// WIC flips before it rotates clockwise, as [`Orient`] does (see the
/// `wic_flip_rotator` test in `edit.rs`), so the fields map one to one.
fn transform_options(o: Orient) -> WICBitmapTransformOptions {
    let r = [WICBitmapTransformRotate0, WICBitmapTransformRotate90, WICBitmapTransformRotate180, WICBitmapTransformRotate270][usize::from(o.rot)];
    if o.flip { WICBitmapTransformOptions(r.0 | WICBitmapTransformFlipHorizontal.0) } else { r }
}

fn encode(f: &IWICImagingFactory, job: &SaveJob) -> std::result::Result<(), String> {
    let src_name = HSTRING::from(job.src.as_os_str());
    let dst_name = HSTRING::from(job.dst.as_os_str());
    // SAFETY: COM calls on valid objects; all pointers refer to locals that
    // outlive each call.
    unsafe {
        let dec = f.CreateDecoderFromFilename(&src_name, None, GENERIC_READ, WICDecodeMetadataCacheOnDemand).ctx("open")?;
        let frame = dec.GetFrame(0).ctx("open")?;

        let rot = f.CreateBitmapFlipRotator().ctx("rotate")?;
        rot.Initialize(&frame, transform_options(job.orient)).ctx("rotate")?;
        let mut source: IWICBitmapSource = rot.cast().ctx("rotate")?;
        if let Some((x, y, w, h)) = job.crop {
            let clip = f.CreateBitmapClipper().ctx("crop")?;
            let r = WICRect { X: x as i32, Y: y as i32, Width: w as i32, Height: h as i32 };
            clip.Initialize(&source, &r).ctx("crop")?;
            source = clip.cast().ctx("crop")?;
        }
        let (mut w, mut h) = (0, 0);
        source.GetSize(&mut w, &mut h).ctx("crop")?;

        let stream = f.CreateStream().ctx("create file")?;
        stream.InitializeFromFilename(&dst_name, GENERIC_WRITE.0).ctx("create file")?;
        let encoder = f.CreateEncoder(&job.container, std::ptr::null()).ctx("encoder")?;
        encoder.Initialize(&stream, WICBitmapEncoderNoCache).ctx("encoder")?;
        let (mut fe, mut bag) = (None, None);
        encoder.CreateNewFrame(&mut fe, &mut bag).ctx("encoder")?;
        let fe = fe.ok_or_else(windows::core::Error::empty).ctx("encoder")?;
        if let Some(bag) = &bag {
            if job.container == GUID_ContainerFormatJpeg || job.container == GUID_ContainerFormatHeif {
                let mut name: Vec<u16> = "ImageQuality".encode_utf16().chain(Some(0)).collect();
                let prop = PROPBAG2 { pstrName: windows::core::PWSTR(name.as_mut_ptr()), ..Default::default() };
                let mut var = VARIANT::default();
                (*var.Anonymous.Anonymous).vt = VT_R4;
                (*var.Anonymous.Anonymous).Anonymous.fltVal = 0.92;
                let _ = bag.Write(1, &prop, &var);
            }
        }
        fe.Initialize(bag.as_ref()).ctx("encoder")?;
        fe.SetSize(w, h).ctx("encoder")?;

        // Negotiate the pixel format; convert if the encoder wants another.
        let wanted = source.GetPixelFormat().ctx("pixel format")?;
        let mut fmt = wanted;
        fe.SetPixelFormat(&mut fmt).ctx("pixel format")?;
        if fmt != wanted {
            let conv = f.CreateFormatConverter().ctx("pixel format")?;
            conv.Initialize(&source, &fmt, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeMedianCut).ctx("pixel format")?;
            source = conv.cast().ctx("pixel format")?;
        }

        // Keep metadata (EXIF, XMP, …) and reset orientation: pixels are upright now.
        if let (Ok(bw), Ok(br)) = (fe.cast::<IWICMetadataBlockWriter>(), frame.cast::<IWICMetadataBlockReader>()) {
            if bw.InitializeFromBlockReader(&br).is_ok() {
                if let Ok(qw) = fe.GetMetadataQueryWriter() {
                    let v = orientation_variant(1);
                    let _ = qw.SetMetadataByName(w!("System.Photo.Orientation"), &v.0);
                }
            }
        }
        let contexts: Vec<Option<IWICColorContext>> = color_contexts(f, &frame).into_iter().map(Some).collect();
        if !contexts.is_empty() {
            let _ = fe.SetColorContexts(&contexts);
        }

        fe.WriteSource(&source, std::ptr::null()).ctx("write")?;
        fe.Commit().ctx("write")?;
        encoder.Commit().ctx("write")?;
        // File streams flush on release; Commit is optional for them.
        let _ = stream.Commit(STGC_DEFAULT);
        Ok(())
    }
}
