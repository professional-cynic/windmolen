//! Per-user installation: no administrator rights, nothing outside HKCU and
//! the user's own folders.
//!
//! Registers the app the way Windows expects of a default-app candidate:
//! a ProgID per extension, `OpenWithProgids` entries, an `Applications`
//! entry, `Capabilities` under `RegisteredApplications` (which lists it in
//! Settings › Default apps), a right-click verb that accepts multiple
//! files, a Start menu shortcut and an entry in Settings › Apps.
//!
//! Windows does not let an app make itself the default: the user confirms
//! that in Settings, which we open for them.

use std::path::{Path, PathBuf};

use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, IPersistFile};
use windows::Win32::System::Registry::*;
use windows::Win32::UI::Shell::*;
use windows::core::{HSTRING, Interface, PCWSTR, w};

use crate::sys;

const NAME: &str = "Windmolen";
const EXE: &str = "windmolen.exe";
const APP_KEY: &str = r"Software\Windmolen";
const CLASSES: &str = r"Software\Classes";
const UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\Windmolen";
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

    fn set_dword(&self, name: &str, value: u32) -> Result<(), String> {
        // SAFETY: four bytes, as REG_DWORD requires.
        let r = unsafe { RegSetValueExW(self.0, &HSTRING::from(name), None, REG_DWORD, Some(&value.to_le_bytes())) };
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

/// Where the installed copy lives, if any.
pub fn installed_exe() -> Option<PathBuf> {
    read_string(APP_KEY, "InstallPath").map(PathBuf::from).filter(|p| p.is_file())
}

/// Whether this process is the installed copy.
pub fn running_installed() -> bool {
    let (Some(a), Ok(b)) = (installed_exe(), std::env::current_exe()) else { return false };
    let norm = |p: &Path| std::fs::canonicalize(p).map(|c| c.to_string_lossy().to_lowercase()).unwrap_or_default();
    norm(&a) == norm(&b)
}

fn install_dir() -> Result<PathBuf, String> {
    // %LOCALAPPDATA%\Programs, the per-user Program Files.
    sys::known_folder(&FOLDERID_UserProgramFiles)
        .or_else(|| sys::known_folder(&FOLDERID_LocalAppData).map(|d| d.join("Programs")))
        .map(|d| d.join(NAME))
        .ok_or_else(|| "Could not find your Programs folder".into())
}

fn shortcut_path() -> Option<PathBuf> {
    Some(sys::known_folder(&FOLDERID_Programs)?.join(format!("{NAME}.lnk")))
}

/// Copies the running executable into place; replacing a copy that is
/// still running works because a running executable can be renamed.
fn place_executable(dir: &Path) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("Could not create {}: {e}", dir.display()))?;
    let dst = dir.join(EXE);
    let src = std::env::current_exe().map_err(|e| e.to_string())?;
    let same = std::fs::canonicalize(&src).ok().zip(std::fs::canonicalize(&dst).ok()).is_some_and(|(a, b)| a == b);
    if !same {
        if dst.exists() {
            let old = dir.join(format!("windmolen.old-{}.exe", std::process::id()));
            std::fs::rename(&dst, &old).map_err(|e| format!("Could not replace the installed copy: {e}"))?;
        }
        std::fs::copy(&src, &dst).map_err(|e| format!("Could not copy the program: {e}"))?;
    }
    // Leftovers from earlier updates whose process has since exited.
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            if e.file_name().to_string_lossy().starts_with("windmolen.old-") {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
    Ok(dst)
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

fn register(exe: &Path, exts: &[String]) -> Result<(), String> {
    let exe_s = exe.display().to_string();
    let open = format!("\"{exe_s}\" \"%1\"");
    let icon = format!("\"{exe_s}\",0");
    let exts: Vec<&str> = exts.iter().map(String::as_str).filter(|e| !e.is_empty() && e.chars().all(|c| c.is_ascii_alphanumeric())).collect();

    unregister_extensions();
    let caps = format!(r"{APP_KEY}\Capabilities");
    let app = format!(r"{CLASSES}\Applications\{EXE}");
    set(&app, "FriendlyAppName", NAME)?;
    set(&format!(r"{app}\DefaultIcon"), "", &icon)?;
    set(&format!(r"{app}\shell\open\command"), "", &open)?;
    set(&format!(r"{app}\shell\open"), "MultiSelectModel", "Player")?;
    set(&caps, "ApplicationName", NAME)?;
    set(&caps, "ApplicationDescription", "A lean, fast photo viewer")?;
    set(&caps, "ApplicationIcon", &icon)?;

    for ext in &exts {
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
        set(&format!(r"{verb}\command"), "", &open)?;
    }
    set(r"Software\RegisteredApplications", NAME, &caps)?;
    set(APP_KEY, "InstallPath", &exe_s)?;
    set(APP_KEY, "Extensions", &exts.join(";"))?;

    let un = Key::create(UNINSTALL_KEY)?;
    un.set("DisplayName", NAME)?;
    un.set("DisplayVersion", env!("CARGO_PKG_VERSION"))?;
    un.set("Publisher", NAME)?;
    un.set("DisplayIcon", &exe_s)?;
    un.set("InstallLocation", &exe.parent().map(|d| d.display().to_string()).unwrap_or_default())?;
    un.set("UninstallString", &format!("\"{exe_s}\" --uninstall"))?;
    un.set_dword("NoModify", 1)?;
    un.set_dword("NoRepair", 1)?;
    let kb = std::fs::metadata(exe).map_or(0, |m| m.len() / 1024) as u32;
    un.set_dword("EstimatedSize", kb)?;
    Ok(())
}

fn create_shortcut(exe: &Path) -> Result<(), String> {
    let Some(lnk) = shortcut_path() else { return Ok(()) };
    // SAFETY: COM calls on a freshly created shell link; strings outlive them.
    unsafe {
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).map_err(|e| e.message())?;
        link.SetPath(&HSTRING::from(exe.as_os_str())).map_err(|e| e.message())?;
        link.SetDescription(w!("A lean, fast photo viewer")).map_err(|e| e.message())?;
        link.SetIconLocation(&HSTRING::from(exe.as_os_str()), 0).map_err(|e| e.message())?;
        if let Some(dir) = exe.parent() {
            link.SetWorkingDirectory(&HSTRING::from(dir.as_os_str())).map_err(|e| e.message())?;
        }
        let file: IPersistFile = link.cast().map_err(|e| e.message())?;
        file.Save(&HSTRING::from(lnk.as_os_str()), true).map_err(|e| e.message())
    }
}

