//! Thin, safe wrappers over the Win32 services the viewer needs: COM,
//! shell dialogs, theme settings, locale formatting and file operations.

use std::cmp::Ordering;
use std::ffi::{OsStr, OsString};
use std::os::windows::ffi::OsStringExt;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::time::SystemTime;

use windows::Win32::Foundation::{
    CloseHandle, ERROR_ALREADY_EXISTS, ERROR_CANCELLED, ERROR_FILE_EXISTS, FILETIME, HANDLE, HWND, LPARAM, SYSTEMTIME, WAIT_ABANDONED, WAIT_OBJECT_0,
    WPARAM,
};
use windows::Win32::Globalization::{DATE_SHORTDATE, GetDateFormatEx, GetTimeFormatEx, TIME_NOSECONDS};
use windows::Win32::Storage::FileSystem::{CopyFileW, MOVEFILE_COPY_ALLOWED, MOVEFILE_WRITE_THROUGH, MoveFileExW, REPLACE_FILE_FLAGS, ReplaceFileW};
use windows::Win32::System::Com::*;
use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
use windows::Win32::System::Threading::{CreateMutexW, ReleaseMutex, WaitForSingleObject};
use windows::Win32::System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime};
use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::{
    PostMessageW, SPI_GETCLIENTAREAANIMATION, SW_SHOWNORMAL, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW,
};
use windows::core::{BOOL, HSTRING, PCWSTR, PWSTR, Result, w};

/// Hands values from a worker thread to the UI thread and wakes it with
/// `msg`, whose handler drains the receiver.
pub struct Poster<T> {
    tx: Sender<T>,
    /// The window, as an integer so the poster can cross threads.
    hwnd: isize,
    msg: u32,
}

impl<T> Poster<T> {
    pub fn new(tx: Sender<T>, hwnd: HWND, msg: u32) -> Self {
        Self { tx, hwnd: hwnd.0 as isize, msg }
    }

    /// Returns false once the receiving side is gone.
    pub fn send(&self, value: T) -> bool {
        if self.tx.send(value).is_err() {
            return false;
        }
        // SAFETY: posting to a window that no longer exists fails harmlessly.
        unsafe {
            let _ = PostMessageW(Some(HWND(self.hwnd as *mut _)), self.msg, WPARAM(0), LPARAM(0));
        }
        true
    }
}

impl<T> Clone for Poster<T> {
    fn clone(&self) -> Self {
        Self { tx: self.tx.clone(), hwnd: self.hwnd, msg: self.msg }
    }
}

/// Initialises COM for the current thread and uninitialises it on drop.
pub struct ComGuard(bool);

impl ComGuard {
    pub fn sta() -> Self {
        // SAFETY: balanced by Drop on the same thread.
        Self(unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) }.is_ok())
    }

    pub fn mta() -> Self {
        // SAFETY: balanced by Drop on the same thread.
        Self(unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok())
    }
}

impl Drop for ComGuard {
    fn drop(&mut self) {
        if self.0 {
            // SAFETY: matched with a successful CoInitializeEx.
            unsafe { CoUninitialize() };
        }
    }
}

/// Takes ownership of a COM-allocated wide string.
fn take_pwstr(p: PWSTR) -> Option<OsString> {
    if p.is_null() {
        return None;
    }
    // SAFETY: `p` is a NUL-terminated string allocated with CoTaskMemAlloc.
    unsafe {
        let s = OsString::from_wide(p.as_wide());
        CoTaskMemFree(Some(p.0 as *const _));
        Some(s)
    }
}

fn dialog_result(d: &IFileDialog) -> Option<PathBuf> {
    // SAFETY: COM calls on a dialog that has been shown successfully.
    unsafe {
        let item = d.GetResult().ok()?;
        take_pwstr(item.GetDisplayName(SIGDN_FILESYSPATH).ok()?).map(PathBuf::from)
    }
}

fn show(d: &IFileDialog, owner: HWND) -> Result<bool> {
    // SAFETY: `owner` is our window; the dialog is modal to it.
    match unsafe { d.Show(Some(owner)) } {
        Ok(()) => Ok(true),
        Err(e) if e.code() == ERROR_CANCELLED.to_hresult() => Ok(false),
        Err(e) => Err(e),
    }
}

