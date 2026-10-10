//! Application state and behaviour. Platform calls that can re-enter the
//! window procedure (dialogs, window placement) are returned as [`Effect`]s
//! and run by the host after it has released its borrow of the app.

use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, channel};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Direct2D::ID2D1Bitmap;
use windows::Win32::Graphics::Imaging::{GUID_ContainerFormatAdng, GUID_ContainerFormatHeif, GUID_ContainerFormatJpeg, GUID_ContainerFormatPng};
use windows::Win32::UI::WindowsAndMessaging::WM_APP;

use crate::edit::{self, Aspect, Handle, Orient};
use crate::geom::Rect;
use crate::render::{Frame, Renderer};
use crate::settings::{SLOTS, Settings};
use crate::sys::Poster;
use crate::theme::{self, Color, Theme};
use crate::ui::{Action, Widget};
use crate::wic::{self, Decoder, Job, Meta, Outcome, Wic};
use crate::{sys, view};

pub const WM_TASK: u32 = WM_APP + 2;
pub const HEADER_H: f32 = 47.0;
pub const SIDEBAR_W: f32 = 320.0;
pub const TOOLBAR_H: f32 = 47.0;
pub const BANNER_H: f32 = 46.0;
const UPDATE_INTERVAL: u64 = 24 * 60 * 60;
const ZOOM_STEP: f32 = 1.25;
const MAX_ZOOM: f32 = 32.0;
const ANIM: Duration = Duration::from_millis(180);
const OSD_IDLE: Duration = Duration::from_secs(2);
const TOAST_TIME: Duration = Duration::from_secs(3);
const TOAST_FADE: Duration = Duration::from_millis(200);
const TOOLTIP_DELAY: Duration = Duration::from_millis(600);
const SETTINGS_LOCK: windows::core::PCWSTR = windows::core::w!("Local\\Windmolen.Settings");

