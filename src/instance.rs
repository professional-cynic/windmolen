//! Groups the files of one Explorer action into one window.
//!
//! For a multi-selection, Explorer starts one process per file almost at
//! once. The first becomes the *leader*: it opens a hidden message-only
//! "gathering" window for a short time. Processes started meanwhile hand
//! their files over (`WM_COPYDATA`) and exit. Once the window closes, a file
//! opened later starts its own instance, as separate opens should.
//! A named mutex makes leader election and closing race-free.

use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::DataExchange::COPYDATASTRUCT;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{PCWSTR, w};

use crate::sys::NamedMutex;

const CLASS: PCWSTR = w!("HetPlantsoen.Gather");
const LOCK: PCWSTR = w!("Local\\HetPlantsoen.Launch");
/// Tag identifying our `WM_COPYDATA` payload ("HPls").
const TAG: usize = 0x4850_6C73;
const MAX_PAYLOAD: u32 = 1 << 20;
/// Stay open at least this long after start …
const MIN_OPEN: Duration = Duration::from_millis(1500);
/// … and this long after the latest arrival, but never longer than CAP.
const IDLE: Duration = Duration::from_millis(800);
const CAP: Duration = Duration::from_secs(10);
const TIMER: usize = 1;

pub const WM_GATHERED: u32 = WM_APP + 3;

struct State {
    started: Instant,
    last: Instant,
    inbox: Vec<PathBuf>,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);
static TARGET: AtomicIsize = AtomicIsize::new(0);

pub enum Launch {
    /// The files went to an instance that is still gathering: exit.
    Forwarded,
    /// This process shows the files (and may receive more).
    Leader,
}

/// Hands `paths` to a gathering instance, or becomes the leader.
pub fn start(paths: &[PathBuf]) -> Launch {
    if paths.is_empty() {
        return Launch::Leader;
    }
    let Some(_lock) = NamedMutex::lock(LOCK, 3000) else { return Launch::Leader };
    // SAFETY: plain window lookup and a synchronous message with a payload
    // that outlives the call.
    unsafe {
        if let Ok(h) = FindWindowExW(Some(HWND_MESSAGE), None, CLASS, PCWSTR::null()) {
            let data: Vec<u16> = paths.iter().flat_map(|p| p.as_os_str().encode_wide_vec()).collect();
            let cds = COPYDATASTRUCT { dwData: TAG, cbData: (data.len() * 2) as u32, lpData: data.as_ptr() as *mut _ };
            let mut result = 0usize;
            let sent = SendMessageTimeoutW(
                h,
                WM_COPYDATA,
                WPARAM(0),
                LPARAM(&cds as *const _ as isize),
                SMTO_ABORTIFHUNG | SMTO_BLOCK,
                2000,
                Some(&mut result),
            );
            if sent.0 != 0 && result == 1 {
                return Launch::Forwarded;
            }
        }
        // Become the leader while still holding the lock.
        let instance = GetModuleHandleW(None).unwrap_or_default();
        let wc = WNDCLASSW { lpfnWndProc: Some(proc), hInstance: instance.into(), lpszClassName: CLASS, ..Default::default() };
        RegisterClassW(&wc);
        if let Ok(h) = CreateWindowExW(WINDOW_EX_STYLE::default(), CLASS, PCWSTR::null(), WINDOW_STYLE::default(), 0, 0, 0, 0, Some(HWND_MESSAGE), None, Some(instance.into()), None) {
            let now = Instant::now();
            *STATE.lock().unwrap_or_else(|p| p.into_inner()) = Some(State { started: now, last: now, inbox: Vec::new() });
            SetTimer(Some(h), TIMER, 100, None);
        }
    }
    Launch::Leader
}

trait EncodeWide {
    fn encode_wide_vec(&self) -> Vec<u16>;
}

impl EncodeWide for std::ffi::OsStr {
    fn encode_wide_vec(&self) -> Vec<u16> {
        use std::os::windows::ffi::OsStrExt;
        self.encode_wide().chain(Some(0)).collect()
    }
}

/// The window that gets `WM_GATHERED` when files arrive.
pub fn set_target(hwnd: HWND) {
    TARGET.store(hwnd.0 as isize, Ordering::Relaxed);
    let pending = STATE.lock().unwrap_or_else(|p| p.into_inner()).as_ref().is_some_and(|s| !s.inbox.is_empty());
    if pending {
        notify();
    }
}

/// Files received since the last call.
pub fn take() -> Vec<PathBuf> {
    STATE.lock().unwrap_or_else(|p| p.into_inner()).as_mut().map(|s| std::mem::take(&mut s.inbox)).unwrap_or_default()
}

fn notify() {
    let t = TARGET.load(Ordering::Relaxed);
    if t != 0 {
        // SAFETY: posting to a possibly-dead window is harmless.
        unsafe {
            let _ = PostMessageW(Some(HWND(t as *mut _)), WM_GATHERED, WPARAM(0), LPARAM(0));
        }
    }
}

fn parse(cds: &COPYDATASTRUCT) -> Option<Vec<PathBuf>> {
    use std::os::windows::ffi::OsStringExt;
    if cds.dwData != TAG || cds.cbData > MAX_PAYLOAD || cds.cbData % 2 != 0 || cds.lpData.is_null() {
        return None;
    }
    // SAFETY: the sender guarantees `cbData` readable bytes for the
    // duration of the message; we copy them out immediately.
    let units = unsafe { std::slice::from_raw_parts(cds.lpData as *const u16, cds.cbData as usize / 2) };
    Some(
        units
            .split(|&c| c == 0)
            .filter(|s| !s.is_empty())
            .map(|s| PathBuf::from(std::ffi::OsString::from_wide(s)))
            .filter(|p| p.is_absolute())
            .collect(),
    )
}

extern "system" fn proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    // SAFETY: message parameters are as documented for each message.
    unsafe {
        match msg {
            WM_COPYDATA => {
                let Some(paths) = parse(&*(lp.0 as *const COPYDATASTRUCT)) else { return LRESULT(0) };
                if let Some(s) = STATE.lock().unwrap_or_else(|p| p.into_inner()).as_mut() {
                    s.last = Instant::now();
                    s.inbox.extend(paths);
                }
                notify();
                LRESULT(1)
            }
            WM_TIMER => {
                let due = STATE.lock().unwrap_or_else(|p| p.into_inner()).as_ref().is_none_or(|s| {
                    let now = Instant::now();
                    now - s.started > CAP || (now - s.started > MIN_OPEN && now - s.last > IDLE)
                });
                // Never block here: a sender may hold the lock while waiting
                // for this very thread. Try again on the next tick instead.
                if due {
                    if let Some(_lock) = NamedMutex::lock(LOCK, 0) {
                        let _ = KillTimer(Some(hwnd), TIMER);
                        let _ = DestroyWindow(hwnd);
                    }
                }
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}
