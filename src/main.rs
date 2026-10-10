//! Het Plantsoen — a lean photo viewer for Windows in the libadwaita style.
#![cfg_attr(windows, windows_subsystem = "windows")]
#![cfg_attr(not(windows), allow(dead_code))]

mod edit;
mod geom;
mod icons;
mod meta;
mod settings;
mod theme;
mod ui;

#[cfg(windows)]
mod app;
#[cfg(windows)]
mod render;
#[cfg(windows)]
mod sys;
#[cfg(windows)]
mod view;
#[cfg(windows)]
mod wic;
#[cfg(windows)]
mod window;

#[cfg(windows)]
fn main() {
    use windows::Win32::UI::HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext};
    use windows::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};
    use windows::core::{HSTRING, w};

    let _com = sys::ComGuard::sta();
    // The embedded manifest already does this; the call covers builds without it.
    // SAFETY: process-wide setting made before any window exists.
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    let arg = std::env::args_os().nth(1).map(std::path::PathBuf::from);
    if let Err(e) = window::run(arg) {
        let text = HSTRING::from(format!("Het Plantsoen could not start.\n\n{}", e.message()));
        // SAFETY: a plain message box without owner.
        unsafe {
            MessageBoxW(None, &text, w!("Het Plantsoen"), MB_OK | MB_ICONERROR);
        }
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("Het Plantsoen runs on Windows only.");
}