/// Native "Open" dialog filtered to `extensions`.
pub fn open_file_dialog(owner: HWND, extensions: &[String], start: Option<&Path>) -> Result<Option<PathBuf>> {
    let mut spec = extensions.iter().map(|e| format!("*.{e}")).collect::<Vec<_>>().join(";");
    if spec.is_empty() {
        spec = "*.*".into();
    }
    let (images, spec, all, any) = (HSTRING::from("Images"), HSTRING::from(spec), HSTRING::from("All Files"), HSTRING::from("*.*"));
    // SAFETY: the filter strings outlive the dialog call.
    unsafe {
        let d: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)?;
        let filters = [
            COMDLG_FILTERSPEC { pszName: PCWSTR(images.as_ptr()), pszSpec: PCWSTR(spec.as_ptr()) },
            COMDLG_FILTERSPEC { pszName: PCWSTR(all.as_ptr()), pszSpec: PCWSTR(any.as_ptr()) },
        ];
        d.SetFileTypes(&filters)?;
        d.SetOptions(d.GetOptions()? | FOS_FILEMUSTEXIST | FOS_FORCEFILESYSTEM)?;
        set_folder(&d, start);
        let d: IFileDialog = windows::core::Interface::cast(&d)?;
        Ok(if show(&d, owner)? { dialog_result(&d) } else { None })
    }
}

/// Native folder picker.
pub fn pick_folder_dialog(owner: HWND, title: &str, start: Option<&Path>) -> Result<Option<PathBuf>> {
    // SAFETY: plain COM calls on a freshly created dialog.
    unsafe {
        let d: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)?;
        d.SetOptions(d.GetOptions()? | FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST)?;
        d.SetTitle(&HSTRING::from(title))?;
        set_folder(&d, start);
        let d: IFileDialog = windows::core::Interface::cast(&d)?;
        Ok(if show(&d, owner)? { dialog_result(&d) } else { None })
    }
}

fn set_folder(d: &IFileOpenDialog, start: Option<&Path>) {
    let Some(dir) = start else { return };
    // SAFETY: COM calls with a local string.
    unsafe {
        if let Ok(item) = SHCreateItemFromParsingName::<_, _, IShellItem>(&HSTRING::from(dir.as_os_str()), None) {
            let _ = d.SetFolder(&item);
        }
    }
}

/// A per-user known folder such as `FOLDERID_RoamingAppData`.
pub fn known_folder(id: &windows::core::GUID) -> Option<PathBuf> {
    // SAFETY: the returned string is owned and freed by `take_pwstr`.
    unsafe { SHGetKnownFolderPath(id, KF_FLAG_CREATE, None) }.ok().and_then(take_pwstr).map(PathBuf::from)
}

/// `%APPDATA%\Windmolen\settings.txt` (per user, no elevation needed).
pub fn settings_path() -> Option<PathBuf> {
    Some(known_folder(&FOLDERID_RoamingAppData)?.join("Windmolen").join("settings.txt"))
}

/// A session-wide named mutex, released on drop.
pub struct NamedMutex(HANDLE);

impl NamedMutex {
    /// Waits up to `ms` milliseconds; `None` on timeout or failure.
    pub fn lock(name: PCWSTR, ms: u32) -> Option<Self> {
        // SAFETY: the handle is owned by the returned guard.
        unsafe {
            let h = CreateMutexW(None, false, name).ok()?;
            let r = WaitForSingleObject(h, ms);
            if r == WAIT_OBJECT_0 || r == WAIT_ABANDONED {
                Some(Self(h))
            } else {
                let _ = CloseHandle(h);
                None
            }
        }
    }
}

impl Drop for NamedMutex {
    fn drop(&mut self) {
        // SAFETY: we own the handle and hold the mutex.
        unsafe {
            let _ = ReleaseMutex(self.0);
            let _ = CloseHandle(self.0);
        }
    }
}

/// Opens a URI or file with its registered handler.
pub fn shell_open(target: &str) -> bool {
    // SAFETY: NUL-terminated strings that outlive the call.
    let r = unsafe { ShellExecuteW(None, w!("open"), &HSTRING::from(target), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL) };
    r.0 as isize > 32
}

