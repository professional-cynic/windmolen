//! Self-update from GitHub Releases over HTTPS (WinHTTP + Schannel).
//!
//! The release's `latest.txt` names the version and the SHA-256 of the
//! executable. A newer build is downloaded beside the running executable,
//! verified, and swapped in: a running executable can be renamed, so the
//! old file moves aside and is deleted on a later start. The current
//! process keeps running the old code until it restarts.

use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

use windows::Win32::Networking::WinHttp::*;
use windows::Win32::Security::Cryptography::{BCRYPT_SHA256_ALG_HANDLE, BCryptHash};
use windows::core::{HSTRING, PCWSTR, w};

use crate::release::{self, Manifest};

const HOST: PCWSTR = w!("github.com");
const MAX_MANIFEST: usize = 4 * 1024;
const MAX_EXECUTABLE: usize = 64 * 1024 * 1024;

/// Held while the new program file is written and swapped in.
static SWAPPING: Mutex<()> = Mutex::new(());

/// Waits for an update that is being put in place and keeps any other from
/// starting. Exiting between the two renames would leave no program behind.
pub fn hold_for_exit() -> MutexGuard<'static, ()> {
    SWAPPING.lock().unwrap_or_else(PoisonError::into_inner)
}

pub enum Outcome {
    UpToDate,
    /// The new version is in place; it runs after a restart.
    Ready(String),
    Failed(String),
}

/// A WinHTTP handle, closed on drop.
struct Handle(*mut c_void);

impl Handle {
    fn new(h: *mut c_void, what: &str) -> Result<Self, String> {
        if h.is_null() { Err(format!("Could not {what}")) } else { Ok(Self(h)) }
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: a handle WinHTTP gave us, closed once.
        unsafe {
            let _ = WinHttpCloseHandle(self.0);
        }
    }
}

/// HTTPS GET on github.com, following redirects (HTTPS only) to GitHub's
/// asset host. Fails on non-200 status or more than `limit` bytes.
fn get(path: &str, limit: usize) -> Result<Vec<u8>, String> {
    let ua = HSTRING::from(format!("Windmolen/{}", env!("CARGO_PKG_VERSION")));
    // SAFETY: WinHTTP calls on handles owned by `Handle` guards; buffers
    // are locals sized as passed.
    unsafe {
        let session = Handle::new(WinHttpOpen(&ua, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, PCWSTR::null(), PCWSTR::null(), 0), "start a connection")?;
        let _ = WinHttpSetTimeouts(session.0, 10_000, 10_000, 30_000, 30_000);
        let connect = Handle::new(WinHttpConnect(session.0, HOST, INTERNET_DEFAULT_HTTPS_PORT, 0), "reach github.com")?;
        let request = Handle::new(
            WinHttpOpenRequest(connect.0, w!("GET"), &HSTRING::from(path), PCWSTR::null(), PCWSTR::null(), std::ptr::null(), WINHTTP_FLAG_SECURE),
            "create the request",
        )?;
        WinHttpSendRequest(request.0, None, None, 0, 0, 0).map_err(|e| format!("Could not connect: {}", e.message()))?;
        WinHttpReceiveResponse(request.0, std::ptr::null_mut()).map_err(|e| format!("No response: {}", e.message()))?;

        let mut status = 0u32;
        let mut len = 4u32;
        let mut index = 0u32;
        WinHttpQueryHeaders(
            request.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            PCWSTR::null(),
            Some((&mut status as *mut u32).cast()),
            &mut len,
            &mut index,
        )
        .map_err(|e| e.message())?;
        match status {
            200 => {}
            404 => return Err("No release has been published yet".into()),
            s => return Err(format!("GitHub answered with status {s}")),
        }

        let mut body = Vec::new();
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            let mut read = 0u32;
            WinHttpReadData(request.0, buf.as_mut_ptr().cast(), buf.len() as u32, &mut read).map_err(|e| format!("Download interrupted: {}", e.message()))?;
            if read == 0 {
                break;
            }
            if body.len() + read as usize > limit {
                return Err("The download is larger than expected".into());
            }
            body.extend_from_slice(&buf[..read as usize]);
        }
        Ok(body)
    }
}

fn sha256(data: &[u8]) -> Result<[u8; 32], String> {
    let mut out = [0u8; 32];
    // SAFETY: the pseudo-handle needs no setup; `out` is the digest size.
    let status = unsafe { BCryptHash(BCRYPT_SHA256_ALG_HANDLE, None, data, &mut out) };
    if status.is_ok() { Ok(out) } else { Err("Could not verify the download".into()) }
}

fn manifest() -> Result<Manifest, String> {
    let text = get(&release::latest_asset("latest.txt"), MAX_MANIFEST)?;
    let text = String::from_utf8(text).map_err(|_| "The release manifest is not text".to_owned())?;
    release::parse_manifest(&text).ok_or_else(|| "The release manifest is malformed".into())
}

/// Moves `new` into place at `exe`; the running file moves aside first.
fn swap(exe: &Path, new: &Path) -> Result<(), String> {
    let old = old_name(exe);
    std::fs::rename(exe, &old).map_err(|e| format!("Could not replace the program: {e}"))?;
    if let Err(e) = std::fs::rename(new, exe) {
        // Put the original back so the app keeps working.
        let _ = std::fs::rename(&old, exe);
        return Err(format!("Could not replace the program: {e}"));
    }
    Ok(())
}

fn old_name(exe: &Path) -> PathBuf {
    use std::sync::atomic::{AtomicU32, Ordering};
    static N: AtomicU32 = AtomicU32::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    exe.with_file_name(format!("windmolen.old-{}-{n}.exe", std::process::id()))
}

/// Checks for a newer release and, if there is one, installs it.
/// Blocking: call from a worker thread.
pub fn run() -> Outcome {
    match try_run() {
        Ok(o) => o,
        Err(e) => Outcome::Failed(e),
    }
}

fn try_run() -> Result<Outcome, String> {
    let m = manifest()?;
    if m.version <= release::current() {
        return Ok(Outcome::UpToDate);
    }
    let version = release::version_string(m.version);
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;

    // Another window may have installed it already.
    if std::fs::read(&exe).ok().and_then(|d| sha256(&d).ok()) == Some(m.sha256) {
        return Ok(Outcome::Ready(version));
    }

    let data = get(&release::latest_asset("windmolen.exe"), MAX_EXECUTABLE)?;
    if sha256(&data)? != m.sha256 {
        return Err("The download did not match the published checksum".into());
    }
    if !data.starts_with(b"MZ") {
        return Err("The download is not a Windows program".into());
    }

    let _swapping = hold_for_exit();
    let tmp = crate::sys::temp_beside(&exe).map_err(|e| format!("Could not write next to the program: {e}"))?;
    let result = std::fs::write(&tmp, &data).map_err(|e| format!("Could not write the update: {e}")).and_then(|()| swap(&exe, &tmp));
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result.map(|()| Outcome::Ready(version))
}

/// Deletes executables moved aside by earlier updates (once unused).
pub fn clean_up() {
    let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)) else { return };
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let name = e.file_name();
        let name = name.to_string_lossy();
        if name.starts_with("windmolen.old-") && name.ends_with(".exe") {
            let _ = std::fs::remove_file(e.path());
        }
    }
}
