//! Symbolic icons drawn on a 16×16 grid, after the Adwaita icon set, plus a
//! minimal SVG path-data parser (M L H V C Q A Z, absolute and relative).

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Icon {
    GoPrevious,
    GoNext,
    PanDown,
    OpenMenu,
    WindowClose,
    WindowMinimize,
    WindowMaximize,
    WindowRestore,
    Folder,
    ZoomIn,
    ZoomOut,
    Fullscreen,
    LeaveFullscreen,
    Info,
    Edit,
    Bookmark,
    BookmarkFilled,
    RotateLeft,
    RotateRight,
    FlipHorizontal,
    FlipVertical,
    Check,
    Remove,
    Image,
    ImageMissing,
    Windmill,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Paint {
    Fill,
    Stroke(f32),
}

impl Icon {
    pub fn parts(self) -> &'static [(&'static str, Paint)] {
        use Paint::{Fill, Stroke};
        const CIRCLE_6: &str = "M14 8A6 6 0 1 1 2 8A6 6 0 1 1 14 8Z";
        const LENS: &str = "M11 6.5A4.5 4.5 0 1 1 2 6.5A4.5 4.5 0 1 1 11 6.5ZM10 10L14 14";
        const FRAME: &str = "M3 2H13A1 1 0 0 1 14 3V13A1 1 0 0 1 13 14H3A1 1 0 0 1 2 13V3A1 1 0 0 1 3 2Z";
        const BOOKMARK: &str = "M4 2H12V14L8 11L4 14Z";
        match self {
            Self::GoPrevious => &[("M10 3L5 8L10 13", Stroke(2.0))],
            Self::GoNext => &[("M6 3L11 8L6 13", Stroke(2.0))],
            Self::PanDown => &[("M4 6L8 10L12 6", Stroke(2.0))],
            Self::OpenMenu => &[("M2 3H14M2 8H14M2 13H14", Stroke(2.0))],
            Self::WindowClose => &[("M4.5 4.5L11.5 11.5M11.5 4.5L4.5 11.5", Stroke(1.5))],
            Self::WindowMinimize => &[("M4.5 11H11.5", Stroke(1.5))],
            Self::WindowMaximize => &[("M4.5 4.5H11.5V11.5H4.5Z", Stroke(1.5))],
            Self::WindowRestore => &[("M6.5 4.5H11.5V9.5M4.5 6.5H9.5V11.5H4.5Z", Stroke(1.5))],
            Self::Folder => &[(
                "M2 4V12A1 1 0 0 0 3 13H13A1 1 0 0 0 14 12V6A1 1 0 0 0 13 5H8L6.5 3H3A1 1 0 0 0 2 4Z",
                Stroke(2.0),
            )],
            Self::ZoomIn => &[(LENS, Stroke(2.0)), ("M6.5 4.5V8.5M4.5 6.5H8.5", Stroke(1.5))],
            Self::ZoomOut => &[(LENS, Stroke(2.0)), ("M4.5 6.5H8.5", Stroke(1.5))],
            Self::Fullscreen => &[("M2 6V2H6M10 2H14V6M14 10V14H10M6 14H2V10", Stroke(2.0))],
            Self::LeaveFullscreen => &[("M6 2V6H2M10 2V6H14M14 10H10V14M2 10H6V14", Stroke(2.0))],
            Self::Info => &[
                (CIRCLE_6, Stroke(1.5)),
                ("M8 7.5V11.5", Stroke(1.75)),
                ("M9 5A1 1 0 1 1 7 5A1 1 0 1 1 9 5Z", Fill),
            ],
            Self::Edit => &[("M10 3L13 6L6 13H3V10ZM8.5 4.5L11.5 7.5", Stroke(1.75))],
            Self::Bookmark => &[(BOOKMARK, Stroke(2.0))],
            Self::BookmarkFilled => &[(BOOKMARK, Stroke(2.0)), (BOOKMARK, Fill)],
            Self::RotateLeft => &[("M8.5 3.5A5 5 0 1 1 3.5 8.5M10.5 1.5L8.5 3.5L10.5 5.5", Stroke(1.75))],
            Self::RotateRight => &[("M7.5 3.5A5 5 0 1 0 12.5 8.5M5.5 1.5L7.5 3.5L5.5 5.5", Stroke(1.75))],
            Self::FlipHorizontal => &[("M7 3L2 13H7Z", Fill), ("M9.75 3.5L14 12.25H9.75Z", Stroke(1.5))],
            Self::FlipVertical => &[("M3 7L13 2V7Z", Fill), ("M3.5 9.75L12.25 14V9.75Z", Stroke(1.5))],
            Self::Check => &[("M3 8.5L6.5 12L13 4.5", Stroke(2.0))],
            Self::Remove => &[("M4 4L12 12M12 4L4 12", Stroke(2.0))],
            Self::Image => &[
                (FRAME, Stroke(1.5)),
                ("M2.5 12L6 8L9 11L11 9L13.5 11.5", Stroke(1.5)),
                ("M11.5 5.5A1.5 1.5 0 1 1 8.5 5.5A1.5 1.5 0 1 1 11.5 5.5Z", Fill),
            ],
            Self::Windmill => &[
                ("M6.25 15L7.25 8.5H8.75L9.75 15Z", Fill),
                ("M3 2L13 12M13 2L3 12", Stroke(1.25)),
                ("M3.5 2.5L6.25 5.25M12.5 2.5L9.75 5.25M3.5 11.5L6.25 8.75M12.5 11.5L9.75 8.75", Stroke(3.0)),
            ],
            Self::ImageMissing => &[(FRAME, Stroke(1.5)), ("M5.5 5.5L10.5 10.5M10.5 5.5L5.5 10.5", Stroke(1.5))],
        }
    }
}

