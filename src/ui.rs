//! Immediate-mode UI: the view emits draw commands and hit-test widgets in
//! one pass, so painting and input always agree. Styling follows libadwaita.

use crate::edit::{Aspect, Handle};
use crate::geom::{Affine, Rect};
use crate::icons::Icon;
use crate::theme::{Color, DISABLED_OPACITY, Theme};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Swallows clicks (modal backdrops).
    None,
    /// Closes the open popover or dialog.
    Dismiss,
    Open,
    Prev,
    Next,
    ZoomIn,
    ZoomOut,
    ZoomFit,
    ZoomToggle,
    Properties,
    Fullscreen,
    MainMenu,
    MarksMenu,
    Edit,
    ToggleMark(u8),
    SendMarked,
    ManageMarks,
    Shortcuts,
    About,
    Minimize,
    Maximize,
    Close,
    EditCancel,
    EditSave,
    SaveMenu,
    ReplaceOriginal,
    RotateCcw,
    RotateCw,
    FlipH,
    FlipV,
    AspectMenu,
    SetAspect(Aspect),
    EditReset,
    Crop(Handle),
    PickMarkDir(u8),
    ClearMarkDir(u8),
    SendCopy,
    SendMove,
    ConfirmReplace,
    Install,
    ConfirmInstall,
    DefaultApps,
    CheckUpdates,
    ToggleAutoUpdate,
    Restart,
    OpenProfile,
    OpenProject,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TextStyle {
    Body,
    Heading,
    Caption,
    Title1,
    Title2,
    Numeric,
}

