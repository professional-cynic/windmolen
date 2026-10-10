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
mod install;
#[cfg(windows)]
mod instance;
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
    use windows::Win32::UI::WindowsAndMessaging::{
        IDYES, MB_ICONERROR, MB_ICONINFORMATION, MB_ICONQUESTION, MB_ICONWARNING, MB_OK, MB_YESNO, MessageBoxW,
    };
    use windows::core::{HSTRING, w};

    let _com = sys::ComGuard::sta();
    // The embedded manifest already does this; the call covers builds without it.
    // SAFETY: process-wide setting made before any window exists.
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    let args: Vec<std::path::PathBuf> = std::env::args_os().skip(1).map(Into::into).collect();
    let message = |text: String, flags| {
        // SAFETY: a plain message box without owner.
        unsafe { MessageBoxW(None, &HSTRING::from(text), w!("Het Plantsoen"), flags) }
    };
    match args.first().and_then(|a| a.to_str()) {
        Some("--install") => {
            let exts: Vec<String> = wic::Wic::new().map(|w| w.extensions().into_iter().collect()).unwrap_or_default();
            match install::install(&exts) {
                Ok(exe) => {
                    message(format!("Installed to {}.\n\nChoose Het Plantsoen as your default image viewer in the Settings page that opens next.", exe.display()), MB_OK | MB_ICONINFORMATION);
                    install::open_default_apps();
                }
                Err(e) => {
                    message(format!("Installation failed.\n\n{e}"), MB_OK | MB_ICONERROR);
                }
            }
            return;
        }
        Some("--uninstall") => {
            if message("Remove Het Plantsoen from this account?\n\nYour marks and settings are kept.".into(), MB_YESNO | MB_ICONQUESTION) == IDYES {
                if let Err(e) = install::uninstall() {
                    message(format!("Het Plantsoen was unregistered, but not completely removed.\n\n{e}"), MB_OK | MB_ICONWARNING);
                }
            }
            return;
        }
        _ => {}
    }

    let files: Vec<std::path::PathBuf> = args.into_iter().filter(|a| !a.as_os_str().to_string_lossy().starts_with("--")).collect();
    if let instance::Launch::Forwarded = instance::start(&files) {
        return;
    }
    if let Err(e) = window::run(files) {
        message(format!("Het Plantsoen could not start.\n\n{}", e.message()), MB_OK | MB_ICONERROR);
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("Het Plantsoen runs on Windows only.");
}
