// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 professional-cynic

//! Windmolen — a lean photo viewer for Windows in the libadwaita style.
#![cfg_attr(windows, windows_subsystem = "windows")]
#![cfg_attr(not(windows), allow(dead_code))]

mod edit;
mod geom;
mod icons;
mod meta;
mod release;
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
mod update;
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
    let args: Vec<std::path::PathBuf> = std::env::args_os().skip(1).map(Into::into).collect();
    let message = |text: String, flags| {
        // SAFETY: a plain message box without owner.
        unsafe { MessageBoxW(None, &HSTRING::from(text), w!("Windmolen"), flags) }
    };
    // Run hidden by the installer and uninstaller.
    match args.first().and_then(|a| a.to_str()) {
        Some("--register") => {
            let exts: Vec<String> = wic::Wic::new().map(|w| w.extensions().into_iter().collect()).unwrap_or_default();
            if let Err(e) = install::register(&exts) {
                message(format!("Windmolen could not register its image formats.\n\n{e}"), MB_OK | MB_ICONERROR);
                std::process::exit(1);
            }
            return;
        }
        Some("--unregister") => {
            install::unregister();
            return;
        }
        _ => {}
    }

    // Explorer's "Open in Windmolen" verb shows just the chosen files.
    let only = args.iter().any(|a| a.as_os_str() == "--only");
    // Absolute, because a gathering instance has its own working directory.
    let files: Vec<std::path::PathBuf> = args
        .into_iter()
        .filter(|a| !a.as_os_str().is_empty() && !a.as_os_str().as_encoded_bytes().starts_with(b"--"))
        .map(|a| std::path::absolute(&a).unwrap_or(a))
        .collect();
    if let instance::Launch::Forwarded = instance::start(&files) {
        return;
    }
    let result = window::run(files, only);
    if let Err(e) = result {
        message(format!("Windmolen could not start.\n\n{}", e.message()), MB_OK | MB_ICONERROR);
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("Windmolen runs on Windows only.");
}
