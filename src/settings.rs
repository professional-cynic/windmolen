// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 professional-cynic

//! Persistent settings and marks, stored as a small tab-separated UTF-8 file.
//!
//! Paths that are not valid Unicode are kept for the session but not saved.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

pub const SLOTS: usize = 9;
const HEADER: &str = "windmolen 1";
const MAX_FILE_SIZE: u64 = 32 * 1024 * 1024;

/// Window size in DIPs (not maximised) and whether it was maximised. Like
/// GNOME apps, windows don't keep their position: they open centred.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Placement {
    pub w: i32,
    pub h: i32,
    pub maximised: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    /// Destination folder of each mark slot.
    pub mark_dirs: [Option<PathBuf>; SLOTS],
    /// folder → file name → bit set of mark slots.
    marks: BTreeMap<PathBuf, BTreeMap<OsString, u16>>,
    pub placement: Option<Placement>,
    pub properties: bool,
    /// Check GitHub for new releases at most once a day.
    pub auto_update: bool,
    /// Unix time of the last update check.
    pub last_update_check: u64,
}

impl Default for Settings {
    fn default() -> Self {
        Self { mark_dirs: Default::default(), marks: BTreeMap::new(), placement: None, properties: false, auto_update: true, last_update_check: 0 }
    }
}

const ALL_SLOTS: u16 = (1 << SLOTS) - 1;

impl Settings {
    pub fn marks(&self, folder: &Path, name: &OsStr) -> u16 {
        self.marks.get(folder).and_then(|m| m.get(name)).copied().unwrap_or(0)
    }

    pub fn set_marks(&mut self, folder: &Path, name: &OsStr, bits: u16) {
        let bits = bits & ALL_SLOTS;
        if bits == 0 {
            if let Some(files) = self.marks.get_mut(folder) {
                files.remove(name);
                if files.is_empty() {
                    self.marks.remove(folder);
                }
            }
        } else {
            self.marks.entry(folder.to_path_buf()).or_default().insert(name.to_os_string(), bits);
        }
    }

    pub fn toggle(&mut self, folder: &Path, name: &OsStr, slot: usize) -> u16 {
        let bits = self.marks(folder, name) ^ (1 << slot);
        self.set_marks(folder, name, bits);
        bits & ALL_SLOTS
    }

    /// Number of files carrying each mark, over all folders.
    pub fn counts(&self) -> [usize; SLOTS] {
        let mut c = [0; SLOTS];
        for bits in self.marks.values().flat_map(|f| f.values()) {
            for (slot, n) in c.iter_mut().enumerate() {
                *n += usize::from(bits & (1 << slot) != 0);
            }
        }
        c
    }

    /// Every marked file as (folder, name, bits).
    pub fn marked(&self) -> Vec<(PathBuf, OsString, u16)> {
        self.marks.iter().flat_map(|(dir, files)| files.iter().map(move |(n, &b)| (dir.clone(), n.clone(), b))).collect()
    }

    pub fn serialise(&self) -> String {
        let mut s = format!("{HEADER}\n");
        for (slot, dir) in self.mark_dirs.iter().enumerate() {
            if let Some(d) = dir.as_deref().and_then(storable) {
                s += &format!("dir\t{}\t{d}\n", slot + 1);
            }
        }
        for (dir, files) in &self.marks {
            let Some(d) = storable(dir) else { continue };
            for (name, bits) in files {
                if let Some(n) = storable(Path::new(name)) {
                    s += &format!("mark\t{d}\t{n}\t{bits}\n");
                }
            }
        }
        if let Some(p) = self.placement {
            s += &format!("size\t{}\t{}\t{}\n", p.w, p.h, u8::from(p.maximised));
        }
        s += &format!("properties\t{}\n", u8::from(self.properties));
        s += &format!("updates\t{}\t{}\n", u8::from(self.auto_update), self.last_update_check);
        s
    }

    /// Lenient parser: malformed or unknown lines are skipped. `None` when
    /// the text is not a settings file at all.
    pub fn parse(text: &str) -> Option<Self> {
        let mut st = Self::default();
        // Notepad may add a byte-order mark.
        let mut lines = text.trim_start_matches('\u{feff}').lines();
        if lines.next() != Some(HEADER) {
            return None;
        }
        for line in lines {
            let f: Vec<&str> = line.split('\t').collect();
            match f.as_slice() {
                ["dir", slot, path] if !path.is_empty() => {
                    if let Ok(n @ 1..=9) = slot.parse::<usize>() {
                        st.mark_dirs[n - 1] = Some(PathBuf::from(path));
                    }
                }
                ["mark", dir, name, bits] if !dir.is_empty() && valid_name(name) => {
                    if let Ok(b) = bits.parse::<u16>() {
                        st.set_marks(Path::new(dir), OsStr::new(name), b);
                    }
                }
                ["size", w, h, m] => {
                    if let (Ok(w @ 1..=100_000), Ok(h @ 1..=100_000)) = (w.parse(), h.parse()) {
                        st.placement = Some(Placement { w, h, maximised: *m == "1" });
                    }
                }
                ["properties", v] => st.properties = *v == "1",
                ["updates", on, last] => {
                    st.auto_update = *on != "0";
                    st.last_update_check = last.parse().unwrap_or(0);
                }
                _ => {}
            }
        }
        Some(st)
    }

