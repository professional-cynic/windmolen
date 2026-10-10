//! The Win32 window: client-side decorations (keeping Windows' resize
//! borders, snap layouts and system menu), DPI handling and input routing.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Dwm::{DWMWA_CLOAK, DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWINDOWATTRIBUTE, DwmSetWindowAttribute};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Shutdown::{ShutdownBlockReasonCreate, ShutdownBlockReasonDestroy};
use windows::Win32::UI::Controls::WM_MOUSELEAVE;
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, GetDpiForWindow, GetSystemMetricsForDpi, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::Shell::{DragAcceptFiles, DragFinish, DragQueryFileW, HDROP};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{BOOL, HSTRING, PCWSTR, Result, w};

use crate::app::{App, Cursor, Effect, Key, NcHit, WM_TASK};
use crate::settings::Placement;
use crate::wic::WM_DECODED;
use crate::{install, instance, sys, update};

const CLASS: PCWSTR = w!("Windmolen.Window");
const TIMER: usize = 1;

struct Host {
    app: RefCell<Option<App>>,
    icons: Cell<[Option<HICON>; 2]>,
    /// Placement to return to when leaving fullscreen.
    restore: Cell<Option<WINDOWPLACEMENT>>,
    tracking: Cell<bool>,
    /// A shutdown block reason is registered.
    blocking: Cell<bool>,
}

fn loword(v: usize) -> u16 {
    v as u16
}

fn hiword(v: usize) -> u16 {
    (v >> 16) as u16
}

fn point(lp: LPARAM) -> (i32, i32) {
    (i32::from(lp.0 as u16 as i16), i32::from((lp.0 >> 16) as u16 as i16))
}

fn set_attr<T>(hwnd: HWND, attr: DWMWINDOWATTRIBUTE, v: &T) {
    // SAFETY: `v` points to a value of the size the attribute expects.
    unsafe {
        let _ = DwmSetWindowAttribute(hwnd, attr, (v as *const T).cast(), std::mem::size_of::<T>() as u32);
    }
}

fn frame_thickness(hwnd: HWND) -> i32 {
    // SAFETY: plain metric queries.
    unsafe {
        let dpi = GetDpiForWindow(hwnd);
        GetSystemMetricsForDpi(SM_CYFRAME, dpi) + GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi)
    }
}

/// Longer side of the window's monitor in pixels.
fn monitor_edge(hwnd: HWND) -> u32 {
    let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
    // SAFETY: out-pointer to a local whose size is set.
    if !unsafe { GetMonitorInfoW(MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST), &mut mi) }.as_bool() {
        return 3840;
    }
    let r = mi.rcMonitor;
    (r.right - r.left).max(r.bottom - r.top).max(1) as u32
}

/// Where a new window opens: centred on the screen under the pointer, at
/// the remembered size (in DIPs), never larger than that screen's work area.
fn initial_rect(placement: Option<Placement>) -> (i32, i32, i32, i32) {
    let (w, h) = placement.map_or((1000, 700), |p| (p.w, p.h));
    // SAFETY: out-pointers to locals whose sizes are set.
    unsafe {
        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        let mon = MonitorFromPoint(pt, MONITOR_DEFAULTTOPRIMARY);
        let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        let (mut dx, mut dy) = (96, 96);
        if !GetMonitorInfoW(mon, &mut mi).as_bool() || GetDpiForMonitor(mon, MDT_EFFECTIVE_DPI, &mut dx, &mut dy).is_err() {
            return (CW_USEDEFAULT, CW_USEDEFAULT, w, h);
        }
        let s = dx as f32 / 96.0;
        let r = mi.rcWork;
        let (ww, wh) = (r.right - r.left, r.bottom - r.top);
        let (w, h) = (((w as f32 * s) as i32).min(ww), ((h as f32 * s) as i32).min(wh));
        (r.left + (ww - w) / 2, r.top + (wh - h) / 2, w, h)
    }
}