fn notify_shell() {
    // SAFETY: plain notification without items.
    unsafe { SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None) };
}

/// Installs (or updates) for the current user and returns the installed
/// executable. `exts` are the extensions WIC can decode right now.
pub fn install(exts: &[String]) -> Result<PathBuf, String> {
    let exe = place_executable(&install_dir()?)?;
    register(&exe, exts)?;
    create_shortcut(&exe)?;
    notify_shell();
    Ok(exe)
}

/// Removes every registration, the shortcut and the program files.
/// Settings and marks in %APPDATA% are kept.
pub fn uninstall() -> Result<(), String> {
    let exe = installed_exe();
    unregister_extensions();
    delete_tree(&format!(r"{CLASSES}\Applications\{EXE}"));
    delete_value(r"Software\RegisteredApplications", NAME);
    delete_tree(APP_KEY);
    delete_tree(UNINSTALL_KEY);
    if let Some(lnk) = shortcut_path() {
        let _ = std::fs::remove_file(lnk);
    }
    notify_shell();

    let Some(dir) = exe.as_deref().and_then(Path::parent).map(Path::to_path_buf) else { return Ok(()) };
    if !dir.ends_with(NAME) {
        return Ok(());
    }
    let running_from_dir = std::env::current_exe().ok().and_then(|p| std::fs::canonicalize(p).ok()).zip(std::fs::canonicalize(&dir).ok()).is_some_and(|(p, d)| p.starts_with(d));
    if running_from_dir {
        // A running program cannot delete itself: let cmd do it once we exit.
        let d = dir.display().to_string();
        if d.contains(['%', '"', '^']) {
            return Err(format!("Remove the folder {d} by hand to finish"));
        }
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let script = format!("/c ping -n 3 127.0.0.1 >nul & del /f /q \"{d}\\windmolen*.exe\" & rmdir \"{d}\"");
        std::process::Command::new("cmd.exe").raw_arg(script).creation_flags(CREATE_NO_WINDOW).spawn().map_err(|e| e.to_string())?;
    } else {
        if let Ok(rd) = std::fs::read_dir(&dir) {
            for e in rd.flatten() {
                if e.file_name().to_string_lossy().starts_with("windmolen") {
                    let _ = std::fs::remove_file(e.path());
                }
            }
        }
        let _ = std::fs::remove_dir(&dir);
    }
    Ok(())
}

/// Opens Settings › Default apps, on the app's own page where supported.
pub fn open_default_apps() {
    if !sys::shell_open("ms-settings:defaultapps?registeredAppUser=Windmolen") {
        sys::shell_open("ms-settings:defaultapps");
    }
}
