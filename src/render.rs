//! Direct2D / DirectWrite back end that executes [`Cmd`] lists.

use std::cell::RefCell;
use std::collections::HashMap;

use windows::Win32::Foundation::{D2DERR_RECREATE_TARGET, HWND};
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::DirectWrite::*;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::UI::WindowsAndMessaging::{
    NONCLIENTMETRICSW, SPI_GETNONCLIENTMETRICS, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW,
};
use windows::core::{BOOL, HSTRING, Interface, Result, w};
use windows_numerics::{Matrix3x2, Vector2};

use crate::geom::{Affine, Rect};
use crate::icons::{Icon, Paint, Seg, parse_path};
use crate::theme::Color;
use crate::ui::{Align, Cmd, Measure, TextStyle};

fn color(c: Color) -> D2D1_COLOR_F {
    D2D1_COLOR_F { r: c.r, g: c.g, b: c.b, a: c.a }
}

fn rect(r: Rect) -> D2D_RECT_F {
    D2D_RECT_F { left: r.x, top: r.y, right: r.right(), bottom: r.bottom() }
}

fn v2(x: f32, y: f32) -> Vector2 {
    Vector2 { X: x, Y: y }
}

fn matrix(m: Affine) -> Matrix3x2 {
    Matrix3x2 { M11: m.m11, M12: m.m12, M21: m.m21, M22: m.m22, M31: m.dx, M32: m.dy }
}