pub fn run(args: Vec<PathBuf>, only: bool) -> Result<()> {
    // SAFETY: standard window creation; the host outlives the window
    // because it is reclaimed only after the message loop has ended.
    unsafe {
        let instance: HINSTANCE = GetModuleHandleW(None)?.into();
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_DBLCLKS,
            lpfnWndProc: Some(wndproc),
            hInstance: instance,
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            lpszClassName: CLASS,
            ..Default::default()
        };
        if RegisterClassExW(&wc) == 0 {
            return Err(windows::core::Error::from_thread());
        }

        let host = Box::into_raw(Box::new(Host {
            app: RefCell::new(None),
            icons: Cell::new([None, None]),
            restore: Cell::new(None),
            tracking: Cell::new(false),
            blocking: Cell::new(false),
        }));
        // Created in place, so it starts at the right DPI for its screen.
        let placement = sys::settings_path().and_then(|p| crate::settings::Settings::load(&p).ok()).and_then(|s| s.placement);
        let (x, y, w, h) = initial_rect(placement);
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            CLASS,
            w!("Windmolen"),
            WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN,
            x,
            y,
            w,
            h,
            None,
            None,
            Some(instance),
            Some(host as *const _),
        )?;

        let app = App::new(hwnd, GetDpiForWindow(hwnd) as f32)?;
        let dark = BOOL::from(app.theme.dark);
        (*host).app.replace(Some(app));

        (*host).set_icons(hwnd);
        set_attr(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, &dark);
        // Stay invisible until the first frame is drawn: no white flash.
        set_attr(hwnd, DWMWA_CLOAK, &BOOL(1));
        DragAcceptFiles(hwnd, true);

        let _ = ShowWindow(hwnd, if placement.is_some_and(|p| p.maximised) { SW_SHOWMAXIMIZED } else { SW_SHOWNORMAL });
        // Apply the custom frame.
        let _ = SetWindowPos(hwnd, None, 0, 0, 0, 0, SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
        let host_ref = &*host;
        host_ref.sync_size(hwnd);
        let fx = host_ref.with(|a| a.open_launch(args, only)).unwrap_or_default();
        host_ref.run_effects(hwnd, fx);
        let _ = UpdateWindow(hwnd);
        set_attr(hwnd, DWMWA_CLOAK, &BOOL(0));

        // Housekeeping the first frame need not wait for.
        instance::set_target(hwnd);
        if let Some(exts) = host_ref.with(|a| a.exts.clone()) {
            install::refresh(&exts);
        }
        update::clean_up();
        host_ref.with(App::maybe_check_for_updates);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        let host = Box::from_raw(host);
        for icon in host.icons.get().into_iter().flatten() {
            let _ = DestroyIcon(icon);
        }
        Ok(())
    }
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    // SAFETY: GWLP_USERDATA holds the `Host` pointer set at WM_NCCREATE,
    // which stays valid until after the message loop ends.
    unsafe {
        if msg == WM_NCCREATE {
            let cs = &*(lp.0 as *const CREATESTRUCTW);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, cs.lpCreateParams as isize);
            return DefWindowProcW(hwnd, msg, wp, lp);
        }
        let host = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const Host;
        if host.is_null() {
            return DefWindowProcW(hwnd, msg, wp, lp);
        }
        if msg == WM_NCDESTROY {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
        }
        (*host).handle(hwnd, msg, wp, lp)
    }
}

impl Host {
    /// Runs `f` with the app unless it is already borrowed (re-entrancy
    /// from a modal loop) or not yet created.
    fn with<R>(&self, f: impl FnOnce(&mut App) -> R) -> Option<R> {
        let mut guard = self.app.try_borrow_mut().ok()?;
        guard.as_mut().map(f)
    }

    /// Window icons for the taskbar and Alt+Tab, at the current DPI.
    fn set_icons(&self, hwnd: HWND) {
        let png = crate::render::APP_ICON_PNG;
        // SAFETY: PNG data in icon-resource form; handles are freed later.
        unsafe {
            let dpi = GetDpiForWindow(hwnd);
            let old = self.icons.get();
            let mut new = [None, None];
            for (i, (kind, metric)) in [(ICON_BIG, SM_CXICON), (ICON_SMALL, SM_CXSMICON)].into_iter().enumerate() {
                let size = GetSystemMetricsForDpi(metric, dpi);
                if let Ok(icon) = CreateIconFromResourceEx(png, true, 0x0003_0000, size, size, LR_DEFAULTCOLOR) {
                    SendMessageW(hwnd, WM_SETICON, Some(WPARAM(kind as usize)), Some(LPARAM(icon.0 as isize)));
                    new[i] = Some(icon);
                }
            }
            self.icons.set(new);
            for icon in old.into_iter().flatten() {
                let _ = DestroyIcon(icon);
            }
        }
    }

