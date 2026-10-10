//! Immediate-mode UI: the view emits draw commands and hit-test widgets in
//! one pass, so painting and input always agree. Styling follows libadwaita.

use crate::edit::{Aspect, Handle};
use crate::geom::{Affine, Rect};
use crate::icons::Icon;
use crate::theme::{Color, DIM_OPACITY, DISABLED_OPACITY, STRONG_DISABLED_OPACITY, Theme};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Swallows clicks (modal backdrops).
    None,
    /// Closes the open popover or dialog.
    Dismiss,
    Open,
    /// Opens the chosen images without the rest of their folder.
    OpenOnly,
    /// Adds the other images in the current image's folder.
    ShowFolder,
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
    Preferences,
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
    DefaultApps,
    CheckUpdates,
    ToggleAutoUpdate,
    Restart,
    OpenProject,
    ReportIssue,
    CloseToast,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TextStyle {
    Body,
    Heading,
    Caption,
    Title1,
    Title2,
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
    /// A filled polygon (popover arrows).
    Polygon {
        points: Vec<(f32, f32)>,
        c: Color,
    },
    /// The application icon.
    AppIcon(Rect),
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

/// One CSS `box-shadow` in `rgb(0 0 6 / a)`: y offset, blur radius, spread.
#[derive(Clone, Copy)]
pub struct Shadow {
    y: f32,
    blur: f32,
    spread: f32,
    a: f32,
}

const fn shadow(y: f32, blur: f32, spread: f32, a: f32) -> Shadow {
    Shadow { y, blur, spread, a }
}

/// `.card` and `.boxed-list`.
pub const CARD_SHADOW: &[Shadow] = &[shadow(0.0, 0.0, 1.0, 0.03), shadow(1.0, 3.0, 1.0, 0.07), shadow(2.0, 6.0, 2.0, 0.03)];
pub const POPOVER_SHADOW: &[Shadow] = &[shadow(0.0, 0.0, 1.0, 0.05), shadow(1.0, 5.0, 1.0, 0.09), shadow(2.0, 14.0, 3.0, 0.05)];
/// The sheet of a floating `AdwDialog`.
pub const DIALOG_SHADOW: &[Shadow] = &[shadow(0.0, 14.0, 2.0, 0.03), shadow(0.0, 5.0, 2.0, 0.10), shadow(0.0, 0.0, 1.0, 0.05)];
pub const TOAST_SHADOW: &[Shadow] = &[shadow(1.0, 3.0, 1.0, 0.07), shadow(2.0, 6.0, 2.0, 0.03)];
const SWITCH_KNOB_SHADOW: &[Shadow] = &[shadow(2.0, 4.0, 0.0, 0.2)];

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
    /// Corner radius of buttons that aren't round.
    pub radius: f32,
}