/// The user's message font (Segoe UI on a stock Windows install).
fn system_ui_font() -> Option<HSTRING> {
    let mut m = NONCLIENTMETRICSW { cbSize: std::mem::size_of::<NONCLIENTMETRICSW>() as u32, ..Default::default() };
    // SAFETY: the struct size is set as the API requires.
    unsafe {
        SystemParametersInfoW(
            SPI_GETNONCLIENTMETRICS,
            m.cbSize,
            Some((&mut m as *mut NONCLIENTMETRICSW).cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
        .ok()?;
    }
    let face = &m.lfMessageFont.lfFaceName;
    let len = face.iter().position(|&c| c == 0).unwrap_or(face.len());
    (len > 0).then(|| HSTRING::from_wide(&face[..len]))
}

/// Measured sizes by (text, style, maximum width bits, wrap).
type SizeKey = (String, TextStyle, u32, bool);

/// DirectWrite text formatting; device independent, so it also serves layout.
pub struct Text {
    factory: IDWriteFactory,
    family: HSTRING,
    formats: RefCell<HashMap<TextStyle, (IDWriteTextFormat, IDWriteInlineObject)>>,
    /// Hit testing lays the UI out on every mouse move; measuring the same
    /// strings again would create a DirectWrite layout each time.
    sizes: RefCell<HashMap<SizeKey, (f32, f32)>>,
}

impl Text {
    fn new() -> Result<Self> {
        // SAFETY: plain factory creation.
        let factory: IDWriteFactory = unsafe { DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)? };
        let mut fonts = None;
        // SAFETY: out-pointer to a local Option.
        unsafe { factory.GetSystemFontCollection(&mut fonts, false)? };
        let fonts = fonts.ok_or_else(windows::core::Error::empty)?;
        // Adwaita's own face first, then its basis, then the Windows UI font.
        let family = ["Adwaita Sans", "Inter", "Segoe UI Variable Text"]
            .into_iter()
            .map(HSTRING::from)
            .chain(system_ui_font())
            .find(|name| {
                let (mut index, mut exists) = (0u32, BOOL(0));
                // SAFETY: out-pointers to locals.
                unsafe { fonts.FindFamilyName(name, &mut index, &mut exists) }.is_ok() && exists.as_bool()
            })
            .unwrap_or_else(|| HSTRING::from("Segoe UI"));
        Ok(Self { factory, family, formats: RefCell::default(), sizes: RefCell::default() })
    }

    fn format(&self, style: TextStyle) -> Result<(IDWriteTextFormat, IDWriteInlineObject)> {
        if let Some(f) = self.formats.borrow().get(&style) {
            return Ok(f.clone());
        }
        let (size, weight) = style.metrics();
        // SAFETY: arguments are valid for the duration of the calls.
        let pair = unsafe {
            let f = self.factory.CreateTextFormat(
                &self.family,
                None,
                DWRITE_FONT_WEIGHT(i32::from(weight)),
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                size,
                w!(""),
            )?;
            let sign = self.factory.CreateEllipsisTrimmingSign(&f)?;
            (f, sign)
        };
        self.formats.borrow_mut().insert(style, pair.clone());
        Ok(pair)
    }

    fn layout(&self, s: &str, style: TextStyle, w: f32, h: f32, align: Align, wrap: bool) -> Result<IDWriteTextLayout> {
        let (format, sign) = self.format(style)?;
        let wide: Vec<u16> = s.encode_utf16().collect();
        // SAFETY: `wide` outlives the call; DirectWrite copies the string.
        unsafe {
            let l = self.factory.CreateTextLayout(&wide, &format, w.max(0.0), h.max(0.0))?;
            l.SetTextAlignment(match align {
                Align::Start => DWRITE_TEXT_ALIGNMENT_LEADING,
                Align::Centre => DWRITE_TEXT_ALIGNMENT_CENTER,
                Align::End => DWRITE_TEXT_ALIGNMENT_TRAILING,
            })?;
            if wrap {
                l.SetWordWrapping(DWRITE_WORD_WRAPPING_WRAP)?;
            } else {
                l.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
                l.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
                let trim = DWRITE_TRIMMING { granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER, ..Default::default() };
                l.SetTrimming(&trim, &sign)?;
            }
            Ok(l)
        }
    }
}

impl Measure for Text {
    fn measure(&self, s: &str, style: TextStyle, max_w: f32, wrap: bool) -> (f32, f32) {
        let max_w = if max_w.is_finite() { max_w } else { 1e6 };
        let key = (s.to_owned(), style, max_w.to_bits(), wrap);
        if let Some(&size) = self.sizes.borrow().get(&key) {
            return size;
        }
        let Ok(l) = self.layout(s, style, max_w, 1e6, Align::Start, wrap) else { return (0.0, 0.0) };
        let mut m = DWRITE_TEXT_METRICS::default();
        // SAFETY: out-pointer to a local.
        if unsafe { l.GetMetrics(&mut m) }.is_err() {
            return (0.0, 0.0);
        }
        let size = (m.widthIncludingTrailingWhitespace.ceil(), m.height.ceil());
        let mut sizes = self.sizes.borrow_mut();
        // File names change while browsing: keep the cache bounded.
        if sizes.len() >= 1024 {
            sizes.clear();
        }
        sizes.insert(key, size);
        size
    }
}

#[derive(Clone)]
struct Target {
    rt: ID2D1HwndRenderTarget,
    dc: ID2D1DeviceContext,
    brush: ID2D1SolidColorBrush,
}

pub struct Renderer {
    factory: ID2D1Factory1,
    hwnd: HWND,
    target: Option<Target>,
    pub text: Text,
    round: ID2D1StrokeStyle,
    icons: HashMap<Icon, Vec<(ID2D1PathGeometry, Paint)>>,
    dpi: f32,
}

/// Outcome of a frame.
pub enum Frame {
    Ok,
    /// The GPU device was lost; every bitmap must be recreated.
    DeviceLost,
}

impl Renderer {
    pub fn new(hwnd: HWND, dpi: f32) -> Result<Self> {
        // SAFETY: plain factory creation; options pointer is to a local.
        unsafe {
            let factory: ID2D1Factory1 = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            let props = D2D1_STROKE_STYLE_PROPERTIES {
                startCap: D2D1_CAP_STYLE_ROUND,
                endCap: D2D1_CAP_STYLE_ROUND,
                dashCap: D2D1_CAP_STYLE_ROUND,
                lineJoin: D2D1_LINE_JOIN_ROUND,
                miterLimit: 10.0,
                ..Default::default()
            };
            let round = ID2D1Factory::CreateStrokeStyle(&factory, &props, None)?;
            Ok(Self { round, factory, hwnd, target: None, text: Text::new()?, icons: HashMap::new(), dpi })
        }
    }

    pub fn set_dpi(&mut self, dpi: f32) {
        self.dpi = dpi;
        if let Some(t) = &self.target {
            // SAFETY: valid render target.
            unsafe { t.rt.SetDpi(dpi, dpi) };
        }
    }

    pub fn resize(&mut self, w: u32, h: u32) {
        if let Some(t) = &self.target {
            // SAFETY: valid render target; size pointer is to a local.
            if unsafe { t.rt.Resize(&D2D_SIZE_U { width: w, height: h }) }.is_err() {
                self.target = None;
            }
        }
    }

    fn target(&mut self, w: u32, h: u32) -> Result<&Target> {
        if self.target.is_none() {
            let props = D2D1_RENDER_TARGET_PROPERTIES {
                pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
                dpiX: self.dpi,
                dpiY: self.dpi,
                ..Default::default()
            };
            let hwnd_props = D2D1_HWND_RENDER_TARGET_PROPERTIES {
                hwnd: self.hwnd,
                pixelSize: D2D_SIZE_U { width: w, height: h },
                presentOptions: D2D1_PRESENT_OPTIONS_NONE,
            };
            // SAFETY: property structs are locals; hwnd is our live window.
            unsafe {
                let rt = self.factory.CreateHwndRenderTarget(&props, &hwnd_props)?;
                rt.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
                let dc: ID2D1DeviceContext = rt.cast()?;
                let brush = rt.CreateSolidColorBrush(&color(Color::BLACK), None)?;
                self.target = Some(Target { rt, dc, brush });
            }
        }
        Ok(self.target.as_ref().expect("target was just created"))
    }

    /// Largest bitmap edge the device accepts.
    pub fn max_bitmap(&mut self, w: u32, h: u32) -> u32 {
        // SAFETY: valid render target.
        self.target(w, h).map(|t| unsafe { t.rt.GetMaximumBitmapSize() }).unwrap_or(8192)
    }

    /// Uploads premultiplied BGRA pixels.
    pub fn bitmap(&mut self, size: (u32, u32), win: (u32, u32), pixels: &[u8]) -> Result<ID2D1Bitmap> {
        let t = self.target(win.0, win.1)?.clone();
        let props = D2D1_BITMAP_PROPERTIES {
            pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
            dpiX: 96.0,
            dpiY: 96.0,
        };
        assert_eq!(pixels.len(), size.0 as usize * size.1 as usize * 4);
        // SAFETY: `pixels` holds exactly width × height × 4 bytes (asserted).
        unsafe { t.rt.CreateBitmap(D2D_SIZE_U { width: size.0, height: size.1 }, Some(pixels.as_ptr().cast()), size.0 * 4, &props) }
    }

    /// A `size` copy of `b`, scaled on the GPU with high-quality cubic
    /// filtering: cheaper than decoding the file again.
    pub fn shrink(&mut self, b: &ID2D1Bitmap, size: (u32, u32), win: (u32, u32)) -> Result<ID2D1Bitmap> {
        let t = self.target(win.0, win.1)?.clone();
        let (w, h) = (size.0 as f32, size.1 as f32);
        // SAFETY: drawing happens between BeginDraw and EndDraw on a fresh
        // target; every pointer argument refers to a local.
        unsafe {
            // Equal DIP and pixel sizes give the copy 96 DPI, as decoded bitmaps have.
            let rt = t.rt.CreateCompatibleRenderTarget(
                Some(&D2D_SIZE_F { width: w, height: h }),
                Some(&D2D_SIZE_U { width: size.0, height: size.1 }),
                None,
                D2D1_COMPATIBLE_RENDER_TARGET_OPTIONS_NONE,
            )?;
            let dc: ID2D1DeviceContext = rt.cast()?;
            rt.BeginDraw();
            let dest = D2D_RECT_F { left: 0.0, top: 0.0, right: w, bottom: h };
            dc.DrawBitmap(b, Some(&dest), 1.0, D2D1_INTERPOLATION_MODE_HIGH_QUALITY_CUBIC, None, None);
            rt.EndDraw(None, None)?;
            rt.GetBitmap()
        }
    }

    fn icon_geometry(&mut self, icon: Icon) -> Result<Vec<(ID2D1PathGeometry, Paint)>> {
        if let Some(g) = self.icons.get(&icon) {
            return Ok(g.clone());
        }
        let mut out = Vec::new();
        for &(d, paint) in icon.parts() {
            let segs = parse_path(d).ok_or_else(windows::core::Error::empty)?;
            out.push((self.path(&segs, paint == Paint::Fill)?, paint));
        }
        self.icons.insert(icon, out.clone());
        Ok(out)
    }

    fn path(&self, segs: &[Seg], filled: bool) -> Result<ID2D1PathGeometry> {
        let begin = if filled { D2D1_FIGURE_BEGIN_FILLED } else { D2D1_FIGURE_BEGIN_HOLLOW };
        // SAFETY: the sink is used and closed within this block.
        unsafe {
            let g: ID2D1PathGeometry = self.factory.CreatePathGeometry()?.into();
            let sink = g.Open()?;
            let mut open = false;
            for s in segs {
                match *s {
                    Seg::Move(x, y) => {
                        if open {
                            sink.EndFigure(D2D1_FIGURE_END_OPEN);
                        }
                        sink.BeginFigure(v2(x, y), begin);
                        open = true;
                    }
                    Seg::Line(x, y) => sink.AddLine(v2(x, y)),
                    Seg::Cubic(x1, y1, x2, y2, x, y) => {
                        sink.AddBezier(&D2D1_BEZIER_SEGMENT { point1: v2(x1, y1), point2: v2(x2, y2), point3: v2(x, y) })
                    }
                    Seg::Quad(x1, y1, x, y) => sink.AddQuadraticBezier(&D2D1_QUADRATIC_BEZIER_SEGMENT { point1: v2(x1, y1), point2: v2(x, y) }),
                    Seg::Close => {
                        if open {
                            sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                            open = false;
                        }
                    }
                }
            }
            if open {
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
            }
            sink.Close()?;
            Ok(g)
        }
    }

    pub fn render(&mut self, size: (u32, u32), clear: Color, cmds: &[Cmd], image: Option<&ID2D1Bitmap>) -> Result<Frame> {
        // Build icon geometries before borrowing the target.
        for c in cmds {
            if let Cmd::Icon { icon, .. } = c {
                self.icon_geometry(*icon)?;
            }
        }
        let t = self.target(size.0, size.1)?.clone();
        let (rt, dc, brush) = (&t.rt, &t.dc, &t.brush);
        let id = matrix(Affine::IDENTITY);
        // SAFETY: all calls happen between BeginDraw and EndDraw on a live
        // target; every pointer argument refers to a local.
        let res = unsafe {
            rt.BeginDraw();
            rt.SetTransform(&id);
            rt.Clear(Some(&color(clear)));
            for c in cmds {
                match c {
                    Cmd::Fill { r, radius, c } => {
                        brush.SetColor(&color(*c));
                        if *radius > 0.0 {
                            let rr = D2D1_ROUNDED_RECT { rect: rect(*r), radiusX: *radius, radiusY: *radius };
                            rt.FillRoundedRectangle(&rr, brush);
                        } else {
                            rt.FillRectangle(&rect(*r), brush);
                        }
                    }
                    Cmd::Stroke { r, radius, c, w } => {
                        brush.SetColor(&color(*c));
                        let rr = D2D1_ROUNDED_RECT { rect: rect(*r), radiusX: *radius, radiusY: *radius };
                        rt.DrawRoundedRectangle(&rr, brush, *w, None);
                    }
                    Cmd::Line { x0, y0, x1, y1, c, w } => {
                        brush.SetColor(&color(*c));
                        rt.DrawLine(v2(*x0, *y0), v2(*x1, *y1), brush, *w, &self.round);
                    }
                    Cmd::Text { r, s, style, c, align, wrap } => {
                        if let Ok(l) = self.text.layout(s, *style, r.w, r.h, *align, *wrap) {
                            brush.SetColor(&color(*c));
                            rt.DrawTextLayout(v2(r.x, r.y), &l, brush, D2D1_DRAW_TEXT_OPTIONS_CLIP);
                        }
                    }
                    Cmd::Icon { icon, r, c } => {
                        brush.SetColor(&color(*c));
                        let s = r.w / 16.0;
                        rt.SetTransform(&matrix(Affine::scale(s, s).then(Affine::translate(r.x, r.y))));
                        for (g, paint) in &self.icons[icon] {
                            match paint {
                                Paint::Fill => rt.FillGeometry(g, brush, None),
                                Paint::Stroke(w) => rt.DrawGeometry(g, brush, *w, &self.round),
                            }
                        }
                        rt.SetTransform(&id);
                    }
                    Cmd::Image { m, smooth } => {
                        if let Some(b) = image {
                            dc.SetTransform(&matrix(*m));
                            let mode = if *smooth { D2D1_INTERPOLATION_MODE_HIGH_QUALITY_CUBIC } else { D2D1_INTERPOLATION_MODE_NEAREST_NEIGHBOR };
                            dc.DrawBitmap(b, None, 1.0, mode, None, None);
                            dc.SetTransform(&id);
                        }
                    }
                    Cmd::Arc { cx, cy, radius, start, sweep, c, w } => {
                        let end = start + sweep;
                        let from = (cx + radius * start.cos(), cy + radius * start.sin());
                        let mut seg = vec![Seg::Move(from.0, from.1)];
                        let to = (cx + radius * end.cos(), cy + radius * end.sin());
                        let large = *sweep > std::f32::consts::PI;
                        crate::icons::arc_to_cubics(&mut seg, from, *radius, *radius, 0.0, large, true, to);
                        if let Ok(g) = self.path(&seg, false) {
                            brush.SetColor(&color(*c));
                            rt.DrawGeometry(&g, brush, *w, &self.round);
                        }
                    }
                    Cmd::Clip(r) => rt.PushAxisAlignedClip(&rect(*r), D2D1_ANTIALIAS_MODE_ALIASED),
                    Cmd::Unclip => rt.PopAxisAlignedClip(),
                }
            }
            rt.EndDraw(None, None)
        };
        match res {
            Ok(()) => Ok(Frame::Ok),
            Err(e) if e.code() == D2DERR_RECREATE_TARGET => {
                self.target = None;
                Ok(Frame::DeviceLost)
            }
            Err(e) => Err(e),
        }
    }
}