    fn dips(&self, hwnd: HWND, x: i32, y: i32) -> (f32, f32) {
        // SAFETY: plain query on our window.
        let s = unsafe { GetDpiForWindow(hwnd) } as f32 / 96.0;
        (x as f32 / s, y as f32 / s)
    }

    fn sync_size(&self, hwnd: HWND) {
        let mut rc = RECT::default();
        // SAFETY: out-pointer to a local.
        unsafe {
            let _ = GetClientRect(hwnd, &mut rc);
            let max = IsZoomed(hwnd).as_bool();
            let edge = monitor_edge(hwnd);
            self.with(|a| a.resize(rc.right as u32, rc.bottom as u32, max, edge));
        }
    }

    /// Repaints if needed and arms the animation/deadline timer.
    fn after(&self, hwnd: HWND) {
        let Some((dirty, wake, work)) = self.with(|a| (a.take_dirty(), a.next_wake(), a.work_reason())) else { return };
        // SAFETY: invalidation, timers and shutdown reasons of our own window.
        unsafe {
            // While files are being written, Windows lists the app (and why)
            // instead of ending it mid-write.
            if work.is_some() != self.blocking.get() {
                self.blocking.set(work.is_some());
                match work {
                    Some(what) => {
                        let _ = ShutdownBlockReasonCreate(hwnd, &HSTRING::from(what));
                    }
                    None => {
                        let _ = ShutdownBlockReasonDestroy(hwnd);
                    }
                }
            }
            if dirty {
                let _ = InvalidateRect(Some(hwnd), None, false);
            }
            match wake {
                Some(d) => {
                    SetTimer(Some(hwnd), TIMER, d.as_millis().clamp(1, 60_000) as u32, None);
                }
                None => {
                    let _ = KillTimer(Some(hwnd), TIMER);
                }
            }
        }
    }

    fn run_effects(&self, hwnd: HWND, fx: Vec<Effect>) {
        for e in fx {
            // SAFETY: window management calls on our own window, made while
            // the app is not borrowed so re-entrant messages are handled.
            unsafe {
                match e {
                    Effect::OpenDialog { only } => {
                        let Some((exts, start)) = self.with(|a| {
                            let start = a.folder.clone().or_else(|| a.current().and_then(Path::parent).map(Path::to_path_buf));
                            (a.exts.clone(), start)
                        }) else {
                            continue;
                        };
                        let title = if only { "Open Image Only" } else { "Open" };
                        match sys::open_file_dialog(hwnd, title, &exts, start.as_deref()) {
                            Ok(files) => {
                                let more = self.with(|a| a.open_files(files, only)).unwrap_or_default();
                                self.run_effects(hwnd, more);
                            }
                            Err(err) => {
                                self.with(|a| a.show_toast(&format!("Could not open the file dialog: {}", err.message())));
                            }
                        }
                    }
                    Effect::PickMarkDir { slot, toggle } => {
                        let Some(start) = self.with(|a| a.settings.mark_dirs[usize::from(slot)].clone().or_else(|| a.folder.clone())) else {
                            continue;
                        };
                        let title = format!("Choose Folder for Mark {}", slot + 1);
                        if let Ok(Some(dir)) = sys::pick_folder_dialog(hwnd, &title, start.as_deref()) {
                            let more = self.with(|a| a.mark_dir_picked(slot, dir, toggle)).unwrap_or_default();
                            self.run_effects(hwnd, more);
                        }
                    }
                    Effect::Fullscreen(on) => self.set_fullscreen(hwnd, on),
                    Effect::Minimize => {
                        let _ = ShowWindow(hwnd, SW_MINIMIZE);
                    }
                    Effect::ToggleMaximize => {
                        let _ = ShowWindow(hwnd, if IsZoomed(hwnd).as_bool() { SW_RESTORE } else { SW_MAXIMIZE });
                    }
                    Effect::Close => {
                        let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
                    }
                    Effect::Title(t) => {
                        let _ = SetWindowTextW(hwnd, &HSTRING::from(t));
                    }
                    Effect::OpenUrl(url) => {
                        sys::shell_open(&url);
                    }
                    // The installer closes this window itself, once it can.
                    Effect::Restart(file) => {
                        if let Err(e) = update::install(file.as_deref()) {
                            self.with(|a| a.show_toast(&format!("Could not update: {e}")));
                        }
                    }
                }
            }
        }
        self.after(hwnd);
    }