/// Work the host must do after releasing the app.
pub enum Effect {
    OpenDialog,
    PickMarkDir {
        slot: u8,
        toggle: bool,
    },
    Fullscreen(bool),
    Minimize,
    ToggleMaximize,
    Close,
    Title(String),
    Install,
    DefaultApps,
    OpenUrl(String),
    /// Starts the (updated) program again with the current file, then closes.
    Restart(Option<PathBuf>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UpdateState {
    Idle,
    Checking {
        manual: bool,
    },
    /// Installed; takes effect after a restart.
    Ready(String),
}

/// A bitmap on the GPU and its size in pixels.
#[derive(Clone)]
pub struct Bitmap {
    pub b: ID2D1Bitmap,
    pub w: u32,
    pub h: u32,
}

pub struct Loaded {
    /// The best bitmap so far (may be smaller than the source).
    pub bitmap: Bitmap,
    /// A screen-sized copy of a larger `bitmap`, drawn while it still has a
    /// pixel for each one on screen: every frame then filters a few
    /// megapixels instead of the whole photo.
    pub preview: Option<Bitmap>,
    pub meta: Meta,
    /// No better bitmap is needed or possible: decoded at its own size or
    /// at the device maximum, or an attempt to do so failed.
    pub full: bool,
}

impl Loaded {
    /// What to draw at `px` device pixels per source pixel, and whether
    /// that is the preview.
    pub fn pick(&self, px: f32) -> (&Bitmap, bool) {
        match &self.preview {
            Some(p) if px * self.meta.width as f32 <= p.w as f32 * 1.001 => (p, true),
            _ => (&self.bitmap, false),
        }
    }
}

pub enum Slot {
    Ready(Loaded),
    Failed(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Popover {
    Main,
    Marks,
    Aspect,
    Save,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dialog {
    Shortcuts,
    About,
    ManageMarks,
    Send,
    Replace,
    Install,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    pub fit: bool,
    /// DIPs per source pixel.
    pub s: f32,
    /// Oriented source pixel at the viewport centre.
    pub cx: f32,
    pub cy: f32,
}

impl View {
    const FIT: Self = Self { fit: true, s: 1.0, cx: 0.0, cy: 0.0 };
}

pub struct EditState {
    pub orient: Orient,
    pub crop: Rect,
    pub aspect: Aspect,
}

struct CropDrag {
    handle: Handle,
    start: Rect,
    mouse: (f32, f32),
    scale: f32,
}

struct PanDrag {
    mouse: (f32, f32),
    c: (f32, f32),
}

pub struct Toast {
    pub text: String,
    pub born: Instant,
    pub sticky: bool,
}

pub enum TaskMsg {
    Update { outcome: crate::update::Outcome, manual: bool },
    Progress(String),
    Sent { mode: SendMode, done: Vec<(PathBuf, OsString, u16)>, missing: Vec<(PathBuf, OsString)>, failed: Vec<String> },
    Saved { result: Result<PathBuf, String>, replaced: bool },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SendMode {
    Copy,
    Move,
}

/// A save or send in progress: files are being written.
struct Work {
    /// What it does, as Windows shows it when the app delays a shutdown.
    what: &'static str,
    /// Set to stop it; half-written files are then removed.
    cancel: Arc<AtomicBool>,
    thread: JoinHandle<()>,
}

pub struct App {
    pub r: Renderer,
    pub exts: Vec<String>,
    ext_set: HashSet<String>,
    decoder: Decoder,
    decoded: Receiver<Outcome>,
    tasks: Poster<TaskMsg>,
    task_rx: Receiver<TaskMsg>,
    work: Option<Work>,
    /// Requests to close while `work` runs: see [`may_close`](Self::may_close).
    close_requests: u8,
    /// Whether an installed copy exists, and whether this is it.
    pub installed: Option<bool>,
    pub update: UpdateState,

    pub settings: Settings,
    settings_path: Option<PathBuf>,

    pub theme: Theme,
    animations: bool,
    pub dpi: f32,
    pub size: (u32, u32),
    /// Longer side of the window's monitor in pixels: the largest a fitted
    /// image can appear, and so the size neighbours are kept at.
    screen_edge: u32,
    pub active: bool,
    pub maximised: bool,
    pub fullscreen: bool,

    pub folder: Option<PathBuf>,
    /// Browsing an explicit set of files rather than a folder.
    pub selection: bool,
    /// Files this window was launched with (grows while gathering).
    launch: Vec<PathBuf>,
    pub files: Vec<PathBuf>,
    pub index: usize,
    pub cache: HashMap<PathBuf, Slot>,
    loading_since: Option<Instant>,

    pub view: View,
    anim: Option<(View, Instant)>,
    pub edit: Option<EditState>,
    pub popover: Option<Popover>,
    pub dialog: Option<Dialog>,
    pub scroll: f32,
    pub props_scroll: f32,

    pub hover: Option<Action>,
    pub pressed: Option<Action>,
    pub focus: Option<Action>,
    pub mouse: (f32, f32),
    mouse_inside: bool,
    hover_since: Instant,
    pan: Option<PanDrag>,
    crop_drag: Option<CropDrag>,
    last_motion: Instant,
    pub osd: f32,
    pub toast: Option<Toast>,
    dirty: bool,
    pub now: Instant,
}

fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - t.clamp(0.0, 1.0)).powi(3)
}

impl App {
    pub fn new(hwnd: HWND, dpi: f32) -> windows::core::Result<Self> {
        let r = Renderer::new(hwnd, dpi)?;
        let wic = Wic::new()?;
        let ext_set = wic.extensions();
        let mut exts: Vec<String> = ext_set.iter().cloned().collect();
        exts.sort();
        let (tx, decoded) = channel();
        let (task_tx, task_rx) = channel();
        let settings_path = sys::settings_path();
        let (settings, unreadable) = match settings_path.as_deref().map(Settings::load) {
            Some(Err(e)) => (Settings::default(), Some(e)),
            loaded => (loaded.and_then(Result::ok).unwrap_or_default(), None),
        };
        let now = Instant::now();
        let mut app = Self {
            r,
            exts,
            ext_set,
            decoder: Decoder::new(Poster::new(tx, hwnd, wic::WM_DECODED)),
            decoded,
            tasks: Poster::new(task_tx, hwnd, WM_TASK),
            task_rx,
            work: None,
            close_requests: 0,
            installed: None,
            update: UpdateState::Idle,
            settings,
            settings_path,
            theme: Self::system_theme(),
            animations: sys::animations_enabled(),
            dpi,
            size: (1, 1),
            screen_edge: 1920,
            active: true,
            maximised: false,
            fullscreen: false,
            folder: None,
            selection: false,
            launch: Vec::new(),
            files: Vec::new(),
            index: 0,
            cache: HashMap::new(),
            loading_since: None,
            view: View::FIT,
            anim: None,
            edit: None,
            popover: None,
            dialog: None,
            scroll: 0.0,
            props_scroll: 0.0,
            hover: None,
            pressed: None,
            focus: None,
            mouse: (-1.0, -1.0),
            mouse_inside: false,
            hover_since: now,
            pan: None,
            crop_drag: None,
            last_motion: now - OSD_IDLE,
            osd: 0.0,
            toast: None,
            dirty: true,
            now,
        };
        if let Some(e) = unreadable {
            app.settings_unreadable(&e);
        }
        Ok(app)
    }

    fn settings_unreadable(&mut self, e: &std::io::Error) {
        self.show_toast(&format!("Could not read your settings ({e}), so changes are not saved"));
    }

    fn system_theme() -> Theme {
        let accent = sys::accent_rgb().map_or(theme::ACCENTS[0], |(r, g, b)| theme::nearest_accent(Color::rgb(r, g, b)));
        Theme::new(sys::prefers_dark(), accent)
    }

    pub fn reload_theme(&mut self) {
        self.theme = Self::system_theme();
        self.animations = sys::animations_enabled();
        self.dirty = true;
    }

    pub fn invalidate(&mut self) {
        self.dirty = true;
    }

    pub fn take_dirty(&mut self) -> bool {
        std::mem::take(&mut self.dirty)
    }

    // --------------------------------------------------------------------
    // Geometry
    // --------------------------------------------------------------------

    pub fn width(&self) -> f32 {
        self.size.0 as f32 * 96.0 / self.dpi
    }

    pub fn height(&self) -> f32 {
        self.size.1 as f32 * 96.0 / self.dpi
    }

    pub fn dpi_scale(&self) -> f32 {
        self.dpi / 96.0
    }

    /// Header height in layout; zero in fullscreen (it overlays instead).
    pub fn header_h(&self) -> f32 {
        if self.fullscreen { 0.0 } else { HEADER_H }
    }

    pub fn header_visible(&self) -> bool {
        !self.fullscreen || self.edit.is_some() || self.popover.is_some() || (self.mouse_inside && self.mouse.1 < HEADER_H && self.pan.is_none())
    }

    pub fn banner_visible(&self) -> bool {
        matches!(self.update, UpdateState::Ready(_)) && !self.fullscreen && self.edit.is_none()
    }

    /// Top of the content area: below the header bar and any banner.
    pub fn content_top(&self) -> f32 {
        self.header_h() + if self.banner_visible() { BANNER_H } else { 0.0 }
    }

    pub fn sidebar_open(&self) -> bool {
        self.settings.properties && self.edit.is_none() && self.loaded().is_some() && self.width() >= 600.0
    }

    /// Area the image is shown in (view mode).
    pub fn viewport(&self) -> Rect {
        let top = if self.edit.is_some() && self.fullscreen { HEADER_H } else { self.content_top() };
        let right = if self.sidebar_open() { SIDEBAR_W } else { 0.0 };
        Rect::new(0.0, top, (self.width() - right).max(1.0), (self.height() - top).max(1.0))
    }

    pub fn current(&self) -> Option<&Path> {
        self.files.get(self.index).map(PathBuf::as_path)
    }

    pub fn loaded(&self) -> Option<&Loaded> {
        match self.cache.get(self.current()?)? {
            Slot::Ready(l) => Some(l),
            Slot::Failed(_) => None,
        }
    }

    pub fn failure(&self) -> Option<&str> {
        match self.cache.get(self.current()?)? {
            Slot::Failed(e) => Some(e),
            Slot::Ready(_) => None,
        }
    }

    pub fn loading(&self) -> bool {
        self.current().is_some() && self.loaded().is_none() && self.failure().is_none()
    }

    pub fn show_spinner(&self) -> bool {
        self.loading() && self.loading_since.is_some_and(|t| self.now - t > Duration::from_millis(300))
    }

    /// Orientation used for display: the file's own, plus the edit.
    pub fn orient(&self) -> Option<Orient> {
        let base = self.loaded()?.meta.orient;
        Some(self.edit.as_ref().map_or(base, |e| base.then(e.orient)))
    }

    /// Oriented size of the current image in source pixels.
    pub fn image_size(&self) -> Option<(f32, f32)> {
        let l = self.loaded()?;
        Some(self.orient()?.apply_size(l.meta.width as f32, l.meta.height as f32))
    }

    fn fit_scale(&self, vp: Rect, w: f32, h: f32) -> f32 {
        (vp.w / w).min(vp.h / h).min(1.0 / self.dpi_scale())
    }

    fn clamp_view(&self, mut v: View) -> View {
        let Some((w, h)) = self.image_size() else { return v };
        let vp = self.viewport();
        if v.fit {
            v.s = self.fit_scale(vp, w, h);
        }
        let (hw, hh) = (vp.w / (2.0 * v.s), vp.h / (2.0 * v.s));
        v.cx = if w <= 2.0 * hw { w / 2.0 } else { v.cx.clamp(hw, w - hw) };
        v.cy = if h <= 2.0 * hh { h / 2.0 } else { v.cy.clamp(hh, h - hh) };
        v
    }

    /// The view currently on screen, including any running animation.
    pub fn effective_view(&self) -> View {
        let target = self.clamp_view(self.view);
        match self.anim {
            Some((from, start)) => {
                let t = ease_out((self.now - start).as_secs_f32() / ANIM.as_secs_f32());
                let l = |a: f32, b: f32| a + (b - a) * t;
                View { fit: target.fit, s: from.s * (target.s / from.s).powf(t), cx: l(from.cx, target.cx), cy: l(from.cy, target.cy) }
            }
            None => target,
        }
    }

    /// Screen rectangle of the image in edit mode, and DIPs per pixel.
    pub fn edit_geometry(&self) -> Option<(Rect, f32)> {
        let (w, h) = self.image_size()?;
        let vp = self.viewport();
        let area = Rect::new(vp.x, vp.y, vp.w, (vp.h - TOOLBAR_H).max(1.0)).inset(32.0);
        let s = (area.w / w).min(area.h / h).max(1e-4);
        Some((area.centred(w * s, h * s), s))
    }

    pub fn zoom_percent(&self) -> f32 {
        self.effective_view().s * self.dpi_scale() * 100.0
    }

    // --------------------------------------------------------------------
    // Files
    // --------------------------------------------------------------------

    fn is_image(&self, p: &Path) -> bool {
        p.extension().is_some_and(|e| self.ext_set.contains(&e.to_string_lossy().to_lowercase()))
    }

    fn scan(&self, dir: &Path) -> Vec<PathBuf> {
        let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
            .map(|rd| rd.flatten().filter(|e| e.file_type().is_ok_and(|t| t.is_file())).map(|e| e.path()).filter(|p| self.is_image(p)).collect())
            .unwrap_or_default();
        v.sort_by(|a, b| sys::natural_cmp(a, b));
        v
    }

    /// Opens what the window was started with: one file browses its
    /// folder, several files are browsed on their own.
    pub fn open_launch(&mut self, paths: Vec<PathBuf>) -> Vec<Effect> {
        self.launch.clear();
        self.add_launch(paths)
    }

    /// More files from the same Explorer action (see `instance`). Paths
    /// are absolute: the processes that send them have their own directory.
    pub fn add_launch(&mut self, paths: Vec<PathBuf>) -> Vec<Effect> {
        for p in paths {
            if !self.launch.contains(&p) {
                self.launch.push(p);
            }
        }
        match self.launch.as_slice() {
            [] => Vec::new(),
            [one] => {
                let one = one.clone();
                self.open_path(&one)
            }
            many => {
                let many = many.to_vec();
                self.open_selection(many)
            }
        }
    }

    fn open_selection(&mut self, mut files: Vec<PathBuf>) -> Vec<Effect> {
        files.retain(|p| p.is_file());
        files.sort_by(|a, b| sys::natural_cmp(a, b));
        files.dedup();
        if files.is_empty() {
            return Vec::new();
        }
        let index = self.current().and_then(|c| files.iter().position(|p| p == c)).unwrap_or(0);
        self.folder = None;
        self.selection = true;
        self.browse(files, index)
    }

    /// Opens a file (browsing its folder) or the first image in a folder.
    pub fn open_path(&mut self, path: &Path) -> Vec<Effect> {
        if self.edit.is_some() {
            self.show_toast("Save or cancel the edit first");
            return Vec::new();
        }
        let path = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
        let (dir, file) =
            if path.is_dir() { (path.clone(), None) } else { (path.parent().map(Path::to_path_buf).unwrap_or_default(), Some(path.clone())) };
        let mut files = self.scan(&dir);
        let index = match &file {
            Some(f) => match files.iter().position(|p| p == f) {
                Some(i) => i,
                None => {
                    // Opened explicitly despite an unknown extension: try anyway.
                    files.push(f.clone());
                    files.sort_by(|a, b| sys::natural_cmp(a, b));
                    files.iter().position(|p| p == f).unwrap_or(0)
                }
            },
            None => 0,
        };
        if files.is_empty() {
            self.show_toast("No images in this folder");
            return Vec::new();
        }
        self.folder = Some(dir);
        self.selection = false;
        // An explicit open reads the files afresh.
        self.cache.clear();
        self.browse(files, index)
    }

    /// Re-reads the folder (or selection), keeping the current file when it
    /// still exists.
    fn rescan(&mut self) -> Vec<Effect> {
        let files = if self.selection {
            self.files.iter().filter(|p| p.is_file()).cloned().collect()
        } else if let Some(dir) = &self.folder {
            self.scan(dir)
        } else {
            return Vec::new();
        };
        let i = self.current().and_then(|c| files.iter().position(|p| p == c)).unwrap_or(self.index);
        self.browse(files, i)
    }

    /// Replaces the file list and shows `files[i]`.
    fn browse(&mut self, files: Vec<PathBuf>, i: usize) -> Vec<Effect> {
        let shown = self.current().map(Path::to_path_buf);
        self.files = files;
        self.show(i, shown)
    }

    fn go(&mut self, i: usize) -> Vec<Effect> {
        let shown = self.current().map(Path::to_path_buf);
        self.show(i, shown)
    }

    /// Makes `files[i]` current. Another image than `shown` starts with a
    /// fresh view and ends any edit, which belonged to the old one.
    fn show(&mut self, i: usize, shown: Option<PathBuf>) -> Vec<Effect> {
        self.index = i.min(self.files.len().saturating_sub(1));
        if self.current() != shown.as_deref() {
            self.view = View::FIT;
            self.anim = None;
            self.props_scroll = 0.0;
            self.edit = None;
            self.crop_drag = None;
        }
        self.loading_since = Some(Instant::now());
        self.request_decodes();
        self.dirty = true;
        let title = match self.current().and_then(Path::file_name) {
            Some(name) => format!("{} – Windmolen", name.to_string_lossy()),
            None => "Windmolen".into(),
        };
        vec![Effect::Title(title)]
    }

    /// Next or previous image; not while editing.
    fn step(&mut self, d: isize) -> Vec<Effect> {
        let n = self.files.len() as isize;
        if n < 2 || self.edit.is_some() {
            return Vec::new();
        }
        self.go((self.index as isize + d).rem_euclid(n) as usize)
    }

    fn wanted(&self) -> Vec<PathBuf> {
        let n = self.files.len();
        if n == 0 {
            return Vec::new();
        }
        let mut v = vec![self.files[self.index].clone()];
        for j in [self.index + 1, self.index + n - 1] {
            let p = &self.files[j % n];
            if !v.contains(p) {
                v.push(p.clone());
            }
        }
        v
    }

    /// Largest bitmap edge the device accepts: what full size means here.
    fn full_edge(&mut self) -> u32 {
        self.r.max_bitmap(self.size.0, self.size.1)
    }

    /// Keeps the current image at full size (for zooming) with a screen-sized
    /// preview (for drawing), its neighbours at screen size only (for
    /// instant browsing), and asks the decoder for what is missing. Only one
    /// full-size bitmap is held at a time.
    fn request_decodes(&mut self) {
        // Collect finished work first, so none of it is requested again.
        self.collect_decoded();
        let wanted = self.wanted();
        self.cache.retain(|k, _| wanted.contains(k));
        let full = self.full_edge();
        let screen = self.screen_edge.clamp(256, full);
        for p in wanted.iter().skip(1) {
            self.reduce(p, screen);
        }
        if let Some(cur) = wanted.first() {
            self.add_preview(cur, screen);
        }
        let mut jobs: Vec<Job> = wanted
            .iter()
            .enumerate()
            .filter(|(_, p)| !self.cache.contains_key(*p))
            .map(|(i, p)| Job { path: p.clone(), max_edge: if i == 0 { full } else { screen } })
            .collect();
        // The current image is already on screen: prefetching comes first.
        if let Some(cur) = wanted.first() {
            if matches!(self.cache.get(cur), Some(Slot::Ready(l)) if !l.full) {
                jobs.push(Job { path: cur.clone(), max_edge: full });
            }
        }
        self.decoder.request(jobs);
    }

    /// Keeps only a screen-sized bitmap of a neighbour: its preview, or a
    /// copy of a bitmap larger than `edge` scaled on the GPU.
    fn reduce(&mut self, path: &Path, edge: u32) {
        let Some(Slot::Ready(l)) = self.cache.get_mut(path) else { return };
        let small = match l.preview.take() {
            Some(p) => p,
            None => {
                let (w, h) = crate::geom::fit_edge(l.bitmap.w, l.bitmap.h, edge);
                if (w, h) == (l.bitmap.w, l.bitmap.h) {
                    return;
                }
                let Ok(b) = self.r.shrink(&l.bitmap.b, (w, h), self.size) else { return };
                Bitmap { b, w, h }
            }
        };
        l.bitmap = small;
        l.full = false;
    }

    /// Gives a bitmap larger than `edge` a preview, scaled on the GPU.
    fn add_preview(&mut self, path: &Path, edge: u32) {
        let Some(Slot::Ready(l)) = self.cache.get_mut(path) else { return };
        let (w, h) = crate::geom::fit_edge(l.bitmap.w, l.bitmap.h, edge);
        if l.preview.is_some() || (w, h) == (l.bitmap.w, l.bitmap.h) {
            return;
        }
        if let Ok(b) = self.r.shrink(&l.bitmap.b, (w, h), self.size) {
            l.preview = Some(Bitmap { b, w, h });
        }
    }

    pub fn on_decoded(&mut self) {
        // Also asks for whatever the new results leave missing, such as
        // the full-size version of a preview that is now current.
        self.request_decodes();
    }

    fn collect_decoded(&mut self) {
        let wanted = self.wanted();
        let full_edge = self.full_edge();
        while let Ok((job, result)) = self.decoded.try_recv() {
            if !wanted.contains(&job.path) {
                continue;
            }
            let loaded = result.and_then(|d| {
                let full = job.max_edge >= full_edge || (d.w, d.h) == (d.meta.width, d.meta.height);
                let b = self.r.bitmap((d.w, d.h), self.size, &d.pixels).map_err(|e| wic::describe(&e))?;
                Ok(Loaded { bitmap: Bitmap { b, w: d.w, h: d.h }, preview: None, meta: d.meta, full })
            });
            match (self.cache.get_mut(&job.path), loaded) {
                // A preview never replaces a full-size bitmap.
                (Some(Slot::Ready(old)), Ok(new)) if old.full && !new.full => {}
                // When the full size cannot be had, the preview has to do.
                (Some(Slot::Ready(old)), Err(_)) => old.full = true,
                // The smaller bitmap that the full size replaces becomes its preview.
                (Some(Slot::Ready(old)), Ok(mut new)) => {
                    if old.bitmap.w < new.bitmap.w {
                        new.preview = Some(old.bitmap.clone());
                    }
                    *old = new;
                }
                (_, new) => {
                    self.cache.insert(job.path, new.map_or_else(Slot::Failed, Slot::Ready));
                }
            }
            self.dirty = true;
        }
    }

    pub fn device_lost(&mut self) {
        self.cache.clear();
        self.request_decodes();
    }

    // --------------------------------------------------------------------
    // Zoom and pan
    // --------------------------------------------------------------------

    fn start_anim(&mut self) {
        if self.animations {
            self.anim = Some((self.effective_view(), self.now));
        }
    }

    /// Zooms to `s` (DIPs per pixel), keeping the point under `at` fixed.
    fn zoom_to(&mut self, s: f32, at: (f32, f32), animate: bool) {
        let Some((w, h)) = self.image_size() else { return };
        let vp = self.viewport();
        let cur = self.effective_view();
        let fit = self.fit_scale(vp, w, h);
        let min = fit.min(1.0 / self.dpi_scale()) / 4.0;
        let s = s.clamp(min, MAX_ZOOM / self.dpi_scale());
        let (px, py) = (cur.cx + (at.0 - vp.cx()) / cur.s, cur.cy + (at.1 - vp.cy()) / cur.s);
        if animate {
            self.start_anim();
        } else {
            self.anim = None;
        }
        // Snap back to "fit" when zooming out across it.
        let snap = s <= fit && cur.s > fit;
        self.view = View { fit: snap, s, cx: px - (at.0 - vp.cx()) / s, cy: py - (at.1 - vp.cy()) / s };
        self.view = self.clamp_view(self.view);
        self.dirty = true;
    }

    fn zoom_by(&mut self, f: f32, at: Option<(f32, f32)>, animate: bool) {
        let vp = self.viewport();
        let s = self.effective_view().s * f;
        self.zoom_to(s, at.unwrap_or((vp.cx(), vp.cy())), animate);
    }

    fn zoom_fit(&mut self) {
        self.start_anim();
        self.view.fit = true;
        self.dirty = true;
    }

    fn zoom_toggle(&mut self, at: Option<(f32, f32)>) {
        let one = 1.0 / self.dpi_scale();
        let v = self.effective_view();
        if v.fit && v.s < one * 0.99 {
            let vp = self.viewport();
            self.zoom_to(one, at.unwrap_or((vp.cx(), vp.cy())), true);
        } else {
            self.zoom_fit();
        }
    }

    fn pannable(&self) -> bool {
        let Some((w, h)) = self.image_size() else { return false };
        let v = self.effective_view();
        let vp = self.viewport();
        w * v.s > vp.w + 0.5 || h * v.s > vp.h + 0.5
    }

    // --------------------------------------------------------------------
    // Editing
    // --------------------------------------------------------------------

    pub fn can_edit(&self) -> bool {
        self.loaded().is_some_and(|l| l.meta.frames <= 1) && !self.busy()
    }

    fn enter_edit(&mut self, orient: Orient) {
        if !self.can_edit() {
            if self.loaded().is_some_and(|l| l.meta.frames > 1) {
                self.show_toast("Animated images can't be edited");
            }
            return;
        }
        self.edit = Some(EditState { orient, crop: edit::FULL, aspect: Aspect::Free });
        self.popover = None;
        // Focus targets of the viewer are gone in the editor.
        self.focus = None;
        self.dirty = true;
    }

    fn edit_rotate(&mut self, cw: bool) {
        let Some(e) = self.edit.as_mut() else { return };
        let step = if cw { Orient::IDENTITY.rotate_cw() } else { Orient::IDENTITY.rotate_ccw() };
        e.orient = e.orient.then(step);
        e.crop = step.map_unit_rect(e.crop);
        e.aspect = e.aspect.turned();
        self.dirty = true;
    }

    fn edit_flip(&mut self, horizontal: bool) {
        let Some(e) = self.edit.as_mut() else { return };
        let step = if horizontal { Orient::IDENTITY.flip_h() } else { Orient::IDENTITY.flip_v() };
        e.orient = e.orient.then(step);
        e.crop = step.map_unit_rect(e.crop);
        self.dirty = true;
    }

    fn set_aspect(&mut self, a: Aspect) {
        let Some((w, h)) = self.image_size() else { return };
        let Some(e) = self.edit.as_mut() else { return };
        e.aspect = a;
        if let Some(r) = a.ratio(w, h) {
            let px = edit::fit_aspect(r, w, h);
            e.crop = Rect::new(px.x / w, px.y / h, px.w / w, px.h / h);
        }
        self.popover = None;
        self.dirty = true;
    }

    fn edit_dirty(&self) -> bool {
        self.edit.as_ref().is_some_and(|e| e.orient != Orient::IDENTITY || !edit::is_full(e.crop))
    }

    fn save_edit(&mut self, replace: bool) {
        let (Some(l), Some(e), Some(src)) = (self.loaded(), self.edit.as_ref(), self.current()) else { return };
        if !self.edit_dirty() {
            self.edit = None;
            self.dirty = true;
            return;
        }
        let orient = l.meta.orient.then(e.orient);
        let (ow, oh) = orient.apply_size(l.meta.width, l.meta.height);
        let crop = edit::crop_pixels(e.crop, ow, oh);
        let (container, ext): (_, OsString) = if l.meta.can_encode {
            (l.meta.container, src.extension().unwrap_or_default().into())
        } else if [GUID_ContainerFormatHeif, GUID_ContainerFormatAdng].contains(&l.meta.container) {
            (GUID_ContainerFormatJpeg, "jpg".into())
        } else {
            (GUID_ContainerFormatPng, "png".into())
        };
        if replace && container != l.meta.container {
            return;
        }
        let src = src.to_path_buf();
        let tmp = match sys::temp_beside(&src) {
            Ok(t) => t,
            Err(e) => {
                self.show_toast(&format!("Could not save: {e}"));
                return;
            }
        };
        let job = wic::SaveJob { src: src.clone(), dst: tmp.clone(), container, orient, crop };
        self.show_sticky("Saving…");
        self.start_work("Saving an image", move |tx, cancel| {
            let _com = sys::ComGuard::mta();
            let result = Wic::new()
                .map_err(|e| wic::describe(&e))
                .and_then(|w| wic::save(&w.0, &job, cancel))
                .and_then(|()| if replace { sys::replace_with(&src, &tmp).map(|()| src.clone()) } else { sys::publish_copy(&tmp, &src, &ext) });
            if result.is_err() {
                let _ = std::fs::remove_file(&tmp);
            }
            tx.send(TaskMsg::Saved { result, replaced: replace });
        });
    }

    fn crop_drag_to(&mut self, x: f32, y: f32) {
        let Some((w, h)) = self.image_size() else { return };
        let (Some(d), Some(e)) = (self.crop_drag.as_ref(), self.edit.as_mut()) else { return };
        let (dx, dy) = ((x - d.mouse.0) / d.scale, (y - d.mouse.1) / d.scale);
        let ratio = e.aspect.ratio(w, h);
        let r = edit::drag_crop(d.handle, d.start, dx, dy, w, h, ratio);
        e.crop = Rect::new(r.x / w, r.y / h, r.w / w, r.h / h);
        self.dirty = true;
    }

    // --------------------------------------------------------------------
    // Marks
    // --------------------------------------------------------------------

    /// Folder and file name of the current image (marks are keyed by both).
    fn current_key(&self) -> Option<(PathBuf, OsString)> {
        let p = self.current()?;
        Some((p.parent()?.to_path_buf(), p.file_name()?.to_os_string()))
    }

    pub fn current_marks(&self) -> u16 {
        self.current_key().map_or(0, |(d, n)| self.settings.marks(&d, &n))
    }

    pub fn mark_label(&self, slot: usize) -> Option<String> {
        let d = self.settings.mark_dirs[slot].as_deref()?;
        Some(d.file_name().unwrap_or(d.as_os_str()).to_string_lossy().into_owned())
    }

    fn toggle_mark(&mut self, slot: u8) -> Vec<Effect> {
        let s = usize::from(slot);
        if s >= SLOTS || self.current().is_none() {
            return Vec::new();
        }
        if self.settings.mark_dirs[s].is_none() {
            return vec![Effect::PickMarkDir { slot, toggle: true }];
        }
        let Some((dir, name)) = self.current_key() else { return Vec::new() };
        let bits = self.update_settings(|st| st.toggle(&dir, &name, s));
        let label = self.mark_label(s).unwrap_or_default();
        self.show_toast(&if bits & (1 << s) != 0 { format!("Marked for “{label}”") } else { format!("Removed from “{label}”") });
        Vec::new()
    }

    pub fn mark_dir_picked(&mut self, slot: u8, dir: PathBuf, toggle: bool) -> Vec<Effect> {
        self.update_settings(|st| st.mark_dirs[usize::from(slot)] = Some(dir));
        if toggle { self.toggle_mark(slot) } else { Vec::new() }
    }

    fn clear_current_marks(&mut self) {
        let Some((dir, name)) = self.current_key() else { return };
        let had = self.update_settings(|st| {
            let had = st.marks(&dir, &name) != 0;
            st.set_marks(&dir, &name, 0);
            had
        });
        if had {
            self.show_toast("Marks removed");
        }
    }

    pub fn send_summary(&self) -> Vec<(usize, PathBuf, usize)> {
        let counts = self.settings.counts();
        (0..SLOTS).filter(|&s| counts[s] > 0).filter_map(|s| Some((s, self.settings.mark_dirs[s].clone()?, counts[s]))).collect()
    }

    fn send(&mut self, mode: SendMode) {
        self.dialog = None;
        self.reload_settings();
        let dirs = self.settings.mark_dirs.clone();
        let items: Vec<_> = self.settings.marked();
        if items.is_empty() || self.busy() {
            return;
        }
        self.show_sticky(if mode == SendMode::Copy { "Copying…" } else { "Moving…" });
        self.start_work(if mode == SendMode::Copy { "Copying files" } else { "Moving files" }, move |tx, cancel| {
            let verb = if mode == SendMode::Copy { "Copying" } else { "Moving" };
            let (mut done, mut missing, mut failed) = (Vec::new(), Vec::new(), Vec::new());
            let total = items.len();
            'files: for (i, (folder, name, bits)) in items.into_iter().enumerate() {
                if cancel.load(Ordering::Relaxed) {
                    break;
                }
                tx.send(TaskMsg::Progress(format!("{verb} {} of {total}…", i + 1)));
                let src = folder.join(&name);
                if !src.is_file() {
                    missing.push((folder, name));
                    continue;
                }
                let targets: Vec<(usize, &PathBuf)> =
                    (0..SLOTS).filter(|s| bits & (1 << s) != 0).filter_map(|s| Some((s, dirs[s].as_ref()?))).collect();
                if targets.is_empty() {
                    continue;
                }
                // Only a folder that resolves can be recognised as the file's own.
                let home = std::fs::canonicalize(&folder).ok();
                let mut sent = 0u16;
                let mut ok = true;
                for (k, &(slot, dest)) in targets.iter().enumerate() {
                    let last = k + 1 == targets.len();
                    let r = std::fs::create_dir_all(dest).map_err(|e| e.to_string()).and_then(|()| {
                        if home.is_some() && std::fs::canonicalize(dest).ok() == home {
                            Ok(()) // Already in that folder.
                        } else if mode == SendMode::Move && last && ok {
                            sys::move_into(&src, dest, cancel).map(drop)
                        } else {
                            sys::copy_into(&src, dest, cancel).map(drop)
                        }
                    });
                    match r {
                        Ok(()) => sent |= 1 << slot,
                        // Stopped on request: Windows removed the partial copy.
                        Err(_) if cancel.load(Ordering::Relaxed) => {
                            if sent != 0 {
                                done.push((folder, name, sent));
                            }
                            break 'files;
                        }
                        Err(e) => {
                            ok = false;
                            failed.push(format!("{}: {e}", name.to_string_lossy()));
                        }
                    }
                }
                if sent != 0 {
                    done.push((folder, name, sent));
                }
            }
            tx.send(TaskMsg::Sent { mode, done, missing, failed });
        });
    }

    /// Looks for a newer release in the background.
    pub fn check_for_updates(&mut self, manual: bool) {
        if matches!(self.update, UpdateState::Checking { .. } | UpdateState::Ready(_)) {
            if manual && matches!(self.update, UpdateState::Ready(_)) {
                self.show_toast("An update is ready — restart to use it");
            }
            return;
        }
        self.update = UpdateState::Checking { manual };
        if manual {
            self.show_sticky("Checking for updates…");
        }
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
        self.update_settings(|st| st.last_update_check = now);
        self.spawn(move |tx| {
            let outcome = crate::update::run();
            tx.send(TaskMsg::Update { outcome, manual });
        });
    }

    /// Daily automatic check, if enabled.
    pub fn maybe_check_for_updates(&mut self) {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
        if self.settings.auto_update && now.saturating_sub(self.settings.last_update_check) >= UPDATE_INTERVAL {
            self.check_for_updates(false);
        }
    }

    /// Runs `f` on a worker thread; its messages arrive in [`on_task`](Self::on_task).
    fn spawn(&self, f: impl FnOnce(Poster<TaskMsg>) + Send + 'static) {
        let tx = self.tasks.clone();
        std::thread::spawn(move || f(tx));
    }

    /// Like [`spawn`](Self::spawn), for work that writes files: `f` stops
    /// early once its flag is set. Its last message ends it.
    fn start_work(&mut self, what: &'static str, f: impl FnOnce(Poster<TaskMsg>, &AtomicBool) + Send + 'static) {
        let cancel = Arc::new(AtomicBool::new(false));
        let (tx, flag) = (self.tasks.clone(), Arc::clone(&cancel));
        let thread = std::thread::spawn(move || f(tx, &flag));
        self.work = Some(Work { what, cancel, thread });
        self.close_requests = 0;
    }

    /// Ends the current work; true when it was stopped on request.
    fn finish_work(&mut self) -> bool {
        self.work.take().is_some_and(|w| w.cancel.load(Ordering::Relaxed))
    }

    /// Saving or sending: files are being written.
    pub fn busy(&self) -> bool {
        self.work.is_some()
    }

    /// Why Windows should wait before ending the session, if it should.
    pub fn work_reason(&self) -> Option<&'static str> {
        self.work.as_ref().map(|w| w.what)
    }

    /// Windows is ending the session: stop the work, give it a moment to
    /// remove half-written files, and record what it finished (such as the
    /// marks of files already sent).
    pub fn end_session(&mut self) {
        if let Some(w) = &self.work {
            w.cancel.store(true, Ordering::Relaxed);
            let start = Instant::now();
            while !w.thread.is_finished() && start.elapsed() < Duration::from_secs(3) {
                std::thread::sleep(Duration::from_millis(20));
            }
        }
        let _ = self.on_task();
    }

    pub fn on_task(&mut self) -> Vec<Effect> {
        let mut fx = Vec::new();
        while let Ok(m) = self.task_rx.try_recv() {
            match m {
                TaskMsg::Progress(s) => {
                    let s = match self.close_requests {
                        0 => s,
                        1 => format!("{s} Then closing."),
                        _ => "Stopping…".into(),
                    };
                    self.show_sticky(&s);
                }
                TaskMsg::Update { outcome, manual } => {
                    if manual {
                        self.toast = None;
                    }
                    self.update = UpdateState::Idle;
                    match outcome {
                        crate::update::Outcome::Ready(v) => self.update = UpdateState::Ready(v),
                        crate::update::Outcome::UpToDate if manual => self.show_toast("Windmolen is up to date"),
                        crate::update::Outcome::Failed(e) if manual => self.show_toast(&format!("Could not update: {e}")),
                        _ => {}
                    }
                }
                TaskMsg::Sent { mode, done, missing, failed } => {
                    let stopped = self.finish_work();
                    self.update_settings(|st| {
                        for (dir, name, bits) in &done {
                            let left = st.marks(dir, name) & !bits;
                            st.set_marks(dir, name, left);
                        }
                        for (dir, name) in &missing {
                            st.set_marks(dir, name, 0);
                        }
                    });
                    let verb = if mode == SendMode::Copy { "Copied" } else { "Moved" };
                    let n = done.len();
                    let files = if n == 1 { "file" } else { "files" };
                    let mut msg = format!("{verb} {n} {files}");
                    if let Some(e) = failed.first() {
                        msg += &format!(", {} failed — {e}", failed.len());
                        if !stopped {
                            // Stay open so the failure is seen.
                            self.close_requests = 0;
                        }
                    }
                    if stopped {
                        msg += " before stopping";
                    }
                    self.show_toast(&msg);
                    if mode == SendMode::Move {
                        fx.extend(self.rescan());
                    }
                }
                TaskMsg::Saved { result, replaced } => {
                    let stopped = self.finish_work();
                    match result {
                        Ok(p) => {
                            self.edit = None;
                            if replaced {
                                self.cache.remove(&p);
                                self.show_toast("Original replaced");
                            } else {
                                let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                                self.show_toast(&format!("Saved as “{name}”"));
                                if self.selection {
                                    self.files.insert((self.index + 1).min(self.files.len()), p.clone());
                                }
                            }
                            fx.extend(self.rescan());
                        }
                        Err(_) if stopped => self.show_toast("Saving stopped"),
                        Err(e) => {
                            // Stay open: the edit is still there to save again.
                            self.close_requests = 0;
                            self.show_toast(&format!("Could not save: {e}"));
                        }
                    }
                }
            }
        }
        if self.close_requests > 0 && !self.busy() {
            fx.push(Effect::Close);
        }
        self.dirty = true;
        fx
    }

    /// Whether the window may close now. While files are being written, a
    /// first request waits for them, a second stops the work (half-written
    /// files are removed) and closes when it has, and a third, for when
    /// even stopping hangs (a lost network drive), closes at once.
    pub fn may_close(&mut self) -> bool {
        let Some(cancel) = self.work.as_ref().map(|w| Arc::clone(&w.cancel)) else { return true };
        self.close_requests = self.close_requests.saturating_add(1);
        match self.close_requests {
            1 => self.show_sticky("Finishing… Close again to stop."),
            2 => {
                cancel.store(true, Ordering::Relaxed);
                self.show_sticky("Stopping… Close again to quit at once.");
            }
            _ => return true,
        }
        false
    }

    /// Applies `f` to the settings on disk and saves them, under a session
    /// lock, so several windows never undo each other's changes.
    pub fn update_settings<R>(&mut self, f: impl FnOnce(&mut Settings) -> R) -> R {
        let Some(path) = self.settings_path.clone() else { return f(&mut self.settings) };
        let _lock = sys::NamedMutex::lock(SETTINGS_LOCK, 2000);
        match Settings::load(&path) {
            Ok(s) => self.settings = s,
            Err(e) => {
                // Never save over a file that could not be read: this change
                // lasts for the session only.
                self.settings_unreadable(&e);
                self.dirty = true;
                return f(&mut self.settings);
            }
        }
        let r = f(&mut self.settings);
        if let Err(e) = self.settings.save(&path) {
            self.show_toast(&format!("Could not save settings: {e}"));
        }
        self.dirty = true;
        r
    }

    /// Picks up changes made by other windows.
    pub fn reload_settings(&mut self) {
        if let Some(Ok(s)) = self.settings_path.as_deref().map(Settings::load) {
            self.settings = s;
            self.dirty = true;
        }
    }

    // --------------------------------------------------------------------
    // Feedback
    // --------------------------------------------------------------------

    pub fn show_toast(&mut self, text: &str) {
        self.toast = Some(Toast { text: text.into(), born: Instant::now(), sticky: false });
        self.dirty = true;
    }

    fn show_sticky(&mut self, text: &str) {
        let born = self.toast.as_ref().filter(|t| t.sticky).map_or_else(Instant::now, |t| t.born);
        self.toast = Some(Toast { text: text.into(), born, sticky: true });
        self.dirty = true;
    }

    pub fn toast_alpha(&self) -> f32 {
        let Some(t) = &self.toast else { return 0.0 };
        let age = (self.now - t.born).as_secs_f32();
        if !self.animations {
            return 1.0;
        }
        let fade_in = (age / 0.15).min(1.0);
        let fade_out = if t.sticky { 1.0 } else { ((TOAST_TIME.as_secs_f32() - age) / TOAST_FADE.as_secs_f32()).clamp(0.0, 1.0) };
        fade_in.min(fade_out)
    }

    /// Whether the hovered widget has waited long enough for a tooltip.
    pub fn tooltip_ready(&self) -> bool {
        self.pressed.is_none() && self.hover.is_some() && self.now - self.hover_since >= TOOLTIP_DELAY
    }

    pub fn dragging_crop(&self) -> bool {
        self.crop_drag.is_some()
    }

    pub fn loading_phase(&self) -> f32 {
        self.loading_since.map_or(0.0, |t| (self.now - t).as_secs_f32())
    }

    // --------------------------------------------------------------------
    // Time
    // --------------------------------------------------------------------

    fn osd_target(&self) -> f32 {
        let in_osd = matches!(self.hover, Some(Action::Prev | Action::Next | Action::ZoomIn | Action::ZoomOut | Action::ZoomToggle));
        let recent = self.now - self.last_motion < OSD_IDLE;
        if self.edit.is_none() && self.loaded().is_some() && self.dialog.is_none() && (in_osd || (recent && self.mouse_inside)) { 1.0 } else { 0.0 }
    }

    /// Advances animations; called on every timer tick and before painting.
    pub fn tick(&mut self) {
        let prev = self.now;
        let toast = self.toast_alpha();
        self.now = Instant::now();
        let dt = (self.now - prev).as_secs_f32();

        if let Some((_, start)) = self.anim {
            if self.now - start >= ANIM {
                self.anim = None;
            }
            self.dirty = true;
        }
        let target = self.osd_target();
        if self.osd != target {
            self.osd = if self.animations {
                let step = dt / 0.15;
                if target > self.osd { (self.osd + step).min(target) } else { (self.osd - step).max(target) }
            } else {
                target
            };
            self.dirty = true;
        }
        if self.toast.as_ref().is_some_and(|t| !t.sticky && self.now - t.born >= TOAST_TIME) {
            self.toast = None;
        }
        // A toast needs painting only while it fades, or when it vanishes.
        if self.toast_alpha() != toast {
            self.dirty = true;
        }
        if self.show_spinner() {
            self.dirty = true;
        }
    }

    /// When the host should call [`tick`](Self::tick) next.
    pub fn next_wake(&self) -> Option<Duration> {
        let frame = Duration::from_millis(16);
        let animating = self.anim.is_some() || self.osd != self.osd_target() || (self.toast.is_some() && self.toast_alpha() < 1.0) || self.loading();
        if animating {
            return Some(frame);
        }
        let mut wake: Vec<Duration> = Vec::new();
        let left = |deadline: Instant| deadline.saturating_duration_since(self.now);
        // Sticky toasts stay until replaced; others fade out (or vanish).
        if let Some(t) = self.toast.as_ref().filter(|t| !t.sticky) {
            let fade = if self.animations { TOAST_FADE } else { Duration::ZERO };
            wake.push(left(t.born + TOAST_TIME - fade));
        }
        // Past the deadline, only input can hide the OSD (it is hovered).
        let idle = self.last_motion + OSD_IDLE;
        if self.osd > 0.0 && self.now < idle {
            wake.push(left(idle));
        }
        if self.hover.is_some() && self.now - self.hover_since < TOOLTIP_DELAY {
            wake.push(left(self.hover_since + TOOLTIP_DELAY));
        }
        wake.into_iter().min().map(|d| d.max(Duration::from_millis(1)))
    }

    // --------------------------------------------------------------------
    // Rendering and hit testing
    // --------------------------------------------------------------------

    pub fn widgets(&self) -> Vec<Widget> {
        view::build(self).widgets
    }

    pub fn paint(&mut self) {
        self.tick();
        let ui = view::build(self);
        let (image, preview) = self.loaded().map_or((None, None), |l| (Some(l.bitmap.b.clone()), l.preview.as_ref().map(|p| p.b.clone())));
        let clear = self.theme.view_bg;
        match self.r.render(self.size, clear, &ui.cmds, image.as_ref(), preview.as_ref()) {
            Ok(Frame::Ok) => {}
            Ok(Frame::DeviceLost) => self.device_lost(),
            Err(_) => self.device_lost(),
        }
    }

    pub fn resize(&mut self, w: u32, h: u32, maximised: bool, screen_edge: u32) {
        self.size = (w.max(1), h.max(1));
        self.maximised = maximised;
        self.screen_edge = screen_edge;
        self.r.resize(self.size.0, self.size.1);
        self.view = self.clamp_view(self.view);
        self.dirty = true;
    }

    pub fn set_dpi(&mut self, dpi: f32) {
        self.dpi = dpi;
        self.r.set_dpi(dpi);
        self.dirty = true;
    }

    /// What the non-client hit test should report at a client point (DIPs).
    pub fn nc_hit(&self, x: f32, y: f32) -> NcHit {
        if self.fullscreen && self.edit.is_none() {
            return NcHit::Client;
        }
        match self.widgets().iter().rev().find(|w| w.rect.contains(x, y)) {
            Some(w) if w.action == Action::Maximize => NcHit::MaxButton,
            Some(_) => NcHit::Client,
            None if y < HEADER_H && !self.fullscreen => NcHit::Caption,
            None => NcHit::Client,
        }
    }

    // --------------------------------------------------------------------
    // Input
    // --------------------------------------------------------------------

    fn set_hover(&mut self, a: Option<Action>) {
        if a != self.hover {
            self.hover = a;
            self.hover_since = Instant::now();
            self.dirty = true;
        }
    }

    pub fn mouse_move(&mut self, x: f32, y: f32) {
        let moved = (x - self.mouse.0).abs() + (y - self.mouse.1).abs() > 0.5;
        self.mouse = (x, y);
        self.mouse_inside = true;
        if moved {
            self.last_motion = Instant::now();
            if self.fullscreen {
                self.dirty = true;
            }
        }
        if let Some(p) = &self.pan {
            let v = self.effective_view();
            self.anim = None;
            self.view = self.clamp_view(View { fit: false, s: v.s, cx: p.c.0 - (x - p.mouse.0) / v.s, cy: p.c.1 - (y - p.mouse.1) / v.s });
            self.dirty = true;
            return;
        }
        if self.crop_drag.is_some() {
            self.crop_drag_to(x, y);
            return;
        }
        let hit = self.widgets().into_iter().rev().find(|w| w.rect.contains(x, y)).map(|w| w.action);
        self.set_hover(hit);
        if moved && self.osd < 1.0 && self.osd_target() > 0.0 {
            self.dirty = true;
        }
    }

    pub fn mouse_leave(&mut self) {
        self.mouse_inside = false;
        self.set_hover(None);
        self.dirty = true;
    }

    /// Hover over the maximise button arrives through non-client messages.
    pub fn nc_hover(&mut self, max_button: bool) {
        self.set_hover(max_button.then_some(Action::Maximize));
    }

    pub fn mouse_down(&mut self, x: f32, y: f32) -> Vec<Effect> {
        self.mouse = (x, y);
        self.focus = None;
        let hit = self.widgets().into_iter().rev().find(|w| w.rect.contains(x, y));
        match hit {
            Some(w) if w.enabled => {
                if let Action::Crop(handle) = w.action {
                    if let (Some(e), Some((w, h)), Some((_, s))) = (&self.edit, self.image_size(), self.edit_geometry()) {
                        let start = Rect::new(e.crop.x * w, e.crop.y * h, e.crop.w * w, e.crop.h * h);
                        self.crop_drag = Some(CropDrag { handle, start, mouse: (x, y), scale: s });
                    }
                    return Vec::new();
                }
                self.pressed = Some(w.action);
            }
            Some(_) => {}
            None => {
                if self.edit.is_none() && self.viewport().contains(x, y) && self.pannable() {
                    let v = self.effective_view();
                    self.pan = Some(PanDrag { mouse: (x, y), c: (v.cx, v.cy) });
                }
            }
        }
        self.dirty = true;
        Vec::new()
    }

    pub fn mouse_up(&mut self, x: f32, y: f32) -> Vec<Effect> {
        self.pan = None;
        self.crop_drag = None;
        let pressed = self.pressed.take();
        self.dirty = true;
        let hit = self.widgets().into_iter().rev().find(|w| w.rect.contains(x, y));
        match (pressed, hit) {
            (Some(p), Some(w)) if w.action == p && w.enabled => self.activate(p),
            _ => Vec::new(),
        }
    }

    pub fn cancel_drag(&mut self) {
        self.pan = None;
        self.crop_drag = None;
        self.pressed = None;
        self.dirty = true;
    }

    pub fn double_click(&mut self, x: f32, y: f32) -> Vec<Effect> {
        let on_widget = self.widgets().iter().rev().any(|w| w.rect.contains(x, y));
        if on_widget {
            return self.mouse_down(x, y);
        }
        if self.edit.is_none() && self.dialog.is_none() && self.popover.is_none() && self.viewport().contains(x, y) {
            self.zoom_toggle(Some((x, y)));
        }
        Vec::new()
    }

    pub fn wheel(&mut self, delta: f32, x: f32, y: f32, horizontal: bool) {
        if self.dialog.is_some() {
            self.scroll = (self.scroll - delta / 2.0).clamp(0.0, view::dialog_max_scroll(self));
            self.dirty = true;
            return;
        }
        if self.popover.is_some() || self.edit.is_some() {
            return;
        }
        if self.sidebar_open() && x >= self.width() - SIDEBAR_W && y >= self.content_top() {
            self.props_scroll = (self.props_scroll - delta / 2.0).clamp(0.0, view::sidebar_max_scroll(self));
            self.dirty = true;
            return;
        }
        if horizontal {
            if self.pannable() {
                let v = self.effective_view();
                self.view = self.clamp_view(View { fit: false, cx: v.cx + delta / v.s / 2.0, ..v });
                self.dirty = true;
            }
            return;
        }
        let notch = delta / 120.0;
        self.zoom_by(ZOOM_STEP.powf(notch), Some((x, y)), notch.abs() >= 1.0);
    }

    pub fn cursor(&self) -> Cursor {
        if self.pan.is_some() {
            return Cursor::Grab;
        }
        let handle = self.crop_drag.as_ref().map(|d| d.handle).or(match self.hover {
            Some(Action::Crop(h)) => Some(h),
            _ => None,
        });
        if let Some(h) = handle {
            return match h {
                Handle::Move => Cursor::Move,
                Handle::N | Handle::S => Cursor::ResizeNs,
                Handle::E | Handle::W => Cursor::ResizeEw,
                Handle::NW | Handle::SE => Cursor::ResizeNwse,
                Handle::NE | Handle::SW => Cursor::ResizeNesw,
            };
        }
        if self.hover.is_none() && self.edit.is_none() && self.viewport().contains(self.mouse.0, self.mouse.1) && self.pannable() {
            return Cursor::Hand;
        }
        Cursor::Arrow
    }

    /// Runs a UI action.
    pub fn activate(&mut self, a: Action) -> Vec<Effect> {
        self.dirty = true;
        let closes_popover = !matches!(a, Action::MainMenu | Action::MarksMenu | Action::AspectMenu | Action::SaveMenu);
        if closes_popover {
            self.popover = None;
        }
        match a {
            Action::None => {}
            Action::Dismiss => {
                self.dialog = None;
                self.popover = None;
                self.focus = None;
            }
            Action::Open => return vec![Effect::OpenDialog],
            Action::Prev => return self.step(-1),
            Action::Next => return self.step(1),
            Action::ZoomIn => self.zoom_by(ZOOM_STEP, None, true),
            Action::ZoomOut => self.zoom_by(1.0 / ZOOM_STEP, None, true),
            Action::ZoomFit => self.zoom_fit(),
            Action::ZoomToggle => self.zoom_toggle(None),
            Action::Properties => {
                self.update_settings(|st| st.properties = !st.properties);
                self.view = self.clamp_view(self.view);
            }
            Action::Fullscreen => return vec![Effect::Fullscreen(!self.fullscreen)],
            Action::MainMenu => self.toggle_popover(Popover::Main),
            Action::MarksMenu => self.toggle_popover(Popover::Marks),
            Action::AspectMenu => self.toggle_popover(Popover::Aspect),
            Action::SaveMenu => self.toggle_popover(Popover::Save),
            Action::Edit => self.enter_edit(Orient::IDENTITY),
            Action::ToggleMark(s) => return self.toggle_mark(s),
            Action::SendMarked => self.open_dialog(Dialog::Send),
            Action::ManageMarks => self.open_dialog(Dialog::ManageMarks),
            Action::Shortcuts => self.open_dialog(Dialog::Shortcuts),
            Action::About => self.open_dialog(Dialog::About),
            Action::Minimize => return vec![Effect::Minimize],
            Action::Maximize => return vec![Effect::ToggleMaximize],
            Action::Close => return vec![Effect::Close],
            Action::EditCancel => self.edit = None,
            Action::EditSave => self.save_edit(false),
            Action::ReplaceOriginal => self.open_dialog(Dialog::Replace),
            Action::ConfirmReplace => {
                self.dialog = None;
                self.save_edit(true);
            }
            Action::RotateCcw => self.edit_rotate(false),
            Action::RotateCw => self.edit_rotate(true),
            Action::FlipH => self.edit_flip(true),
            Action::FlipV => self.edit_flip(false),
            Action::SetAspect(x) => self.set_aspect(x),
            Action::EditReset => {
                if let Some(e) = self.edit.as_mut() {
                    *e = EditState { orient: Orient::IDENTITY, crop: edit::FULL, aspect: Aspect::Free };
                }
            }
            Action::Crop(_) => {}
            Action::PickMarkDir(slot) => return vec![Effect::PickMarkDir { slot, toggle: false }],
            Action::ClearMarkDir(slot) => {
                self.update_settings(|st| st.mark_dirs[usize::from(slot)] = None);
            }
            Action::Install => self.open_dialog(Dialog::Install),
            Action::ConfirmInstall => {
                self.dialog = None;
                return vec![Effect::Install];
            }
            Action::DefaultApps => return vec![Effect::DefaultApps],
            Action::CheckUpdates => self.check_for_updates(true),
            Action::ToggleAutoUpdate => {
                let on = self.update_settings(|st| {
                    st.auto_update = !st.auto_update;
                    st.auto_update
                });
                self.show_toast(if on { "Automatic updates on" } else { "Automatic updates off" });
            }
            Action::Restart => return vec![Effect::Restart(self.current().map(Path::to_path_buf))],
            Action::OpenProfile => return vec![Effect::OpenUrl(crate::release::profile_url())],
            Action::OpenProject => return vec![Effect::OpenUrl(crate::release::project_url())],
            Action::SendCopy => self.send(SendMode::Copy),
            Action::SendMove => self.send(SendMode::Move),
        }
        Vec::new()
    }

    fn toggle_popover(&mut self, p: Popover) {
        self.popover = if self.popover == Some(p) { None } else { Some(p) };
        self.focus = None;
    }

    fn open_dialog(&mut self, d: Dialog) {
        self.dialog = Some(d);
        self.popover = None;
        self.focus = None;
        self.scroll = 0.0;
    }

    /// Keyboard focus traversal over the current widgets.
    fn move_focus(&mut self, forward: bool) {
        let list: Vec<Action> = self.widgets().into_iter().filter(|w| w.enabled && w.focusable).map(|w| w.action).collect();
        if list.is_empty() {
            return;
        }
        let i = self.focus.and_then(|f| list.iter().position(|&a| a == f));
        let n = list.len();
        let j = match (i, forward) {
            (None, true) => 0,
            (None, false) => n - 1,
            (Some(i), true) => (i + 1) % n,
            (Some(i), false) => (i + n - 1) % n,
        };
        self.focus = Some(list[j]);
        self.dirty = true;
    }

    /// Non-character keys. Returns `None` when the key is not handled.
    pub fn key(&mut self, k: Key, ctrl: bool, shift: bool) -> Option<Vec<Effect>> {
        let modal = self.dialog.is_some() || self.popover.is_some();
        let fx = match k {
            Key::Escape => {
                if self.dialog.is_some() || self.popover.is_some() {
                    self.dialog = None;
                    self.popover = None;
                    self.focus = None;
                } else if self.edit.is_some() {
                    self.edit = None;
                } else if self.fullscreen {
                    return Some(vec![Effect::Fullscreen(false)]);
                }
                self.dirty = true;
                Vec::new()
            }
            Key::Tab => {
                self.move_focus(!shift);
                Vec::new()
            }
            Key::Up | Key::Down if modal => {
                self.move_focus(k == Key::Down);
                Vec::new()
            }
            Key::Enter | Key::Space if self.focus.is_some() => {
                let a = self.focus.take().expect("checked");
                self.activate(a)
            }
            // The default response of an alert, as in AdwAlertDialog.
            Key::Enter if self.dialog == Some(Dialog::Send) && !self.send_summary().is_empty() => self.activate(Action::SendCopy),
            Key::Enter if self.dialog == Some(Dialog::Install) => self.activate(Action::ConfirmInstall),
            Key::Enter if ctrl && self.edit.is_some() => {
                self.save_edit(false);
                Vec::new()
            }
            _ if modal => return None,
            Key::Left | Key::PageUp if self.edit.is_none() => self.step(-1),
            Key::Right | Key::PageDown if self.edit.is_none() => self.step(1),
            Key::Home if self.edit.is_none() => self.go(0),
            Key::End if self.edit.is_none() => self.go(self.files.len().saturating_sub(1)),
            Key::Up | Key::Down | Key::Left | Key::Right if self.pannable() => {
                let v = self.effective_view();
                let d = 48.0 / v.s;
                let (dx, dy) = match k {
                    Key::Up => (0.0, -d),
                    Key::Down => (0.0, d),
                    Key::Left => (-d, 0.0),
                    _ => (d, 0.0),
                };
                self.view = self.clamp_view(View { fit: false, cx: v.cx + dx, cy: v.cy + dy, ..v });
                self.dirty = true;
                Vec::new()
            }
            Key::F9 => self.activate(Action::Properties),
            Key::F10 => self.activate(Action::MainMenu),
            Key::F11 => self.activate(Action::Fullscreen),
            Key::Letter(c) if ctrl => match (c, shift) {
                ('o', _) if self.edit.is_none() => self.activate(Action::Open),
                ('s', _) if self.edit.is_some() => self.activate(Action::EditSave),
                ('r', false) => self.rotate_shortcut(true),
                ('r', true) => self.rotate_shortcut(false),
                ('m', _) => self.activate(Action::SendMarked),
                ('q', _) | ('w', _) => vec![Effect::Close],
                ('0', _) => self.activate(Action::ZoomFit),
                ('1', _) => {
                    let vp = self.viewport();
                    self.zoom_to(1.0 / self.dpi_scale(), (vp.cx(), vp.cy()), true);
                    Vec::new()
                }
                ('+', _) | ('=', _) => self.activate(Action::ZoomIn),
                ('-', _) => self.activate(Action::ZoomOut),
                ('?', _) | ('/', true) => self.activate(Action::Shortcuts),
                _ => return None,
            },
            _ => return None,
        };
        Some(fx)
    }

    fn rotate_shortcut(&mut self, cw: bool) -> Vec<Effect> {
        if self.edit.is_none() {
            self.enter_edit(Orient::IDENTITY);
        }
        self.edit_rotate(cw);
        Vec::new()
    }

    /// Printable characters (layout-aware), without Ctrl.
    pub fn char(&mut self, c: char) -> Vec<Effect> {
        if self.dialog.is_some() || self.popover.is_some() {
            return Vec::new();
        }
        let in_edit = self.edit.is_some();
        match c {
            '1'..='9' if !in_edit => self.activate(Action::ToggleMark(c as u8 - b'1')),
            'a' | 'A' if !in_edit => self.step(-1),
            'd' | 'D' if !in_edit => self.step(1),
            '+' | '=' if !in_edit => self.activate(Action::ZoomIn),
            '-' | '_' if !in_edit => self.activate(Action::ZoomOut),
            '0' if !in_edit => {
                let vp = self.viewport();
                self.zoom_to(1.0 / self.dpi_scale(), (vp.cx(), vp.cy()), true);
                Vec::new()
            }
            'f' | 'F' if !in_edit => self.activate(Action::ZoomFit),
            'r' => self.rotate_shortcut(true),
            'R' => self.rotate_shortcut(false),
            'e' | 'E' | 'c' | 'C' if !in_edit => self.activate(Action::Edit),
            'i' | 'I' if !in_edit => self.activate(Action::Properties),
            'm' | 'M' if !in_edit => self.activate(Action::MarksMenu),
            'u' | 'U' if !in_edit => {
                self.clear_current_marks();
                Vec::new()
            }
            'h' | 'H' if in_edit => self.activate(Action::FlipH),
            'v' | 'V' if in_edit => self.activate(Action::FlipV),
            '?' => self.activate(Action::Shortcuts),
            _ => Vec::new(),
        }
    }

    pub fn mouse_button(&mut self, back: bool) -> Vec<Effect> {
        if self.edit.is_none() && self.dialog.is_none() { self.step(if back { -1 } else { 1 }) } else { Vec::new() }
    }

    pub fn set_fullscreen(&mut self, on: bool) {
        self.fullscreen = on;
        self.popover = None;
        self.dirty = true;
    }

    pub fn window_active(&mut self, active: bool) {
        self.active = active;
        if active {
            self.reload_settings();
        } else {
            self.cancel_drag();
        }
        self.dirty = true;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NcHit {
    Client,
    Caption,
    MaxButton,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cursor {
    Arrow,
    Hand,
    Grab,
    Move,
    ResizeNs,
    ResizeEw,
    ResizeNwse,
    ResizeNesw,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Escape,
    Tab,
    Enter,
    Space,
    Left,
    Right,
    Up,
    Down,
    PageUp,
    PageDown,
    Home,
    End,
    F9,
    F10,
    F11,
    /// A letter, digit or symbol pressed together with Ctrl.
    Letter(char),
}
