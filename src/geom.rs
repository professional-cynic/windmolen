// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 professional-cynic

//! Small geometry primitives in device-independent pixels (DIPs).

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    pub fn from_edges(l: f32, t: f32, r: f32, b: f32) -> Self {
        Self::new(l, t, r - l, b - t)
    }

    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }

    pub fn cx(&self) -> f32 {
        self.x + self.w / 2.0
    }

    pub fn cy(&self) -> f32 {
        self.y + self.h / 2.0
    }

    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.right() && y >= self.y && y < self.bottom()
    }

    pub fn inset(&self, d: f32) -> Self {
        Self::new(self.x + d, self.y + d, (self.w - 2.0 * d).max(0.0), (self.h - 2.0 * d).max(0.0))
    }

    /// A `w`×`h` rectangle centred inside `self`.
    pub fn centred(&self, w: f32, h: f32) -> Self {
        Self::new(self.cx() - w / 2.0, self.cy() - h / 2.0, w, h)
    }

    pub fn round(&self) -> Self {
        Self::from_edges(self.x.round(), self.y.round(), self.right().round(), self.bottom().round())
    }
}

/// Row-major 2×3 affine matrix, laid out like `D2D_MATRIX_3X2_F`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Affine {
    pub m11: f32,
    pub m12: f32,
    pub m21: f32,
    pub m22: f32,
    pub dx: f32,
    pub dy: f32,
}

impl Affine {
    pub const IDENTITY: Self = Self { m11: 1.0, m12: 0.0, m21: 0.0, m22: 1.0, dx: 0.0, dy: 0.0 };

    pub const fn translate(x: f32, y: f32) -> Self {
        Self { dx: x, dy: y, ..Self::IDENTITY }
    }

    pub const fn scale(sx: f32, sy: f32) -> Self {
        Self { m11: sx, m22: sy, ..Self::IDENTITY }
    }

    /// `self` followed by `next` (row-vector convention, as Direct2D).
    pub fn then(self, n: Self) -> Self {
        Self {
            m11: self.m11 * n.m11 + self.m12 * n.m21,
            m12: self.m11 * n.m12 + self.m12 * n.m22,
            m21: self.m21 * n.m11 + self.m22 * n.m21,
            m22: self.m21 * n.m12 + self.m22 * n.m22,
            dx: self.dx * n.m11 + self.dy * n.m21 + n.dx,
            dy: self.dx * n.m12 + self.dy * n.m22 + n.dy,
        }
    }
}

/// Size of a `w`×`h` image scaled down so that neither edge exceeds `edge`
/// (aspect kept, each edge at least 1); unchanged when it already fits.
pub fn fit_edge(w: u32, h: u32, edge: u32) -> (u32, u32) {
    if w <= edge && h <= edge {
        return (w, h);
    }
    let s = f64::from(edge) / f64::from(w.max(h));
    let scale = |v: u32| ((f64::from(v) * s).round() as u32).clamp(1, edge);
    (scale(w), scale(h))
}

#[cfg(test)]
mod tests {
    use super::fit_edge;

    #[test]
    fn edges() {
        assert_eq!(fit_edge(6000, 4000, 1920), (1920, 1280));
        assert_eq!(fit_edge(4000, 6000, 1920), (1280, 1920));
        assert_eq!(fit_edge(800, 600, 1920), (800, 600));
        assert_eq!(fit_edge(100_000, 3, 1000), (1000, 1));
    }
}