    /// Reads the settings file; a missing one gives the defaults. Anything
    /// else that cannot be read is an error, so that it is never saved over:
    /// a passing read failure must not wipe every mark.
    pub fn load(path: &Path) -> io::Result<Self> {
        let mut f = match fs::File::open(path) {
            Ok(f) => f,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(e),
        };
        if f.metadata()?.len() > MAX_FILE_SIZE {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "the settings file is too large"));
        }
        let mut text = String::new();
        f.read_to_string(&mut text)?;
        Self::parse(&text).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "not a Windmolen settings file"))
    }

    /// Writes atomically: a temporary file (named per process, so windows
    /// never share one) is written, flushed and renamed over the original.
    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
        let result = fs::File::create(&tmp)
            .and_then(|mut f| {
                f.write_all(self.serialise().as_bytes())?;
                f.sync_all()
            })
            .and_then(|()| fs::rename(&tmp, path));
        if result.is_err() {
            let _ = fs::remove_file(&tmp);
        }
        result
    }
}

/// Text that can be stored in one tab-separated field.
fn storable(p: &Path) -> Option<&str> {
    p.to_str().filter(|t| !t.contains(['\t', '\n', '\r']))
}

/// A plain file name: no separators, no traversal.
fn valid_name(n: &str) -> bool {
    !n.is_empty() && n != "." && n != ".." && !n.contains(['/', '\\', ':'])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let mut s = Settings::default();
        s.mark_dirs[0] = Some(PathBuf::from(r"D:\Holiday"));
        s.mark_dirs[8] = Some(PathBuf::from(r"E:\Best ones"));
        s.toggle(Path::new(r"C:\Photos"), OsStr::new("a.jpg"), 0);
        s.toggle(Path::new(r"C:\Photos"), OsStr::new("a.jpg"), 8);
        s.toggle(Path::new(r"C:\Other"), OsStr::new("b.png"), 2);
        s.placement = Some(Placement { w: 800, h: 600, maximised: true });
        s.properties = true;
        s.auto_update = false;
        s.last_update_check = 1_790_000_000;
        let t = Settings::parse(&s.serialise()).unwrap();
        assert_eq!(t, s);
        assert_eq!(t.counts(), [1, 0, 1, 0, 0, 0, 0, 0, 1]);
    }

    #[test]
    fn toggling_off_removes_entry() {
        let mut s = Settings::default();
        s.toggle(Path::new("x"), OsStr::new("a"), 3);
        s.toggle(Path::new("x"), OsStr::new("a"), 3);
        assert!(s.marked().is_empty());
    }

    #[test]
    fn rejects_hostile_input() {
        let t = "windmolen 1\nmark\tC:\\x\t..\\..\\evil\t1\nmark\tC:\\x\tok.jpg\tnope\ndir\t0\tX\ndir\t10\tY\nsize\t-3\t4\t0\n";
        let s = Settings::parse(t).unwrap();
        assert!(s.marked().is_empty());
        assert!(s.mark_dirs.iter().all(Option::is_none));
        assert!(s.placement.is_none());
        assert_eq!(Settings::parse("garbage\nmark\ta\tb\t1"), None);
        assert_eq!(Settings::parse("\u{feff}windmolen 1\nproperties\t1\n").map(|s| s.properties), Some(true));
    }

    #[test]
    fn unreadable_files_are_errors() {
        let dir = std::env::temp_dir().join(format!("windmolen-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.txt");
        // Missing: the defaults.
        assert_eq!(Settings::load(&path).unwrap(), Settings::default());
        // Not a settings file: an error, never silently the defaults.
        fs::write(&path, "something else").unwrap();
        assert_eq!(Settings::load(&path).unwrap_err().kind(), io::ErrorKind::InvalidData);
        let mut s = Settings::default();
        s.toggle(Path::new("C:\\x"), OsStr::new("a.jpg"), 1);
        s.save(&path).unwrap();
        assert_eq!(Settings::load(&path).unwrap(), s);
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1, "no temporary file left");
        fs::remove_dir_all(&dir).unwrap();
    }
}