/// A path segment in absolute coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Seg {
    Move(f32, f32),
    Line(f32, f32),
    Cubic(f32, f32, f32, f32, f32, f32),
    Quad(f32, f32, f32, f32),
    Close,
}

/// Appends an SVG elliptical arc as cubic Béziers (SVG 1.1, appendix F.6),
/// so every renderer draws identical shapes.
#[allow(clippy::too_many_arguments)]
pub fn arc_to_cubics(out: &mut Vec<Seg>, from: (f32, f32), rx: f32, ry: f32, rot_deg: f32, large: bool, sweep: bool, to: (f32, f32)) {
    let (x1, y1) = (f64::from(from.0), f64::from(from.1));
    let (x2, y2) = (f64::from(to.0), f64::from(to.1));
    let (mut rx, mut ry) = (f64::from(rx).abs(), f64::from(ry).abs());
    if (x1 - x2).abs() < 1e-9 && (y1 - y2).abs() < 1e-9 {
        return;
    }
    if rx < 1e-9 || ry < 1e-9 {
        out.push(Seg::Line(to.0, to.1));
        return;
    }
    let phi = f64::from(rot_deg).to_radians();
    let (sin, cos) = phi.sin_cos();
    let (dx, dy) = ((x1 - x2) / 2.0, (y1 - y2) / 2.0);
    let (x1p, y1p) = (cos * dx + sin * dy, -sin * dx + cos * dy);
    let lambda = (x1p * x1p) / (rx * rx) + (y1p * y1p) / (ry * ry);
    if lambda > 1.0 {
        rx *= lambda.sqrt();
        ry *= lambda.sqrt();
    }
    let num = rx * rx * ry * ry - rx * rx * y1p * y1p - ry * ry * x1p * x1p;
    let den = rx * rx * y1p * y1p + ry * ry * x1p * x1p;
    let mut k = (num / den).max(0.0).sqrt();
    if large == sweep {
        k = -k;
    }
    let (cxp, cyp) = (k * rx * y1p / ry, -k * ry * x1p / rx);
    let (cx, cy) = (cos * cxp - sin * cyp + (x1 + x2) / 2.0, sin * cxp + cos * cyp + (y1 + y2) / 2.0);
    let angle = |ux: f64, uy: f64, vx: f64, vy: f64| (ux * vy - uy * vx).atan2(ux * vx + uy * vy);
    let (ux, uy) = ((x1p - cxp) / rx, (y1p - cyp) / ry);
    let (vx, vy) = ((-x1p - cxp) / rx, (-y1p - cyp) / ry);
    let theta1 = angle(1.0, 0.0, ux, uy);
    let mut delta = angle(ux, uy, vx, vy);
    if !sweep && delta > 0.0 {
        delta -= std::f64::consts::TAU;
    } else if sweep && delta < 0.0 {
        delta += std::f64::consts::TAU;
    }
    let n = (delta.abs() / std::f64::consts::FRAC_PI_2).ceil().max(1.0) as usize;
    let step = delta / n as f64;
    let t = 4.0 / 3.0 * (step / 4.0).tan();
    let point = |a: f64| {
        let (s, c) = a.sin_cos();
        (cx + rx * c * cos - ry * s * sin, cy + rx * c * sin + ry * s * cos)
    };
    let deriv = |a: f64| {
        let (s, c) = a.sin_cos();
        (-rx * s * cos - ry * c * sin, -rx * s * sin + ry * c * cos)
    };
    let mut a = theta1;
    for i in 0..n {
        let b = a + step;
        let (p0, p3) = (point(a), if i + 1 == n { (x2, y2) } else { point(b) });
        let (d0, d3) = (deriv(a), deriv(b));
        out.push(Seg::Cubic(
            (p0.0 + t * d0.0) as f32,
            (p0.1 + t * d0.1) as f32,
            (p3.0 - t * d3.0) as f32,
            (p3.1 - t * d3.1) as f32,
            p3.0 as f32,
            p3.1 as f32,
        ));
        a = b;
    }
}

