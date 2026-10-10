//! Updates from GitHub Releases over HTTPS (WinHTTP + Schannel), for the
//! copy the installer put in place.
//!
//! The release's `latest.txt` names the version and the SHA-256 of
//! `Windmolen-Setup.exe`. A newer installer is downloaded to the temporary
//! folder and verified; on **Restart** it runs silently, closes Windmolen,
//! updates it and opens it again.

use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use windows::Win32::Networking::WinHttp::*;
use windows::Win32::Security::Cryptography::{BCRYPT_SHA256_ALG_HANDLE, BCryptHash};
use windows::core::{HSTRING, PCWSTR, w};

use crate::release::{self, Manifest};

const HOST: PCWSTR = w!("github.com");
const MAX_MANIFEST: usize = 4 * 1024;
const MAX_INSTALLER: usize = 64 * 1024 * 1024;
const INSTALLER: &str = "Windmolen-Setup.exe";

/// The verified installer and its SHA-256, once downloaded.
static READY: Mutex<Option<(PathBuf, [u8; 32])>> = Mutex::new(None);

pub enum Outcome {
    UpToDate,
    /// The new version's installer is downloaded and verified.
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
            WinHttpReadData(request.0, buf.as_mut_ptr().cast(), buf.len() as u32, &mut read)
                .map_err(|e| format!("Download interrupted: {}", e.message()))?;
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

/// Where the installer of `version` is downloaded to.
fn setup_path(version: &str) -> PathBuf {
    std::env::temp_dir().join(format!("Windmolen-Setup-{version}.exe"))
}

/// Checks for a newer release and, if there is one, downloads its installer.
/// Blocking: call from a worker thread.
pub fn run() -> Outcome {
    match try_run() {
        Ok(o) => o,
        Err(e) => Outcome::Failed(e),
    }
}

fn try_run() -> Result<Outcome, String> {
    if !crate::install::is_installed_copy() {
        return Err("only installed copies update; install Windmolen with Windmolen-Setup.exe".into());
    }
    let m = manifest()?;
    if m.version <= release::current() {
        return Ok(Outcome::UpToDate);
    }
    let version = release::version_string(m.version);
    let path = setup_path(&version);

    // Another window may have downloaded it already.
    if std::fs::read(&path).ok().and_then(|d| sha256(&d).ok()) != Some(m.sha256) {
        let data = get(&release::latest_asset(INSTALLER), MAX_INSTALLER)?;
        if sha256(&data)? != m.sha256 {
            return Err("The download did not match the published checksum".into());
        }
        if !data.starts_with(b"MZ") {
            return Err("The download is not a Windows program".into());
        }
        std::fs::write(&path, &data).map_err(|e| format!("Could not save the update: {e}"))?;
    }
    *READY.lock().unwrap_or_else(PoisonError::into_inner) = Some((path, m.sha256));
    Ok(Outcome::Ready(version))
}

/// Runs the downloaded installer silently: it closes every Windmolen window,
/// updates the program and starts it again with `open`, if any.
pub fn install(open: Option<&Path>) -> Result<(), String> {
    let Some((path, hash)) = READY.lock().unwrap_or_else(PoisonError::into_inner).clone() else {
        return Err("No update has been downloaded".into());
    };
    // Checked again: the temporary folder is shared with other programs.
    if std::fs::read(&path).ok().and_then(|d| sha256(&d).ok()) != Some(hash) {
        return Err("The downloaded update has changed; check for updates again".into());
    }
    let mut cmd = std::process::Command::new(&path);
    cmd.args(["/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/CLOSEAPPLICATIONS", "/UPDATE=1"]);
    if let Some(f) = open {
        let mut arg = std::ffi::OsString::from("/OPEN=");
        arg.push(f);
        cmd.arg(arg);
    }
    cmd.spawn().map(drop).map_err(|e| e.to_string())
}

/// Deletes installers downloaded for versions this one has caught up with.
pub fn clean_up() {
    let Ok(rd) = std::fs::read_dir(std::env::temp_dir()) else { return };
    for e in rd.flatten() {
        let name = e.file_name();
        let name = name.to_string_lossy();
        let version = name.strip_prefix("Windmolen-Setup-").and_then(|v| v.strip_suffix(".exe")).and_then(release::parse_version);
        if version.is_some_and(|v| v <= release::current()) {
            let _ = std::fs::remove_file(e.path());
        }
    }
}
