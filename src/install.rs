//! File-type registration, for the copy that the installer
//! (`installer/windmolen.iss`) put in place: everything in HKCU, no
//! administrator rights.
//!
//! Registers the app the way Windows expects of a default-app candidate:
//! a ProgID per extension, `OpenWithProgids` entries, an `Applications`
//! entry, `Capabilities` under `RegisteredApplications` (which lists it in
//! Settings › Default apps) and a right-click verb that opens just the
//! chosen files, not their folder. The extensions are those WIC can decode,
//! so codecs added later are registered at the next start ([`refresh`]).
//!
//! Windows does not let an app make itself the default: the installer
//! explains how to choose it in Settings.

use std::path::Path;

use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Registry::*;
use windows::Win32::UI::Shell::{SHCNE_ASSOCCHANGED, SHCNF_IDLIST, SHChangeNotify};
use windows::core::{HSTRING, PCWSTR};

const NAME: &str = "Windmolen";
const EXE: &str = "windmolen.exe";
const APP_KEY: &str = r"Software\Windmolen";
const CLASSES: &str = r"Software\Classes";
/// Written by the self-installer of earlier test builds.
const LEGACY_UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\Windmolen";
const VERB: &str = "Windmolen";

/// An open HKCU key, closed on drop.
struct Key(HKEY);

impl Key {
    fn create(path: &str) -> Result<Self, String> {
        let mut h = HKEY::default();
        // SAFETY: out-pointer to a local; the handle is owned by `Key`.
        let r = unsafe {
            RegCreateKeyExW(HKEY_CURRENT_USER, &HSTRING::from(path), None, PCWSTR::null(), REG_OPTION_NON_VOLATILE, KEY_WRITE, None, &mut h, None)
        };
        if r == ERROR_SUCCESS { Ok(Self(h)) } else { Err(format!("Could not write the registry key {path}")) }
    }

    fn set(&self, name: &str, value: &str) -> Result<(), String> {
        let data: Vec<u8> = value.encode_utf16().chain(Some(0)).flat_map(u16::to_le_bytes).collect();
        let name = if name.is_empty() { HSTRING::new() } else { HSTRING::from(name) };
        // SAFETY: `data` is a NUL-terminated UTF-16 string as REG_SZ requires.
        let r = unsafe { RegSetValueExW(self.0, &name, None, REG_SZ, Some(&data)) };
        if r == ERROR_SUCCESS { Ok(()) } else { Err("Could not write to the registry".into()) }
    }
}

impl Drop for Key {
    fn drop(&mut self) {
        // SAFETY: we own the handle.
        unsafe {
            let _ = RegCloseKey(self.0);
        }
    }
}

fn set(path: &str, name: &str, value: &str) -> Result<(), String> {
    Key::create(path)?.set(name, value)
}

fn delete_tree(path: &str) {
    // SAFETY: plain registry call; a missing key is fine.
    unsafe {
        let _ = RegDeleteTreeW(HKEY_CURRENT_USER, &HSTRING::from(path));
        let _ = RegDeleteKeyW(HKEY_CURRENT_USER, &HSTRING::from(path));
    }
}

fn delete_value(path: &str, name: &str) {
    let mut h = HKEY::default();
    // SAFETY: the key handle is closed right after use.
    unsafe {
        if RegOpenKeyExW(HKEY_CURRENT_USER, &HSTRING::from(path), None, KEY_SET_VALUE, &mut h) == ERROR_SUCCESS {
            let _ = RegDeleteValueW(h, &HSTRING::from(name));
            let _ = RegCloseKey(h);
        }
    }
}

fn read_string(path: &str, name: &str) -> Option<String> {
    let mut buf = vec![0u16; 4096];
    let mut size = (buf.len() * 2) as u32;
    // SAFETY: the buffer and its byte size are passed together.
    let r = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            &HSTRING::from(path),
            &HSTRING::from(name),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr().cast()),
            Some(&mut size),
        )
    };
    if r != ERROR_SUCCESS {
        return None;
    }
    let n = (size as usize / 2).min(buf.len());
    let s = String::from_utf16_lossy(&buf[..n]);
    Some(s.trim_end_matches('\0').to_owned())
}

fn unregister_extensions() {
    let Some(list) = read_string(APP_KEY, "Extensions") else { return };
    for ext in list.split(';').filter(|e| !e.is_empty()) {
        let progid = format!("Windmolen.{ext}");
        delete_value(&format!(r"{CLASSES}\.{ext}\OpenWithProgids"), &progid);
        delete_tree(&format!(r"{CLASSES}\{progid}"));
        delete_tree(&format!(r"{CLASSES}\SystemFileAssociations\.{ext}\shell\{VERB}"));
    }
}