/// Parses SVG path data. Returns `None` on malformed input.
pub fn parse_path(d: &str) -> Option<Vec<Seg>> {
    let mut out = Vec::new();
    let b = d.as_bytes();
    let mut i = 0;
    let (mut cx, mut cy, mut sx, mut sy) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
    let mut cmd = 0u8;

    let skip = |i: &mut usize| {
        while *i < b.len() && (b[*i].is_ascii_whitespace() || b[*i] == b',') {
            *i += 1;
        }
    };
    let num = |i: &mut usize| -> Option<f32> {
        skip(i);
        let start = *i;
        if *i < b.len() && (b[*i] == b'-' || b[*i] == b'+') {
            *i += 1;
        }
        let mut dot = false;
        while *i < b.len() && (b[*i].is_ascii_digit() || (b[*i] == b'.' && !dot)) {
            dot |= b[*i] == b'.';
            *i += 1;
        }
        if *i < b.len() && (b[*i] == b'e' || b[*i] == b'E') {
            *i += 1;
            if *i < b.len() && (b[*i] == b'-' || b[*i] == b'+') {
                *i += 1;
            }
            while *i < b.len() && b[*i].is_ascii_digit() {
                *i += 1;
            }
        }
        d.get(start..*i)?.parse().ok()
    };
    let flag = |i: &mut usize| -> Option<bool> {
        skip(i);
        let f = match b.get(*i)? {
            b'0' => false,
            b'1' => true,
            _ => return None,
        };
        *i += 1;
        Some(f)
    };

    loop {
        skip(&mut i);
        if i >= b.len() {
            break;
        }
        if b[i].is_ascii_alphabetic() {
            cmd = b[i];
            i += 1;
        } else if cmd == 0 {
            return None;
        }
        let rel = cmd.is_ascii_lowercase();
        let (ox, oy) = if rel { (cx, cy) } else { (0.0, 0.0) };
        match cmd.to_ascii_uppercase() {
            b'M' => {
                (cx, cy) = (ox + num(&mut i)?, oy + num(&mut i)?);
                (sx, sy) = (cx, cy);
                out.push(Seg::Move(cx, cy));
                // Further coordinate pairs are implicit line-tos.
                cmd = if rel { b'l' } else { b'L' };
            }
            b'L' => {
                (cx, cy) = (ox + num(&mut i)?, oy + num(&mut i)?);
                out.push(Seg::Line(cx, cy));
            }
            b'H' => {
                cx = ox + num(&mut i)?;
                out.push(Seg::Line(cx, cy));
            }
            b'V' => {
                cy = oy + num(&mut i)?;
                out.push(Seg::Line(cx, cy));
            }
            b'C' => {
                let (x1, y1) = (ox + num(&mut i)?, oy + num(&mut i)?);
                let (x2, y2) = (ox + num(&mut i)?, oy + num(&mut i)?);
                (cx, cy) = (ox + num(&mut i)?, oy + num(&mut i)?);
                out.push(Seg::Cubic(x1, y1, x2, y2, cx, cy));
            }
            b'Q' => {
                let (x1, y1) = (ox + num(&mut i)?, oy + num(&mut i)?);
                (cx, cy) = (ox + num(&mut i)?, oy + num(&mut i)?);
                out.push(Seg::Quad(x1, y1, cx, cy));
            }
            b'A' => {
                let (rx, ry, rot) = (num(&mut i)?, num(&mut i)?, num(&mut i)?);
                let (large, sweep) = (flag(&mut i)?, flag(&mut i)?);
                let (x, y) = (ox + num(&mut i)?, oy + num(&mut i)?);
                arc_to_cubics(&mut out, (cx, cy), rx, ry, rot, large, sweep, (x, y));
                (cx, cy) = (x, y);
            }
            b'Z' => {
                (cx, cy) = (sx, sy);
                out.push(Seg::Close);
                cmd = 0;
            }
            _ => return None,
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_icons() {
        let all = [
            Icon::GoPrevious,
            Icon::GoNext,
            Icon::PanDown,
            Icon::OpenMenu,
            Icon::WindowClose,
            Icon::WindowMinimize,
            Icon::WindowMaximize,
            Icon::WindowRestore,
            Icon::Folder,
            Icon::ZoomIn,
            Icon::ZoomOut,
            Icon::Fullscreen,
            Icon::LeaveFullscreen,
            Icon::Info,
            Icon::Edit,
            Icon::Bookmark,
            Icon::BookmarkFilled,
            Icon::RotateLeft,
            Icon::RotateRight,
            Icon::FlipHorizontal,
            Icon::FlipVertical,
            Icon::Check,
            Icon::Remove,
            Icon::Image,
            Icon::ImageMissing,
            Icon::Windmill,
        ];
        for icon in all {
            for (d, _) in icon.parts() {
                let segs = parse_path(d).unwrap_or_else(|| panic!("{icon:?}"));
                assert!(matches!(segs[0], Seg::Move(..)));
            }
        }
    }

    #[test]
    fn arcs_become_cubics_on_the_circle() {
        let segs = parse_path("M14 8A6 6 0 1 1 2 8A6 6 0 1 1 14 8Z").unwrap();
        let cubics: Vec<_> = segs.iter().filter(|s| matches!(s, Seg::Cubic(..))).collect();
        assert_eq!(cubics.len(), 4);
        for s in cubics {
            if let Seg::Cubic(_, _, _, _, x, y) = *s {
                assert!(((x - 8.0).hypot(y - 8.0) - 6.0).abs() < 1e-3);
            }
        }
        // A quarter arc ends exactly at its end point.
        let mut v = Vec::new();
        arc_to_cubics(&mut v, (10.0, 0.0), 10.0, 10.0, 0.0, false, true, (0.0, 10.0));
        assert_eq!(v.len(), 1);
        assert!(matches!(v[0], Seg::Cubic(_, _, _, _, x, y) if x == 0.0 && y == 10.0));
    }

    #[test]
    fn relative_and_implicit() {
        let s = parse_path("m1 1 2 0h1v1z").unwrap();
        assert_eq!(s, vec![Seg::Move(1.0, 1.0), Seg::Line(3.0, 1.0), Seg::Line(4.0, 1.0), Seg::Line(4.0, 2.0), Seg::Close]);
        assert!(parse_path("1 2").is_none());
        assert!(parse_path("M1 2 A1 1 0 2 0 3 3").is_none());
    }
}
