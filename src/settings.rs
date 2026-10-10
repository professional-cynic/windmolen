//! Persistent settings and marks, stored as a small tab-separated UTF-8 file.
//!
//! Paths that are not valid Unicode are kept for the session but not saved.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

pub const SLOTS: usize = 9;
const HEADER: &str = "het-plantsoen 1";
const MAX_FILE_SIZE: u64 = 32 * 1024 * 1024;

/// Window placement: normal-position rectangle and maximised flag.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Placement {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub maximised: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Settings {
    /// Destination folder of each mark slot.
    pub mark_dirs: [Option<PathBuf>; SLOTS],
    /// folder → file name → bit set of mark slots.
    marks: BTreeMap<PathBuf, BTreeMap<OsString, u16>>,
    pub placement: Option<Placement>,
    pub properties: bool,
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
        self.marks
            .iter()
            .flat_map(|(dir, files)| files.iter().map(move |(n, &b)| (dir.clone(), n.clone(), b)))
            .collect()
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
            s += &format!("window\t{}\t{}\t{}\t{}\t{}\n", p.x, p.y, p.w, p.h, u8::from(p.maximised));
        }
        s += &format!("properties\t{}\n", u8::from(self.properties));
        s
    }

    /// Lenient parser: malformed or unknown lines are skipped.
    pub fn parse(text: &str) -> Self {
        let mut st = Self::default();
        let mut lines = text.lines();
        if lines.next() != Some(HEADER) {
            return st;
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
                ["window", x, y, w, h, m] => {
                    if let (Ok(x), Ok(y), Ok(w), Ok(h)) = (x.parse(), y.parse(), w.parse(), h.parse()) {
                        if w > 0 && h > 0 {
                            st.placement = Some(Placement { x, y, w, h, maximised: *m == "1" });
                        }
                    }
                }
                ["properties", v] => st.properties = *v == "1",
                _ => {}
            }
        }
        st
    }

    pub fn load(path: &Path) -> Self {
        match fs::metadata(path) {
            Ok(m) if m.len() <= MAX_FILE_SIZE => {}
            _ => return Self::default(),
        }
        fs::read_to_string(path).map(|t| Self::parse(&t)).unwrap_or_default()
    }

    /// Writes atomically: a temporary file is written, flushed and renamed.
    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("tmp");
        {
            let mut f = fs::File::create(&tmp)?;
            f.write_all(self.serialise().as_bytes())?;
            f.sync_all()?;
        }
        fs::rename(&tmp, path)
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
        s.placement = Some(Placement { x: -5, y: 10, w: 800, h: 600, maximised: true });
        s.properties = true;
        let t = Settings::parse(&s.serialise());
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
        let t = "het-plantsoen 1\nmark\tC:\\x\t..\\..\\evil\t1\nmark\tC:\\x\tok.jpg\tnope\ndir\t0\tX\ndir\t10\tY\nwindow\t1\t2\t-3\t4\t0\n";
        let s = Settings::parse(t);
        assert!(s.marked().is_empty());
        assert!(s.mark_dirs.iter().all(Option::is_none));
        assert!(s.placement.is_none());
        assert_eq!(Settings::parse("garbage\nmark\ta\tb\t1"), Settings::default());
    }
}