impl TextStyle {
    /// (size in DIPs, weight) after the libadwaita type scale.
    pub fn metrics(self) -> (f32, u16) {
        const BASE: f32 = 14.0;
        match self {
            Self::Body => (BASE, 400),
            Self::Heading => (BASE, 700),
            Self::Caption => (BASE * 0.82, 400),
            Self::Title1 => (BASE * 1.81, 800),
            Self::Title2 => (BASE * 1.36, 800),
            Self::Numeric => (BASE, 600),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Start,
    Centre,
    End,
}

#[derive(Clone, Debug)]
pub enum Cmd {
    Fill {
        r: Rect,
        radius: f32,
        c: Color,
    },
    Stroke {
        r: Rect,
        radius: f32,
        c: Color,
        w: f32,
    },
    Line {
        x0: f32,
        y0: f32,
        x1: f32,
        y1: f32,
        c: Color,
        w: f32,
    },
    Text {
        r: Rect,
        s: String,
        style: TextStyle,
        c: Color,
        align: Align,
        wrap: bool,
    },
    Icon {
        icon: Icon,
        r: Rect,
        c: Color,
    },
    /// Draws the current image; `m` maps bitmap pixels to DIPs.
    Image {
        m: Affine,
        smooth: bool,
        /// From its screen-sized preview rather than its full bitmap.
        preview: bool,
    },
    Arc {
        cx: f32,
        cy: f32,
        radius: f32,
        start: f32,
        sweep: f32,
        c: Color,
        w: f32,
    },
    Clip(Rect),
    Unclip,
}

pub trait Measure {
    /// Returns (width, height) of `s` laid out within `max_w`.
    fn measure(&self, s: &str, style: TextStyle, max_w: f32, wrap: bool) -> (f32, f32);
}

pub struct Widget {
    pub rect: Rect,
    pub action: Action,
    pub tooltip: Option<String>,
    pub enabled: bool,
    /// Reachable with Tab (controls, not backdrops or surfaces).
    pub focusable: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Style {
    /// Header-bar and toolbar buttons.
    Flat,
    Normal,
    Suggested,
    /// A large rounded call-to-action (libadwaita `.pill.suggested-action`).
    Pill,
    Destructive,
    /// On-screen display buttons over the image.
    Osd,
    /// Window controls: a 24px circle inside a larger hit area.
    Control,
}

pub enum Content<'s> {
    Icon(Icon),
    Label(&'s str),
    Dropdown(&'s str),
}

pub struct Ui<'a> {
    pub cmds: Vec<Cmd>,
    pub widgets: Vec<Widget>,
    pub t: &'a Theme,
    pub m: &'a dyn Measure,
    pub hover: Option<Action>,
    pub pressed: Option<Action>,
    pub focus: Option<Action>,
    /// Opacity applied to everything emitted (for fading overlays).
    pub alpha: f32,
}

impl<'a> Ui<'a> {
    pub fn new(t: &'a Theme, m: &'a dyn Measure) -> Self {
        Self { cmds: Vec::with_capacity(256), widgets: Vec::with_capacity(64), t, m, hover: None, pressed: None, focus: None, alpha: 1.0 }
    }

    pub fn fill(&mut self, r: Rect, radius: f32, c: Color) {
        let c = c.alpha(self.alpha);
        if c.a > 0.0 {
            self.cmds.push(Cmd::Fill { r, radius, c });
        }
    }

    pub fn stroke(&mut self, r: Rect, radius: f32, c: Color, w: f32) {
        self.cmds.push(Cmd::Stroke { r, radius, c: c.alpha(self.alpha), w });
    }

    pub fn line(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, c: Color, w: f32) {
        self.cmds.push(Cmd::Line { x0, y0, x1, y1, c: c.alpha(self.alpha), w });
    }

    pub fn hline(&mut self, x0: f32, x1: f32, y: f32, c: Color) {
        self.line(x0, y + 0.5, x1, y + 0.5, c, 1.0);
    }

    pub fn vline(&mut self, x: f32, y0: f32, y1: f32, c: Color) {
        self.line(x + 0.5, y0, x + 0.5, y1, c, 1.0);
    }

    pub fn text(&mut self, r: Rect, s: impl Into<String>, style: TextStyle, c: Color, align: Align) {
        self.cmds.push(Cmd::Text { r, s: s.into(), style, c: c.alpha(self.alpha), align, wrap: false });
    }

    /// Wrapped paragraph; returns its height.
    #[allow(clippy::too_many_arguments)]
    pub fn paragraph(&mut self, x: f32, y: f32, w: f32, s: &str, style: TextStyle, c: Color, align: Align) -> f32 {
        let (_, h) = self.m.measure(s, style, w, true);
        self.cmds.push(Cmd::Text { r: Rect::new(x, y, w, h), s: s.into(), style, c: c.alpha(self.alpha), align, wrap: true });
        h
    }

    pub fn icon(&mut self, icon: Icon, r: Rect, c: Color) {
        self.cmds.push(Cmd::Icon { icon, r, c: c.alpha(self.alpha) });
    }

    pub fn text_width(&self, s: &str, style: TextStyle) -> f32 {
        self.m.measure(s, style, f32::MAX, false).0
    }

    pub fn widget(&mut self, rect: Rect, action: Action, tooltip: Option<&str>) {
        self.widgets.push(Widget { rect, action, tooltip: tooltip.map(Into::into), enabled: true, focusable: false });
    }

    /// Makes everything emitted so far inert, as below a modal surface.
    pub fn modal(&mut self, dim: Option<Color>, size: Rect, backdrop: Action) {
        self.widgets.clear();
        if let Some(c) = dim {
            self.fill(size, 0.0, c);
        }
        self.widget(size, backdrop, None);
    }

    /// A soft drop shadow made of stacked translucent outlines.
    pub fn shadow(&mut self, r: Rect, radius: f32, strength: f32) {
        const STEPS: usize = 8;
        for i in (1..=STEPS).rev() {
            let d = i as f32 * 1.5;
            let a = strength * 0.035 * (1.0 - i as f32 / (STEPS as f32 + 1.0));
            let s = Rect::new(r.x - d, r.y - d + d * 0.5, r.w + 2.0 * d, r.h + 2.0 * d);
            self.fill(s, radius + d, Color::BLACK.alpha(a));
        }
    }

    fn state(&self, action: Action) -> (bool, bool) {
        (self.hover == Some(action), self.pressed == Some(action) && self.hover == Some(action))
    }

    /// Draws and registers a button. `fg` is the colour of the surface it
    /// sits on (libadwaita's `currentColor`).
    #[allow(clippy::too_many_arguments)]
    pub fn button(
        &mut self,
        r: Rect,
        action: Action,
        content: Content<'_>,
        style: Style,
        fg: Color,
        enabled: bool,
        checked: bool,
        tooltip: Option<&str>,
    ) {
        let t = self.t;
        let (hover, active) = if enabled { self.state(action) } else { (false, false) };
        let k = if enabled { 1.0 } else { DISABLED_OPACITY };
        let radius = match style {
            Style::Osd | Style::Control | Style::Pill => r.h.min(r.w) / 2.0,
            _ => 6.0_f32.min(r.h / 2.0),
        };

        let (bg, fg) = match style {
            Style::Flat => {
                let a = match (active, checked, hover) {
                    (true, _, _) => 0.16,
                    (_, true, true) => 0.13,
                    (_, true, false) => 0.10,
                    (_, _, true) => 0.07,
                    _ => 0.0,
                };
                (fg.alpha(a), fg)
            }
            Style::Normal => {
                let a = match (active, checked, hover) {
                    (true, _, _) => 0.3,
                    (_, true, true) => 0.35,
                    (_, true, false) => 0.3,
                    (_, _, true) => 0.15,
                    _ => 0.1,
                };
                (fg.alpha(a), fg)
            }
            Style::Suggested | Style::Pill | Style::Destructive => {
                let base = if style == Style::Destructive { t.destructive_bg } else { t.accent_bg };
                let bg = match (active, hover) {
                    (true, _) => base.mix(Color::BLACK, 0.2),
                    (_, true) => base.mix(Color::WHITE, 0.1),
                    _ => base,
                };
                (bg, t.accent_fg)
            }
            Style::Osd => {
                let bg = match (active, hover) {
                    (true, _) => t.osd_bg.mix(Color::WHITE, 0.2),
                    (_, true) => t.osd_bg.mix(Color::WHITE, 0.1),
                    _ => t.osd_bg,
                };
                (bg, t.osd_fg)
            }
            Style::Control => (Color::TRANSPARENT, fg),
        };

        if style == Style::Control {
            let a = if active {
                0.3
            } else if hover {
                0.15
            } else {
                0.1
            };
            let c = r.centred(24.0, 24.0);
            self.fill(c, 12.0, fg.alpha(a * k));
        } else {
            self.fill(r, radius, bg.alpha(k));
        }

        let fg = fg.alpha(k);
        match content {
            Content::Icon(icon) => self.icon(icon, r.centred(16.0, 16.0).round(), fg),
            Content::Label(s) => {
                let style = if matches!(style, Style::Flat | Style::Normal | Style::Osd) { TextStyle::Body } else { TextStyle::Heading };
                self.text(r, s, style, fg, Align::Centre)
            }
            Content::Dropdown(s) => {
                let tw = self.text_width(s, TextStyle::Body);
                let x = r.cx() - (tw + 6.0 + 16.0) / 2.0;
                self.text(Rect::new(x, r.y, tw + 1.0, r.h), s, TextStyle::Body, fg, Align::Start);
                self.icon(Icon::PanDown, Rect::new(x + tw + 6.0, r.cy() - 8.0, 16.0, 16.0), fg.alpha(0.8));
            }
        }

        if self.focus == Some(action) {
            self.stroke(r.inset(1.0), (radius - 1.0).max(0.0), t.accent.alpha(0.5), 2.0);
        }
        self.widgets.push(Widget { rect: r, action, tooltip: tooltip.map(Into::into), enabled, focusable: true });
    }

    /// A popover menu row with optional check mark and accelerator.
    #[allow(clippy::too_many_arguments)]
    pub fn menu_item(&mut self, r: Rect, action: Action, label: &str, accel: Option<&str>, check: Option<bool>, enabled: bool, fg: Color) {
        let (hover, active) = if enabled { self.state(action) } else { (false, false) };
        let k = if enabled { 1.0 } else { DISABLED_OPACITY };
        if hover || active || self.focus == Some(action) {
            self.fill(r, 6.0, fg.alpha(if active { 0.16 } else { 0.1 }));
        }
        let mut x = r.x + 12.0;
        if let Some(on) = check {
            if on {
                self.icon(Icon::Check, Rect::new(x, r.cy() - 8.0, 16.0, 16.0), fg.alpha(k));
            }
            x += 24.0;
        }
        let mut right = r.right() - 12.0;
        if let Some(a) = accel {
            let w = self.text_width(a, TextStyle::Body);
            self.text(Rect::new(right - w, r.y, w + 1.0, r.h), a, TextStyle::Body, fg.alpha(0.55 * k), Align::End);
            right -= w + 24.0;
        }
        self.text(Rect::new(x, r.y, (right - x).max(0.0), r.h), label, TextStyle::Body, fg.alpha(k), Align::Start);
        self.widgets.push(Widget { rect: r, action, tooltip: None, enabled, focusable: true });
    }
}
