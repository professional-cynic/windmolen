//! Pure editing model: orientation (the dihedral group D4) and cropping.
//!
//! Kept free of any platform code so it can be unit-tested anywhere.

use crate::geom::{Affine, Rect};

/// A lossless orientation: flip horizontally first (if `flip`), then rotate
/// `rot` quarter turns clockwise.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Orient {
    pub rot: u8,
    pub flip: bool,
}

impl Orient {
    pub const IDENTITY: Self = Self { rot: 0, flip: false };
    const CW: Self = Self { rot: 1, flip: false };
    const CCW: Self = Self { rot: 3, flip: false };
    const FLIP_H: Self = Self { rot: 0, flip: true };
    const FLIP_V: Self = Self { rot: 2, flip: true };

    /// Maps an EXIF `Orientation` tag value (1–8) to the transform that
    /// displays the stored pixels upright.
    pub fn from_exif(v: u16) -> Self {
        match v {
            2 => Self { rot: 0, flip: true },
            3 => Self { rot: 2, flip: false },
            4 => Self { rot: 2, flip: true },
            5 => Self { rot: 3, flip: true },
            6 => Self { rot: 1, flip: false },
            7 => Self { rot: 1, flip: true },
            8 => Self { rot: 3, flip: false },
            _ => Self::IDENTITY,
        }
    }

    pub fn to_exif(self) -> u16 {
        (1..=8).find(|&v| Self::from_exif(v) == self).unwrap_or(1)
    }

    /// `self` followed by `next`.
    pub fn then(self, next: Self) -> Self {
        // next ∘ self = R^b F^g R^a F^f, and F R^a = R^-a F.
        if next.flip {
            Self { rot: (next.rot + 4 - self.rot) % 4, flip: !self.flip }
        } else {
            Self { rot: (next.rot + self.rot) % 4, flip: self.flip }
        }
    }

    pub fn rotate_cw(self) -> Self {
        self.then(Self::CW)
    }

    pub fn rotate_ccw(self) -> Self {
        self.then(Self::CCW)
    }

    pub fn flip_h(self) -> Self {
        self.then(Self::FLIP_H)
    }

    pub fn flip_v(self) -> Self {
        self.then(Self::FLIP_V)
    }

    pub fn swaps_axes(self) -> bool {
        self.rot % 2 == 1
    }

    /// Size of a `w`×`h` image after this transform.
    pub fn apply_size<T>(self, w: T, h: T) -> (T, T) {
        if self.swaps_axes() { (h, w) } else { (w, h) }
    }

    /// Matrix mapping source pixel coordinates to oriented pixel coordinates.
    pub fn matrix(self, w: f32, h: f32) -> Affine {
        let (ow, oh) = self.apply_size(w, h);
        let mut m = Affine::translate(-w / 2.0, -h / 2.0);
        if self.flip {
            m = m.then(Affine::scale(-1.0, 1.0));
        }
        let quarter = Affine { m11: 0.0, m12: 1.0, m21: -1.0, m22: 0.0, dx: 0.0, dy: 0.0 };
        for _ in 0..self.rot {
            m = m.then(quarter);
        }
        m.then(Affine::translate(ow / 2.0, oh / 2.0))
    }

    /// Maps a point in normalised coordinates (0‥1) through this transform.
    pub fn map_unit(self, mut u: f32, mut v: f32) -> (f32, f32) {
        if self.flip {
            u = 1.0 - u;
        }
        for _ in 0..self.rot {
            (u, v) = (1.0 - v, u);
        }
        (u, v)
    }

    /// Maps a normalised rectangle through this transform.
    pub fn map_unit_rect(self, r: Rect) -> Rect {
        let (x0, y0) = self.map_unit(r.x, r.y);
        let (x1, y1) = self.map_unit(r.right(), r.bottom());
        Rect::from_edges(x0.min(x1), y0.min(y1), x0.max(x1), y0.max(y1))
    }
}

/// Crop aspect-ratio constraint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Aspect {
    Free,
    Original,
    Square,
    Ratio(u8, u8),
}

impl Aspect {
    pub const ALL: [Self; 9] = [
        Self::Free,
        Self::Original,
        Self::Square,
        Self::Ratio(4, 3),
        Self::Ratio(3, 4),
        Self::Ratio(3, 2),
        Self::Ratio(2, 3),
        Self::Ratio(16, 9),
        Self::Ratio(9, 16),
    ];

    pub fn label(self) -> String {
        match self {
            Self::Free => "Free".into(),
            Self::Original => "Original".into(),
            Self::Square => "Square".into(),
            Self::Ratio(a, b) => format!("{a}:{b}"),
        }
    }

    /// Width / height in pixels for an image of the given (oriented) size.
    pub fn ratio(self, w: f32, h: f32) -> Option<f32> {
        match self {
            Self::Free => None,
            Self::Original => Some(w / h),
            Self::Square => Some(1.0),
            Self::Ratio(a, b) => Some(f32::from(a) / f32::from(b)),
        }
    }