fn reg_dword(key: PCWSTR, value: PCWSTR) -> Option<u32> {
    let mut data = 0u32;
    let mut size = 4u32;
    // SAFETY: out-pointers to locals of the declared size.
    let r = unsafe { RegGetValueW(HKEY_CURRENT_USER, key, value, RRF_RT_REG_DWORD, None, Some((&mut data as *mut u32).cast()), Some(&mut size)) };
    r.is_ok().then_some(data)
}

/// Whether apps should use the dark style.
pub fn prefers_dark() -> bool {
    reg_dword(w!(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize"), w!("AppsUseLightTheme")) == Some(0)
}

/// The Windows accent colour as (r, g, b).
pub fn accent_rgb() -> Option<(u8, u8, u8)> {
    // Stored as 0xAABBGGRR.
    let v = reg_dword(w!(r"Software\Microsoft\Windows\DWM"), w!("AccentColor"))?;
    Some((v as u8, (v >> 8) as u8, (v >> 16) as u8))
}

/// False when the user turned off animations in Windows settings.
pub fn animations_enabled() -> bool {
    let mut on = BOOL(1);
    // SAFETY: SPI_GETCLIENTAREAANIMATION writes a BOOL to the pointer.
    let ok =
        unsafe { SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION, 0, Some((&mut on as *mut BOOL).cast()), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0)) };
    ok.is_err() || on.as_bool()
}

/// Explorer's ordering ("img2" before "img10").
pub fn natural_cmp(a: &Path, b: &Path) -> Ordering {
    let (a, b) = (HSTRING::from(a.as_os_str()), HSTRING::from(b.as_os_str()));
    // SAFETY: both strings are NUL-terminated and live across the call.
    unsafe { StrCmpLogicalW(&a, &b) }.cmp(&0)
}

fn locale_format(st: &SYSTEMTIME) -> String {
    let mut d = [0u16; 128];
    let mut t = [0u16; 64];
    // SAFETY: buffers are locals; lengths are passed via the slices.
    let (dn, tn) = unsafe {
        (
            GetDateFormatEx(PCWSTR::null(), DATE_SHORTDATE, Some(st), PCWSTR::null(), Some(&mut d), PCWSTR::null()),
            GetTimeFormatEx(PCWSTR::null(), TIME_NOSECONDS, Some(st), PCWSTR::null(), Some(&mut t)),
        )
    };
    let s = |b: &[u16], n: i32| String::from_utf16_lossy(&b[..(n.max(1) as usize - 1).min(b.len())]);
    format!("{} {}", s(&d, dn), s(&t, tn))
}

/// Formats an EXIF date (already local time) with the user's locale.
pub fn format_date(d: [u16; 6]) -> String {
    let st = SYSTEMTIME { wYear: d[0], wMonth: d[1], wDay: d[2], wHour: d[3], wMinute: d[4], wSecond: d[5], ..Default::default() };
    locale_format(&st)
}

/// Formats a file time in local time with the user's locale.
pub fn format_time(t: SystemTime) -> String {
    let Ok(since) = t.duration_since(SystemTime::UNIX_EPOCH) else { return String::new() };
    // FILETIME counts 100 ns intervals since 1601.
    let ft = (since.as_nanos() / 100) as u64 + 116_444_736_000_000_000;
    let ft = FILETIME { dwLowDateTime: ft as u32, dwHighDateTime: (ft >> 32) as u32 };
    let (mut utc, mut local) = (SYSTEMTIME::default(), SYSTEMTIME::default());
    // SAFETY: out-pointers to locals.
    unsafe {
        if FileTimeToSystemTime(&ft, &mut utc).is_err() || SystemTimeToTzSpecificLocalTime(None, &utc, &mut local).is_err() {
            return String::new();
        }
    }
    locale_format(&local)
}

// ------------------------------------------------------------------------
// File operations that never overwrite
// ------------------------------------------------------------------------

fn exists_error(e: &windows::core::Error) -> bool {
    e.code() == ERROR_FILE_EXISTS.to_hresult() || e.code() == ERROR_ALREADY_EXISTS.to_hresult()
}

/// Candidate names: `stem.ext`, then `stem (2).ext`, `stem (3).ext`, …
/// Built from OS strings, so names that are not valid Unicode survive.
fn candidate(stem: &OsStr, ext: Option<&OsStr>, n: u32) -> OsString {
    let mut name = stem.to_os_string();
    if n > 1 {
        name.push(format!(" ({n})"));
    }
    if let Some(ext) = ext.filter(|e| !e.is_empty()) {
        name.push(".");
        name.push(ext);
    }
    name
}

