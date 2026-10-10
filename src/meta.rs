// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 professional-cynic

//! Platform-independent formatting of image metadata.

/// A metadata value as read from the container.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Int(i64),
    Float(f64),
    Rational(i64, i64),
    Text(String),
}

impl Value {
    pub fn as_f64(&self) -> Option<f64> {
        match *self {
            Self::Int(v) => Some(v as f64),
            Self::Float(v) => Some(v),
            Self::Rational(n, d) if d != 0 => Some(n as f64 / d as f64),
            _ => None,
        }
    }

    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text(s) => Some(s.trim_matches(|c: char| c.is_whitespace() || c == '\0')).filter(|s| !s.is_empty()),
            _ => None,
        }
    }
}

pub fn exposure(v: &Value) -> Option<String> {
    let t = v.as_f64().filter(|t| *t > 0.0 && t.is_finite())?;
    Some(if t >= 0.5 { format!("{} s", trim(t, 1)) } else { format!("1/{} s", (1.0 / t).round()) })
}

pub fn aperture(v: &Value) -> Option<String> {
    v.as_f64().filter(|f| *f > 0.0 && f.is_finite()).map(|f| format!("ƒ/{}", trim(f, 1)))
}

pub fn focal_length(v: &Value) -> Option<String> {
    v.as_f64().filter(|f| *f > 0.0 && f.is_finite()).map(|f| format!("{} mm", trim(f, 1)))
}

pub fn iso(v: &Value) -> Option<String> {
    v.as_f64().filter(|i| *i > 0.0 && i.is_finite()).map(|i| format!("ISO {}", i.round()))
}

/// Joins maker and model, dropping a maker the model already repeats
/// ("Canon" + "Canon EOS R6" → "Canon EOS R6").
pub fn camera(make: Option<&str>, model: Option<&str>) -> Option<String> {
    match (make, model) {
        (Some(a), Some(b)) if b.to_lowercase().starts_with(&a.to_lowercase()) => Some(b.into()),
        (Some(a), Some(b)) => Some(format!("{a} {b}")),
        (a, b) => a.or(b).map(Into::into),
    }
}

/// Parses an EXIF date ("YYYY:MM:DD HH:MM:SS") into its numeric fields.
pub fn exif_date(s: &str) -> Option<[u16; 6]> {
    let b = s.trim().as_bytes();
    if b.len() < 19 {
        return None;
    }
    let n = |r: std::ops::Range<usize>| -> Option<u16> { std::str::from_utf8(&b[r]).ok()?.parse().ok() };
    let d = [n(0..4)?, n(5..7)?, n(8..10)?, n(11..13)?, n(14..16)?, n(17..19)?];
    ((1..=12).contains(&d[1]) && (1..=31).contains(&d[2]) && d[3] < 24 && d[4] < 60 && d[5] < 61).then_some(d)
}

/// SI file size, as GNOME's `g_format_size`.
pub fn file_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["kB", "MB", "GB", "TB"];
    if bytes < 1000 {
        return format!("{bytes} bytes");
    }
    let mut v = bytes as f64 / 1000.0;
    let mut u = 0;
    while v >= 1000.0 && u < UNITS.len() - 1 {
        v /= 1000.0;
        u += 1;
    }
    format!("{v:.1} {}", UNITS[u])
}

fn trim(v: f64, decimals: usize) -> String {
    let s = format!("{v:.decimals$}");
    if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').into() } else { s }
}

/// Reads the profile description from ICC data (v2 `desc` or v4 `mluc`).
pub fn icc_description(icc: &[u8]) -> Option<String> {
    let be32 = |o: usize| -> Option<u32> { Some(u32::from_be_bytes(icc.get(o..o + 4)?.try_into().ok()?)) };
    let count = be32(128)? as usize;
    for i in 0..count.min(256) {
        let e = 132 + i * 12;
        if icc.get(e..e + 4)? != b"desc" {
            continue;
        }
        let (off, len) = (be32(e + 4)? as usize, be32(e + 8)? as usize);
        let tag = icc.get(off..off.checked_add(len)?)?;
        let s = match tag.get(0..4)? {
            b"desc" => {
                let n = u32::from_be_bytes(tag.get(8..12)?.try_into().ok()?) as usize;
                let raw = tag.get(12..12usize.checked_add(n)?)?;
                String::from_utf8_lossy(raw.split(|&c| c == 0).next()?).into_owned()
            }
            b"mluc" => {
                let records = u32::from_be_bytes(tag.get(8..12)?.try_into().ok()?);
                if records == 0 {
                    return None;
                }
                let n = u32::from_be_bytes(tag.get(20..24)?.try_into().ok()?) as usize;
                let o = u32::from_be_bytes(tag.get(24..28)?.try_into().ok()?) as usize;
                let raw = tag.get(o..o.checked_add(n)?)?;
                let units: Vec<u16> = raw.chunks_exact(2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
                String::from_utf16_lossy(&units).trim_end_matches('\0').to_owned()
            }
            _ => return None,
        };
        let s = s.trim().to_owned();
        return (!s.is_empty()).then_some(s);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats() {
        assert_eq!(exposure(&Value::Rational(1, 250)).unwrap(), "1/250 s");
        assert_eq!(exposure(&Value::Rational(5, 2)).unwrap(), "2.5 s");
        assert_eq!(aperture(&Value::Rational(28, 10)).unwrap(), "ƒ/2.8");
        assert_eq!(aperture(&Value::Float(8.0)).unwrap(), "ƒ/8");
        assert_eq!(focal_length(&Value::Rational(50, 1)).unwrap(), "50 mm");
        assert_eq!(iso(&Value::Int(400)).unwrap(), "ISO 400");
        assert!(exposure(&Value::Rational(1, 0)).is_none());
        assert_eq!(camera(Some("Canon"), Some("Canon EOS R6")).unwrap(), "Canon EOS R6");
        assert_eq!(camera(Some("Apple"), Some("iPhone 15")).unwrap(), "Apple iPhone 15");
        assert_eq!(file_size(999), "999 bytes");
        assert_eq!(file_size(4_200_000), "4.2 MB");
    }

    #[test]
    fn dates() {
        assert_eq!(exif_date("2023:05:01 12:34:56"), Some([2023, 5, 1, 12, 34, 56]));
        assert_eq!(exif_date("0000:00:00 00:00:00"), None);
        assert_eq!(exif_date("short"), None);
    }

    #[test]
    fn icc() {
        // Minimal v2 profile with one `desc` tag.
        let mut p = vec![0u8; 128];
        p.extend(1u32.to_be_bytes());
        p.extend(b"desc");
        p.extend(144u32.to_be_bytes());
        let text = b"Display P3\0";
        let tag_len = 12 + text.len();
        p.extend((tag_len as u32).to_be_bytes());
        p.extend(b"desc\0\0\0\0");
        p.extend((text.len() as u32).to_be_bytes());
        p.extend(text);
        assert_eq!(icc_description(&p).unwrap(), "Display P3");
        assert_eq!(icc_description(&p[..140]), None);
        assert_eq!(icc_description(&[]), None);
    }
}