    /// Borderless fullscreen on the window's monitor (Raymond Chen's recipe).
    fn set_fullscreen(&self, hwnd: HWND, on: bool) {
        // SAFETY: style and placement changes on our own window.
        unsafe {
            let style = GetWindowLongPtrW(hwnd, GWL_STYLE);
            let overlapped = WS_OVERLAPPEDWINDOW.0 as isize;
            if on {
                let mut wp = WINDOWPLACEMENT { length: std::mem::size_of::<WINDOWPLACEMENT>() as u32, ..Default::default() };
                let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
                if GetWindowPlacement(hwnd, &mut wp).is_err()
                    || !GetMonitorInfoW(MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST), &mut mi).as_bool()
                {
                    return;
                }
                self.restore.set(Some(wp));
                self.with(|a| a.set_fullscreen(true));
                SetWindowLongPtrW(hwnd, GWL_STYLE, style & !overlapped);
                let m = mi.rcMonitor;
                let _ = SetWindowPos(hwnd, Some(HWND_TOP), m.left, m.top, m.right - m.left, m.bottom - m.top, SWP_NOOWNERZORDER | SWP_FRAMECHANGED);
            } else {
                self.with(|a| a.set_fullscreen(false));
                SetWindowLongPtrW(hwnd, GWL_STYLE, style | overlapped);
                if let Some(wp) = self.restore.take() {
                    let _ = SetWindowPlacement(hwnd, &wp);
                }
                let flags = SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOOWNERZORDER | SWP_FRAMECHANGED;
                let _ = SetWindowPos(hwnd, None, 0, 0, 0, 0, flags);
            }
            self.sync_size(hwnd);
        }
    }

    fn save_placement(&self, hwnd: HWND) {
        let mut wp = WINDOWPLACEMENT { length: std::mem::size_of::<WINDOWPLACEMENT>() as u32, ..Default::default() };
        // SAFETY: out-pointer to a local.
        let wp = match self.restore.get() {
            Some(r) => r,
            None if unsafe { GetWindowPlacement(hwnd, &mut wp) }.is_ok() => wp,
            None => return,
        };
        let r = wp.rcNormalPosition;
        let maximised = wp.showCmd == SW_SHOWMAXIMIZED.0 as u32 || (wp.flags.0 & WPF_RESTORETOMAXIMIZED.0) != 0;
        // SAFETY: plain query on our window.
        let s = unsafe { GetDpiForWindow(hwnd) }.max(1) as f32 / 96.0;
        let (w, h) = (((r.right - r.left) as f32 / s).round() as i32, ((r.bottom - r.top) as f32 / s).round() as i32);
        self.with(|a| a.update_settings(|st| st.placement = Some(Placement { w, h, maximised })));
    }

    fn handle(&self, hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        // SAFETY: standard message handling on our own window; pointers in
        // `lp` are those documented for each message.
        unsafe {
            let def = || DefWindowProcW(hwnd, msg, wp, lp);
            let ctrl = GetKeyState(VK_CONTROL.0.into()) < 0;
            let shift = GetKeyState(VK_SHIFT.0.into()) < 0;
            let mut fx = Vec::new();
            match msg {
                WM_NCCALCSIZE if wp.0 != 0 && (GetWindowLongPtrW(hwnd, GWL_STYLE) & WS_CAPTION.0 as isize) != 0 => {
                    // Keep the system's side and bottom borders; drop the caption.
                    let params = &mut *(lp.0 as *mut NCCALCSIZE_PARAMS);
                    let top = params.rgrc[0].top;
                    let r = def();
                    if r.0 != 0 {
                        return r;
                    }
                    params.rgrc[0].top = top + if IsZoomed(hwnd).as_bool() { frame_thickness(hwnd) } else { 0 };
                    return LRESULT(0);
                }
                WM_NCHITTEST => {
                    let r = def();
                    if r.0 != HTCLIENT as isize {
                        return r;
                    }
                    let (sx, sy) = point(lp);
                    let mut pt = POINT { x: sx, y: sy };
                    let _ = ScreenToClient(hwnd, &mut pt);
                    let caption = (GetWindowLongPtrW(hwnd, GWL_STYLE) & WS_CAPTION.0 as isize) != 0;
                    if caption && !IsZoomed(hwnd).as_bool() && pt.y < frame_thickness(hwnd) {
                        return LRESULT(HTTOP as isize);
                    }
                    let (x, y) = self.dips(hwnd, pt.x, pt.y);
                    let hit = self.with(|a| a.nc_hit(x, y)).unwrap_or(NcHit::Client);
                    return LRESULT(match hit {
                        NcHit::Client => HTCLIENT,
                        NcHit::Caption => HTCAPTION,
                        NcHit::MaxButton => HTMAXBUTTON,
                    } as isize);
                }
                // The maximise button reports HTMAXBUTTON for Windows 11 snap
                // layouts, so its clicks arrive as non-client messages.
                WM_NCMOUSEMOVE => {
                    let on = wp.0 == HTMAXBUTTON as usize;
                    self.with(|a| a.nc_hover(on));
                    let mut t = TRACKMOUSEEVENT {
                        cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                        dwFlags: TME_LEAVE | TME_NONCLIENT,
                        hwndTrack: hwnd,
                        dwHoverTime: 0,
                    };
                    let _ = TrackMouseEvent(&mut t);
                    self.after(hwnd);
                    if on {
                        return LRESULT(0);
                    }
                    return def();
                }
                WM_NCMOUSELEAVE => {
                    self.with(|a| a.nc_hover(false));
                }
                WM_NCLBUTTONDOWN | WM_NCLBUTTONDBLCLK if wp.0 == HTMAXBUTTON as usize => {
                    self.with(|a| {
                        a.pressed = Some(crate::ui::Action::Maximize);
                        a.invalidate();
                    });
                }
                WM_NCLBUTTONUP if wp.0 == HTMAXBUTTON as usize => {
                    let was = self.with(|a| {
                        a.invalidate();
                        a.pressed.take() == Some(crate::ui::Action::Maximize)
                    });
                    if was == Some(true) {
                        fx.push(Effect::ToggleMaximize);
                    }
                }
                WM_MOUSEMOVE => {
                    if !self.tracking.get() {
                        let mut t = TRACKMOUSEEVENT {
                            cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                            dwFlags: TME_LEAVE,
                            hwndTrack: hwnd,
                            dwHoverTime: 0,
                        };
                        self.tracking.set(TrackMouseEvent(&mut t).is_ok());
                    }
                    let (px, py) = point(lp);
                    let (x, y) = self.dips(hwnd, px, py);
                    self.with(|a| a.mouse_move(x, y));
                }
                WM_MOUSELEAVE => {
                    self.tracking.set(false);
                    self.with(App::mouse_leave);
                }
                WM_LBUTTONDOWN => {
                    SetCapture(hwnd);
                    let (px, py) = point(lp);
                    let (x, y) = self.dips(hwnd, px, py);
                    fx = self.with(|a| a.mouse_down(x, y)).unwrap_or_default();
                }
                WM_LBUTTONUP => {
                    let (px, py) = point(lp);
                    let (x, y) = self.dips(hwnd, px, py);
                    fx = self.with(|a| a.mouse_up(x, y)).unwrap_or_default();
                    let _ = ReleaseCapture();
                }
                WM_LBUTTONDBLCLK => {
                    let (px, py) = point(lp);
                    let (x, y) = self.dips(hwnd, px, py);
                    fx = self.with(|a| a.double_click(x, y)).unwrap_or_default();
                }
                WM_CAPTURECHANGED => {
                    self.with(App::cancel_drag);
                }
                WM_XBUTTONUP => {
                    let back = hiword(wp.0) == XBUTTON1;
                    fx = self.with(|a| a.mouse_button(back)).unwrap_or_default();
                    self.run_effects(hwnd, fx);
                    return LRESULT(1);
                }
                WM_MOUSEWHEEL | WM_MOUSEHWHEEL => {
                    let delta = f32::from(hiword(wp.0) as i16);
                    let (sx, sy) = point(lp);
                    let mut pt = POINT { x: sx, y: sy };
                    let _ = ScreenToClient(hwnd, &mut pt);
                    let (x, y) = self.dips(hwnd, pt.x, pt.y);
                    let horizontal = msg == WM_MOUSEHWHEEL;
                    self.with(|a| a.wheel(delta, x, y, horizontal));
                }
                WM_SETCURSOR if loword(lp.0 as usize) == HTCLIENT as u16 => {
                    let c = self.with(|a| a.cursor()).unwrap_or(Cursor::Arrow);
                    let id = match c {
                        Cursor::Arrow => IDC_ARROW,
                        Cursor::Hand => IDC_HAND,
                        Cursor::Grab | Cursor::Move => IDC_SIZEALL,
                        Cursor::ResizeNs => IDC_SIZENS,
                        Cursor::ResizeEw => IDC_SIZEWE,
                        Cursor::ResizeNwse => IDC_SIZENWSE,
                        Cursor::ResizeNesw => IDC_SIZENESW,
                    };
                    if let Ok(h) = LoadCursorW(None, id) {
                        SetCursor(Some(h));
                    }
                    return LRESULT(1);
                }
                WM_KEYDOWN | WM_SYSKEYDOWN => {
                    let vk = VIRTUAL_KEY(loword(wp.0));
                    let key = match vk {
                        VK_ESCAPE => Some(Key::Escape),
                        VK_TAB => Some(Key::Tab),
                        VK_RETURN => Some(Key::Enter),
                        VK_SPACE => Some(Key::Space),
                        VK_LEFT => Some(Key::Left),
                        VK_RIGHT => Some(Key::Right),
                        VK_UP => Some(Key::Up),
                        VK_DOWN => Some(Key::Down),
                        VK_PRIOR => Some(Key::PageUp),
                        VK_NEXT => Some(Key::PageDown),
                        VK_HOME => Some(Key::Home),
                        VK_END => Some(Key::End),
                        VK_F9 => Some(Key::F9),
                        VK_F10 => Some(Key::F10),
                        VK_F11 => Some(Key::F11),
                        _ if ctrl => match vk.0 {
                            v @ 0x41..=0x5A => Some(Key::Letter(char::from(v as u8).to_ascii_lowercase())),
                            v @ 0x30..=0x39 => Some(Key::Letter(char::from(v as u8))),
                            _ if vk == VK_OEM_PLUS || vk == VK_ADD => Some(Key::Letter('+')),
                            _ if vk == VK_OEM_MINUS || vk == VK_SUBTRACT => Some(Key::Letter('-')),
                            _ if vk == VK_OEM_2 => Some(Key::Letter(if shift { '?' } else { '/' })),
                            _ if vk == VK_OEM_COMMA => Some(Key::Letter(',')),
                            _ => None,
                        },
                        _ => None,
                    };
                    // Alt combinations other than F10 belong to the system.
                    let alt = msg == WM_SYSKEYDOWN;
                    let handled = match key {
                        Some(k) if !alt || k == Key::F10 => self.with(|a| a.key(k, ctrl, shift)).flatten(),
                        _ => None,
                    };
                    match handled {
                        Some(f) => fx = f,
                        None => return def(),
                    }
                }
                WM_CHAR => {
                    let c = char::from_u32(wp.0 as u32).filter(|c| !c.is_control() && *c != ' ');
                    match c {
                        Some(c) if !ctrl => fx = self.with(|a| a.char(c)).unwrap_or_default(),
                        _ => return def(),
                    }
                }
                WM_TIMER => {
                    self.with(App::tick);
                }
                WM_PAINT => {
                    let mut ps = PAINTSTRUCT::default();
                    BeginPaint(hwnd, &mut ps);
                    self.with(App::paint);
                    let _ = EndPaint(hwnd, &ps);
                }
                WM_ERASEBKGND => return LRESULT(1),
                WM_SIZE => {
                    if wp.0 as u32 != SIZE_MINIMIZED {
                        let (w, h) = (u32::from(loword(lp.0 as usize)), u32::from(hiword(lp.0 as usize)));
                        let max = wp.0 as u32 == SIZE_MAXIMIZED;
                        let edge = monitor_edge(hwnd);
                        self.with(|a| a.resize(w, h, max, edge));
                    }
                }
                WM_DPICHANGED => {
                    let dpi = f32::from(loword(wp.0));
                    self.with(|a| a.set_dpi(dpi));
                    let r = &*(lp.0 as *const RECT);
                    let _ = SetWindowPos(hwnd, None, r.left, r.top, r.right - r.left, r.bottom - r.top, SWP_NOZORDER | SWP_NOACTIVATE);
                    self.sync_size(hwnd);
                    self.set_icons(hwnd);
                }
                WM_DISPLAYCHANGE => self.sync_size(hwnd),
                WM_GETMINMAXINFO => {
                    let mmi = &mut *(lp.0 as *mut MINMAXINFO);
                    let s = GetDpiForWindow(hwnd) as f32 / 96.0;
                    mmi.ptMinTrackSize = POINT { x: (360.0 * s) as i32, y: (294.0 * s) as i32 };
                    return LRESULT(0);
                }
                WM_ACTIVATE => {
                    let active = u32::from(loword(wp.0)) != WA_INACTIVE;
                    self.with(|a| a.window_active(active));
                }
                WM_SETTINGCHANGE | WM_DWMCOLORIZATIONCOLORCHANGED | WM_THEMECHANGED => {
                    let dark = self.with(|a| {
                        a.reload_theme();
                        a.theme.dark
                    });
                    if let Some(d) = dark {
                        set_attr(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, &BOOL::from(d));
                    }
                }
                WM_DROPFILES => {
                    let drop = HDROP(wp.0 as *mut _);
                    let mut buf = vec![0u16; 32_768];
                    let n = DragQueryFileW(drop, 0, Some(&mut buf)) as usize;
                    DragFinish(drop);
                    if n > 0 {
                        let path = PathBuf::from(<std::ffi::OsString as std::os::windows::ffi::OsStringExt>::from_wide(&buf[..n.min(buf.len())]));
                        let _ = SetForegroundWindow(hwnd);
                        fx = self.with(|a| a.open_path(&path)).unwrap_or_default();
                    }
                }
                WM_DECODED => {
                    self.with(App::on_decoded);
                }
                WM_TASK => {
                    fx = self.with(App::on_task).unwrap_or_default();
                }
                instance::WM_GATHERED => {
                    let more = instance::take();
                    if !more.is_empty() {
                        fx = self.with(|a| a.add_launch(more)).unwrap_or_default();
                    }
                }
                WM_CLOSE => {
                    if self.with(App::may_close) == Some(false) {
                        self.after(hwnd);
                        return LRESULT(0);
                    }
                    self.save_placement(hwnd);
                    return def();
                }
                // Asked before Windows ends the session: not while files are
                // being written (the user may still end it regardless).
                WM_QUERYENDSESSION => {
                    let busy = self.with(|a| a.busy()).unwrap_or(false);
                    return LRESULT(isize::from(!busy));
                }
                WM_ENDSESSION => {
                    if wp.0 != 0 {
                        self.with(App::end_session);
                        self.save_placement(hwnd);
                        // Closed for an installer (Restart Manager), not for
                        // a shutdown: quit now rather than wait to be killed.
                        if lp.0 as u32 & ENDSESSION_CLOSEAPP != 0 {
                            let _ = DestroyWindow(hwnd);
                        }
                    }
                    return LRESULT(0);
                }
                WM_DESTROY => {
                    let _ = KillTimer(Some(hwnd), TIMER);
                    PostQuitMessage(0);
                    return LRESULT(0);
                }
                _ => return def(),
            }
            self.run_effects(hwnd, fx);
            LRESULT(0)
        }
    }
}