    /// The same constraint after a quarter turn.
    pub fn turned(self) -> Self {
        match self {
            Self::Ratio(a, b) => Self::Ratio(b, a),
            other => other,
        }
    }
}

/// Which part of the crop rectangle is being dragged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handle {
    Move,
    N,
    S,
    E,
    W,
    NE,
    NW,
    SE,
    SW,
}

impl Handle {
    fn sides(self) -> (i8, i8) {
        match self {
            Self::Move => (0, 0),
            Self::N => (0, -1),
            Self::S => (0, 1),
            Self::E => (1, 0),
            Self::W => (-1, 0),
            Self::NE => (1, -1),
            Self::NW => (-1, -1),
            Self::SE => (1, 1),
            Self::SW => (-1, 1),
        }
    }

    pub fn is_corner(self) -> bool {
        let (x, y) = self.sides();
        x != 0 && y != 0
    }
}

/// Smallest crop, in image pixels.
const MIN_CROP: f32 = 16.0;

/// Largest centred rectangle of `ratio` inside a `w`×`h` image.
pub fn fit_aspect(ratio: f32, w: f32, h: f32) -> Rect {
    let (cw, ch) = if w / h > ratio { (h * ratio, h) } else { (w, w / ratio) };
    Rect::new((w - cw) / 2.0, (h - ch) / 2.0, cw, ch)
}

/// Applies a drag of (`dx`, `dy`) image pixels to `start` (image pixels) in a
/// `w`×`h` image, keeping `ratio` for corner drags when given.
pub fn drag_crop(handle: Handle, start: Rect, dx: f32, dy: f32, w: f32, h: f32, ratio: Option<f32>) -> Rect {
    let m = MIN_CROP.min(w).min(h);
    let (l, t, r, b) = (start.x, start.y, start.right(), start.bottom());

    if handle == Handle::Move {
        let x = (l + dx).clamp(0.0, w - start.w);
        let y = (t + dy).clamp(0.0, h - start.h);
        return Rect::new(x, y, start.w, start.h);
    }

    let (sx, sy) = handle.sides();
    match ratio {
        Some(ratio) if handle.is_corner() => {
            // The opposite corner stays put.
            let ax = if sx > 0 { l } else { r };
            let ay = if sy > 0 { t } else { b };
            let px = if sx > 0 { r + dx } else { l + dx };
            let py = if sy > 0 { b + dy } else { t + dy };
            let mut cw = (f32::from(sx) * (px - ax)).max(m);
            let mut ch = (f32::from(sy) * (py - ay)).max(m);
            if cw / ch > ratio { cw = ch * ratio } else { ch = cw / ratio }
            let max_w = if sx > 0 { w - ax } else { ax };
            let max_h = if sy > 0 { h - ay } else { ay };
            if cw > max_w {
                cw = max_w;
                ch = cw / ratio;
            }
            if ch > max_h {
                ch = max_h;
                cw = ch * ratio;
            }
            let (cw, ch) = (cw.max(m.min(max_w)), ch.max(m.min(max_h)));
            let x = if sx > 0 { ax } else { ax - cw };
            let y = if sy > 0 { ay } else { ay - ch };
            Rect::new(x, y, cw, ch)
        }
        _ => {
            let (mut l, mut t, mut r, mut b) = (l, t, r, b);
            match sx {
                -1 => l = (l + dx).clamp(0.0, r - m),
                1 => r = (r + dx).clamp(l + m, w),
                _ => {}
            }
            match sy {
                -1 => t = (t + dy).clamp(0.0, b - m),
                1 => b = (b + dy).clamp(t + m, h),
                _ => {}
            }
            Rect::from_edges(l, t, r, b)
        }
    }
}

/// The full-image crop in normalised coordinates.
pub const FULL: Rect = Rect::new(0.0, 0.0, 1.0, 1.0);

pub fn is_full(r: Rect) -> bool {
    const E: f32 = 1e-4;
    r.x <= E && r.y <= E && r.right() >= 1.0 - E && r.bottom() >= 1.0 - E
}