/// Extensions safe to put in a registry path, sorted and without repeats.
fn clean(exts: &[String]) -> Vec<String> {
    let mut v: Vec<String> =
        exts.iter().map(|e| e.to_ascii_lowercase()).filter(|e| !e.is_empty() && e.chars().all(|c| c.is_ascii_alphanumeric())).collect();
    v.sort();
    v.dedup();
    v
}

fn write(exe: &Path, exts: &[String]) -> Result<(), String> {
    let exe_s = exe.display().to_string();
    let open = format!("\"{exe_s}\" \"%1\"");
    // The right-click verb opens just the chosen files, not their folder.
    let open_only = format!("\"{exe_s}\" --only \"%1\"");
    let icon = format!("\"{exe_s}\",0");

    unregister_extensions();
    delete_tree(LEGACY_UNINSTALL_KEY);
    let caps = format!(r"{APP_KEY}\Capabilities");
    let app = format!(r"{CLASSES}\Applications\{EXE}");
    set(&app, "FriendlyAppName", NAME)?;
    set(&format!(r"{app}\DefaultIcon"), "", &icon)?;
    set(&format!(r"{app}\shell\open\command"), "", &open)?;
    set(&format!(r"{app}\shell\open"), "MultiSelectModel", "Player")?;
    set(&caps, "ApplicationName", NAME)?;
    set(&caps, "ApplicationDescription", "A lean, fast photo viewer")?;
    set(&caps, "ApplicationIcon", &icon)?;

    for ext in exts {
        let progid = format!("Windmolen.{ext}");
        let pk = format!(r"{CLASSES}\{progid}");
        let type_name = format!("{} Image", ext.to_uppercase());
        set(&pk, "", &type_name)?;
        set(&pk, "FriendlyTypeName", &type_name)?;
        set(&format!(r"{pk}\DefaultIcon"), "", &icon)?;
        set(&format!(r"{pk}\shell\open"), "MultiSelectModel", "Player")?;
        set(&format!(r"{pk}\shell\open\command"), "", &open)?;
        set(&format!(r"{CLASSES}\.{ext}\OpenWithProgids"), &progid, "")?;
        set(&format!(r"{app}\SupportedTypes"), &format!(".{ext}"), "")?;
        set(&format!(r"{caps}\FileAssociations"), &format!(".{ext}"), &progid)?;

        // Right-click › "Open in Windmolen", for any number of files.
        let verb = format!(r"{CLASSES}\SystemFileAssociations\.{ext}\shell\{VERB}");
        set(&verb, "", &format!("Open in {NAME}"))?;
        set(&verb, "Icon", &icon)?;
        set(&verb, "MultiSelectModel", "Player")?;
        set(&format!(r"{verb}\command"), "", &open_only)?;
    }
    set(r"Software\RegisteredApplications", NAME, &caps)?;
    set(APP_KEY, "InstallPath", &exe_s)?;
    set(APP_KEY, "Extensions", &exts.join(";"))?;
    Ok(())
}

fn notify_shell() {
    // SAFETY: plain notification without items.
    unsafe { SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None) };
}

/// Registers the running executable for `exts` (`--register`, run by the
/// installer).
pub fn register(exts: &[String]) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    write(&exe, &clean(exts))?;
    notify_shell();
    Ok(())
}

/// Removes every registration (`--unregister`, run by the uninstaller).
/// Settings and marks in %APPDATA% are kept.
pub fn unregister() {
    unregister_extensions();
    delete_tree(&format!(r"{CLASSES}\Applications\{EXE}"));
    delete_value(r"Software\RegisteredApplications", NAME);
    delete_tree(APP_KEY);
    delete_tree(LEGACY_UNINSTALL_KEY);
    notify_shell();
}

/// At start-up: when this is the registered copy and the decodable formats
/// changed (a codec was installed or removed), registers them again.
pub fn refresh(exts: &[String]) {
    let Some(registered) = read_string(APP_KEY, "InstallPath") else { return };
    let same = |a: &Path, b: &Path| std::fs::canonicalize(a).ok().zip(std::fs::canonicalize(b).ok()).is_some_and(|(a, b)| a == b);
    let Ok(exe) = std::env::current_exe() else { return };
    let exts = clean(exts);
    if same(&exe, Path::new(&registered)) && read_string(APP_KEY, "Extensions").as_deref() != Some(exts.join(";").as_str()) {
        let _ = write(&exe, &exts);
        notify_shell();
    }
}

/// Whether this process is the copy the installer registered.
pub fn is_installed_copy() -> bool {
    let canon = |p: &Path| std::fs::canonicalize(p).ok();
    read_string(APP_KEY, "InstallPath")
        .is_some_and(|p| std::env::current_exe().ok().and_then(|e| canon(&e)).is_some_and(|e| Some(e) == canon(Path::new(&p))))
}
