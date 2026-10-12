// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 professional-cynic

//! Embeds the application manifest (asInvoker, per-monitor DPI v2, long
//! paths) and, when a resource compiler is available, the Explorer icon.
//!
//! No build dependencies: `res/windmolen.ico` (every size drawn by
//! `res/make-icon.py`) is embedded as is, and the manifest goes through the
//! MSVC linker directly. Without a
//! resource compiler the program still works; it sets its DPI awareness at
//! run time and never requests elevation either way.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=res/windmolen.manifest");
    println!("cargo:rerun-if-changed=res/windmolen.ico");
    println!("cargo:rerun-if-env-changed=WINDRES");
    println!("cargo:rerun-if-env-changed=RC");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let msvc = env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc");
    let res = Path::new(&env::var("CARGO_MANIFEST_DIR").unwrap()).join("res");
    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    let manifest = res.join("windmolen.manifest");

    let ico_path = res.join("windmolen.ico");

    let quote = |p: &Path| p.display().to_string().replace('\\', "\\\\");
    let mut rc = format!("1 ICON \"{}\"\n", quote(&ico_path));
    if msvc {
        // Resolve the program's own DLL imports (d2d1, dwrite, winhttp, …)
        // from System32 only, never from the folder it runs in: a DLL planted
        // next to a portable copy in Downloads would otherwise run with it.
        // LOAD_LIBRARY_SEARCH_SYSTEM32; Windows 10 1607 and later.
        println!("cargo:rustc-link-arg-bins=/DEPENDENTLOADFLAG:0x800");
        // link.exe embeds the manifest itself.
        println!("cargo:rustc-link-arg-bins=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg-bins=/MANIFESTINPUT:{}", manifest.display());
        println!("cargo:rustc-link-arg-bins=/MANIFESTUAC:NO");
    } else {
        rc += &format!("1 24 \"{}\"\n", quote(&manifest));
    }
    let rc_path = out.join("windmolen.rc");
    fs::write(&rc_path, rc).unwrap();

    if msvc {
        let tool = env::var("RC").unwrap_or_else(|_| "rc.exe".into());
        let res_path = out.join("windmolen.res");
        let ok = Command::new(&tool).arg("/nologo").arg("/fo").arg(&res_path).arg(&rc_path).status().is_ok_and(|s| s.success());
        if ok {
            println!("cargo:rustc-link-arg-bins={}", res_path.display());
        } else {
            println!("cargo:warning=rc.exe not found: building without the Explorer icon (run from a Developer prompt or set RC)");
        }
    } else {
        let target = env::var("TARGET").unwrap();
        let arch = target.split('-').next().unwrap_or("x86_64");
        let candidates = env::var("WINDRES").map(|w| vec![w]).unwrap_or_else(|_| vec![format!("{arch}-w64-mingw32-windres"), "windres".into()]);
        let obj = out.join("windmolen-res.o");
        let ok = candidates
            .iter()
            .any(|tool| Command::new(tool).arg("-i").arg(&rc_path).arg("-O").arg("coff").arg("-o").arg(&obj).status().is_ok_and(|s| s.success()));
        if ok {
            println!("cargo:rustc-link-arg-bins={}", obj.display());
        } else {
            println!("cargo:warning=windres not found: building without manifest and Explorer icon (set WINDRES)");
        }
    }
}