/// Runs `op(target)` for successive free names in `dir` until it does not
/// fail with "already exists". The operation itself refuses to overwrite,
/// so there is no check-then-act race.
fn with_free_name(dir: &Path, stem: &OsStr, ext: Option<&OsStr>, mut op: impl FnMut(&HSTRING) -> Result<()>) -> std::result::Result<PathBuf, String> {
    for n in 1..10_000 {
        let target = dir.join(candidate(stem, ext, n));
        match op(&HSTRING::from(target.as_os_str())) {
            Ok(()) => return Ok(target),
            Err(e) if exists_error(&e) => continue,
            Err(e) => return Err(e.message()),
        }
    }
    Err("Too many files with the same name".into())
}

/// Copies `src` into `dir`, choosing a free name.
pub fn copy_into(src: &Path, dir: &Path) -> std::result::Result<PathBuf, String> {
    let s = HSTRING::from(src.as_os_str());
    with_free_name(dir, src.file_stem().unwrap_or_default(), src.extension(), |t| {
        // SAFETY: NUL-terminated paths; `true` refuses to overwrite.
        unsafe { CopyFileW(&s, t, true) }
    })
}

/// Moves `src` into `dir`, choosing a free name (copies across volumes).
pub fn move_into(src: &Path, dir: &Path) -> std::result::Result<PathBuf, String> {
    let s = HSTRING::from(src.as_os_str());
    with_free_name(dir, src.file_stem().unwrap_or_default(), src.extension(), |t| {
        // SAFETY: NUL-terminated paths; without MOVEFILE_REPLACE_EXISTING
        // the call fails instead of overwriting.
        unsafe { MoveFileExW(&s, t, MOVEFILE_COPY_ALLOWED | MOVEFILE_WRITE_THROUGH) }
    })
}

/// Renames `tmp` to "`stem` (edited).`ext`" next to `original`.
pub fn publish_copy(tmp: &Path, original: &Path, ext: &OsStr) -> std::result::Result<PathBuf, String> {
    let dir = original.parent().unwrap_or(Path::new("."));
    let mut stem = original.file_stem().unwrap_or_default().to_os_string();
    stem.push(" (edited)");
    let s = HSTRING::from(tmp.as_os_str());
    with_free_name(dir, &stem, Some(ext), |t| {
        // SAFETY: as in `move_into`.
        unsafe { MoveFileExW(&s, t, MOVEFILE_WRITE_THROUGH) }
    })
}

/// Atomically replaces `original` with `tmp`, keeping the original's
/// attributes and permissions.
pub fn replace_with(original: &Path, tmp: &Path) -> std::result::Result<(), String> {
    // SAFETY: NUL-terminated paths on the same volume.
    unsafe { ReplaceFileW(&HSTRING::from(original.as_os_str()), &HSTRING::from(tmp.as_os_str()), PCWSTR::null(), REPLACE_FILE_FLAGS(0), None, None) }
        .map_err(|e| e.message())
}

/// Creates a new, empty temporary file next to `near` and returns its path.
/// Creation is exclusive, so an existing file or link is never reused.
pub fn temp_beside(near: &Path) -> std::io::Result<PathBuf> {
    use std::sync::atomic::{AtomicU32, Ordering};
    static N: AtomicU32 = AtomicU32::new(0);
    let dir = near.parent().unwrap_or(Path::new("."));
    loop {
        let n = N.fetch_add(1, Ordering::Relaxed);
        let p = dir.join(format!(".windmolen-{}-{n}.tmp", std::process::id()));
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&p) {
            Ok(_) => return Ok(p),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;

    #[test]
    fn candidates() {
        let jpg = Some(OsStr::new("jpg"));
        assert_eq!(super::candidate(OsStr::new("a"), jpg, 1), "a.jpg");
        assert_eq!(super::candidate(OsStr::new("a"), jpg, 3), "a (3).jpg");
        assert_eq!(super::candidate(OsStr::new("a"), None, 2), "a (2)");
        assert_eq!(super::candidate(OsStr::new("a"), Some(OsStr::new("")), 1), "a");
    }
}