impl<'a> Ui<'a> {
    pub fn new(t: &'a Theme, m: &'a dyn Measure) -> Self {
        Self {
            cmds: Vec::with_capacity(256),
            widgets: Vec::with_capacity(64),
            t,
            m,
            hover: None,
            pressed: None,
            focus: None,
            alpha: 1.0,
            radius: 9.0,
        }
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

    /// CSS `box-shadow`s behind `r`, which must then be painted opaque. A
    /// blurred edge is approximated by stacked fills ramping linearly from
    /// `spread + blur` outside the box to `spread - blur` inside it.
    pub fn box_shadow(&mut self, r: Rect, radius: f32, shadows: &[Shadow]) {
        for s in shadows {
            let n = (s.blur.ceil() as usize).clamp(1, 8);
            for i in 0..n {
                let e = s.spread + s.blur * (1.0 - (2 * i + 1) as f32 / n as f32);
                let b = Rect::new(r.x - e, r.y + s.y - e, r.w + 2.0 * e, r.h + 2.0 * e);
                if b.w > 0.0 && b.h > 0.0 {
                    self.fill(b, (radius + e).max(0.0), Color::rgba(0, 0, 6, s.a / n as f32));
                }
            }
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
        let k = match (enabled, style) {
            (true, _) => 1.0,
            (false, Style::Flat) if !checked => STRONG_DISABLED_OPACITY,
            _ => DISABLED_OPACITY,
        };
        let radius = match style {
            Style::Osd | Style::Control | Style::Pill => r.h.min(r.w) / 2.0,
            _ => self.radius.min(r.h / 2.0),
        };

        let (bg, fg) = match style {
            Style::Flat => {
                let a = match (active, checked, hover) {
                    (true, true, _) => 0.19,
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
            Style::Suggested | Style::Pill => {
                let bg = match (active, hover) {
                    (true, _) => t.accent_bg.mix(Color::rgb(0, 0, 6), 0.2),
                    (_, true) => t.accent_bg.mix(t.accent_fg, 0.1),
                    _ => t.accent_bg,
                };
                (bg, t.accent_fg)
            }
            // Red text on a red tint, not a red fill.
            Style::Destructive => {
                let a = match (active, hover) {
                    (true, _) => 0.35,
                    (_, true) => 0.2,
                    _ => 0.15,
                };
                (t.destructive.alpha(a), t.destructive)
            }
            // Black at 65 %, lightened by the label colour when pointed at.
            Style::Osd => {
                let lift = match (active, hover) {
                    (true, _) => 0.25,
                    (_, true) => 0.15,
                    _ => 0.0,
                };
                let fg = if active || hover { Color::WHITE } else { t.osd_fg };
                (Color { a: 0.65, ..Color::BLACK.mix(fg, lift) }, fg)
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
            // Button labels are bold in libadwaita.
            Content::Label(s) => self.text(r, s, TextStyle::Heading, fg, Align::Centre),
            Content::Dropdown(s) => {
                let tw = self.text_width(s, TextStyle::Heading);
                let x = r.cx() - (tw + 6.0 + 16.0) / 2.0;
                self.text(Rect::new(x, r.y, tw + 1.0, r.h), s, TextStyle::Heading, fg, Align::Start);
                self.icon(Icon::PanDown, Rect::new(x + tw + 6.0, r.cy() - 8.0, 16.0, 16.0), fg);
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
            self.fill(r, 9.0, fg.alpha(if active { 0.19 } else { 0.1 }));
        }
        let mut x = r.x + 12.0;
        if let Some(on) = check {
            if on {
                self.icon(Icon::Check, Rect::new(x - 2.0, r.cy() - 8.0, 16.0, 16.0), fg.alpha(k));
            }
            x += 20.0;
        }
        let mut right = r.right() - 12.0;
        if let Some(a) = accel {
            let w = self.text_width(a, TextStyle::Body);
            self.text(Rect::new(right - w, r.y, w + 1.0, r.h), a, TextStyle::Body, fg.alpha(DIM_OPACITY * k), Align::End);
            right -= w + 12.0;
        }
        self.text(Rect::new(x, r.y, (right - x).max(0.0), r.h), label, TextStyle::Body, fg.alpha(k), Align::Start);
        self.widgets.push(Widget { rect: r, action, tooltip: None, enabled, focusable: true });
    }

    /// A `GtkSwitch`, 46 × 26, its knob sliding to the end when on. Rows
    /// that hold one register their own, larger hit area for `action`.
    pub fn switch(&mut self, x: f32, y: f32, action: Action, on: bool, fg: Color) {
        let t = self.t;
        let (hover, active) = self.state(action);
        let r = Rect::new(x, y, 46.0, 26.0);
        let track = if on {
            match (active, hover) {
                (true, _) => t.accent_bg.mix(Color::rgb(0, 0, 6), 0.2),
                (_, true) => t.accent_bg.mix(t.accent_fg, 0.1),
                _ => t.accent_bg,
            }
        } else {
            fg.alpha(if active {
                0.25
            } else if hover {
                0.2
            } else {
                0.15
            })
        };
        self.fill(r, 13.0, track);
        let knob = Rect::new(if on { r.right() - 23.0 } else { r.x + 3.0 }, r.y + 3.0, 20.0, 20.0);
        self.box_shadow(knob, 10.0, SWITCH_KNOB_SHADOW);
        let colour = if on || hover || active { Color::WHITE } else { Color::WHITE.mix(t.view_bg, 0.2) };
        self.fill(knob, 10.0, colour);
        if self.focus == Some(action) {
            self.stroke(r.inset(-2.0), 15.0, t.accent.alpha(0.5), 2.0);
        }
    }
}
