//! Release manifest (`latest.txt`) and version comparison; pure and tested.
//!
//! The manifest is published next to each release's executable:
//!
//! ```text
//! 0.2.0
//! <sha-256 of windmolen.exe, 64 lower-case hex digits>
//! ```

pub const OWNER: &str = "professional-cynic";
pub const REPO: &str = "windmolen";

pub fn project_url() -> String {
    format!("https://github.com/{OWNER}/{REPO}")
}

pub fn issues_url() -> String {
    format!("https://github.com/{OWNER}/{REPO}/issues")
}

/// Path on github.com of an asset of the latest release.
pub fn latest_asset(name: &str) -> String {
    format!("/{OWNER}/{REPO}/releases/latest/download/{name}")
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Manifest {
    pub version: [u32; 3],
    pub sha256: [u8; 32],
}

/// Parses "MAJOR.MINOR.PATCH"; pre-releases and anything else are rejected.
pub fn parse_version(s: &str) -> Option<[u32; 3]> {
    let mut it = s.trim().trim_start_matches('v').split('.');
    let mut v = [0u32; 3];
    for part in &mut v {
        let p = it.next()?;
        if p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()) || p.len() > 9 {
            return None;
        }
        *part = p.parse().ok()?;
    }
    it.next().is_none().then_some(v)
}

pub fn version_string(v: [u32; 3]) -> String {
    format!("{}.{}.{}", v[0], v[1], v[2])
}

pub fn parse_manifest(text: &str) -> Option<Manifest> {
    let mut lines = text.lines().map(str::trim).filter(|l| !l.is_empty());
    let version = parse_version(lines.next()?)?;
    let hex = lines.next()?;
    if hex.len() != 64 || lines.next().is_some() {
        return None;
    }
    let mut sha256 = [0u8; 32];
    for (i, byte) in sha256.iter_mut().enumerate() {
        *byte = u8::from_str_radix(hex.get(2 * i..2 * i + 2)?, 16).ok()?;
    }
    Some(Manifest { version, sha256 })
}

pub fn current() -> [u32; 3] {
    parse_version(env!("CARGO_PKG_VERSION")).expect("crate version is MAJOR.MINOR.PATCH")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        assert_eq!(parse_version("0.2.10"), Some([0, 2, 10]));
        assert_eq!(parse_version("v1.0.0"), Some([1, 0, 0]));
        assert_eq!(parse_version("1.0"), None);
        assert_eq!(parse_version("1.0.0-beta"), None);
        assert_eq!(parse_version("1.0.0.1"), None);
        assert_eq!(parse_version("+1.0.0"), None);
        assert!([0, 10, 0] > [0, 9, 9]);
        assert_eq!(version_string([1, 2, 3]), "1.2.3");
    }

    #[test]
    fn manifests() {
        let hash = "00ff".repeat(16);
        let m = parse_manifest(&format!("0.3.1\r\n{hash}\r\n")).unwrap();
        assert_eq!(m.version, [0, 3, 1]);
        assert_eq!(m.sha256[1], 0xff);
        assert!(parse_manifest("0.3.1").is_none());
        assert!(parse_manifest(&format!("0.3.1\n{}", &hash[..62])).is_none());
        assert!(parse_manifest(&format!("0.3.1\n{hash}\nextra")).is_none());
        assert!(parse_manifest(&format!("0.3.1\n{}", "zz".repeat(32))).is_none());
        assert!(parse_manifest("<html>Not Found</html>").is_none());
    }
}