/// Converts a normalised crop to whole pixels, `None` when nothing is cut.
pub fn crop_pixels(r: Rect, w: u32, h: u32) -> Option<(u32, u32, u32, u32)> {
    if is_full(r) {
        return None;
    }
    let px = |v: f32, max: u32| ((v * max as f32).round().max(0.0) as u32).min(max);
    let (l, t) = (px(r.x, w), px(r.y, h));
    let (rr, bb) = (px(r.right(), w).max(l + 1).min(w), px(r.bottom(), h).max(t + 1).min(h));
    let l = l.min(rr - 1);
    let t = t.min(bb - 1);
    if l == 0 && t == 0 && rr == w && bb == h {
        return None;
    }
    Some((l, t, rr - l, bb - t))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all() -> Vec<Orient> {
        (0..8).map(|i| Orient { rot: i % 4, flip: i >= 4 }).collect()
    }

    #[test]
    fn exif_round_trip() {
        for v in 1..=8 {
            assert_eq!(Orient::from_exif(v).to_exif(), v);
        }
    }

    #[test]
    fn composition_matches_point_mapping() {
        let pts = [(0.1, 0.2), (0.7, 0.3), (0.0, 1.0)];
        for a in all() {
            for b in all() {
                let ab = a.then(b);
                for &(u, v) in &pts {
                    let (u1, v1) = a.map_unit(u, v);
                    let direct = b.map_unit(u1, v1);
                    let composed = ab.map_unit(u, v);
                    assert!((direct.0 - composed.0).abs() < 1e-6 && (direct.1 - composed.1).abs() < 1e-6);
                }
            }
        }
    }

    #[test]
    fn rotations_and_flips() {
        let o = Orient::IDENTITY;
        assert_eq!(o.rotate_cw().rotate_cw().rotate_cw().rotate_cw(), o);
        assert_eq!(o.rotate_cw().rotate_ccw(), o);
        assert_eq!(o.flip_h().flip_h(), o);
        assert_eq!(o.flip_h().flip_v(), Orient { rot: 2, flip: false });
        // Top-left goes to top-right on a clockwise turn.
        assert_eq!(o.rotate_cw().map_unit(0.0, 0.0), (1.0, 0.0));
        // EXIF 6 means "rotate 90° clockwise to display".
        assert_eq!(Orient::from_exif(6).map_unit(0.0, 0.0), (1.0, 0.0));
        // EXIF 5 is a transpose.
        assert_eq!(Orient::from_exif(5).map_unit(0.25, 0.75), (0.75, 0.25));
    }

    #[test]
    fn matrix_matches_unit_mapping() {
        let (w, h) = (40.0, 20.0);
        for o in all() {
            let m = o.matrix(w, h);
            let (ow, oh) = o.apply_size(w, h);
            let (x, y) = (10.0, 5.0);
            let px = x * m.m11 + y * m.m21 + m.dx;
            let py = x * m.m12 + y * m.m22 + m.dy;
            let (u, v) = o.map_unit(x / w, y / h);
            assert!((px / ow - u).abs() < 1e-5 && (py / oh - v).abs() < 1e-5, "{o:?}");
        }
    }

    /// `IWICBitmapFlipRotator` as Wine implements it (verified against
    /// Windows): the source pixel shown at destination pixel (x, y) for
    /// `WICBitmapTransformRotate(rot * 90) | FlipHorizontal(flip)`.
    fn wic_source_pixel(rot: u8, flip: bool, (w, h): (u32, u32), (x, y): (u32, u32)) -> (u32, u32) {
        let (swap, mut flip_x, mut flip_y) = [(false, false, false), (true, true, false), (false, true, true), (true, false, true)][usize::from(rot)];
        if flip {
            if swap { flip_y = !flip_y } else { flip_x = !flip_x }
        }
        let (dw, dh) = if swap { (h, w) } else { (w, h) };
        let sx = if flip_x { dw - 1 - x } else { x };
        let sy = if flip_y { dh - 1 - y } else { y };
        if swap { (sy, sx) } else { (sx, sy) }
    }

    #[test]
    fn wic_flip_rotator() {
        // Saving passes `rot` and `flip` to WIC unchanged, so WIC must place
        // every pixel where `Orient` (and the preview) does.
        let (w, h) = (4u32, 3u32);
        for o in all() {
            let (dw, dh) = o.apply_size(w, h);
            for (sx, sy) in (0..h).flat_map(|y| (0..w).map(move |x| (x, y))) {
                let (u, v) = o.map_unit((sx as f32 + 0.5) / w as f32, (sy as f32 + 0.5) / h as f32);
                let (dx, dy) = ((u * dw as f32 - 0.5).round() as u32, (v * dh as f32 - 0.5).round() as u32);
                assert_eq!(wic_source_pixel(o.rot, o.flip, (w, h), (dx, dy)), (sx, sy), "{o:?}");
            }
        }
    }

    #[test]
    fn crop_drag_stays_inside() {
        let start = Rect::new(10.0, 10.0, 50.0, 50.0);
        let r = drag_crop(Handle::Move, start, -100.0, 500.0, 100.0, 100.0, None);
        assert_eq!(r, Rect::new(0.0, 50.0, 50.0, 50.0));
        let r = drag_crop(Handle::SE, start, 500.0, 10.0, 100.0, 100.0, Some(1.0));
        assert!((r.w - r.h).abs() < 1e-4 && r.right() <= 100.0 && r.bottom() <= 100.0);
        let r = drag_crop(Handle::W, start, 100.0, 0.0, 100.0, 100.0, None);
        assert!(r.w >= MIN_CROP);
    }

    #[test]
    fn pixel_crop() {
        assert_eq!(crop_pixels(FULL, 100, 50), None);
        assert_eq!(crop_pixels(Rect::new(0.1, 0.2, 0.5, 0.5), 100, 50), Some((10, 10, 50, 25)));
    }
}
