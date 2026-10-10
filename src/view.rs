// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 professional-cynic

//! Layout of every surface, following libadwaita's widgets and metrics:
//! header bar, status pages, OSD controls, boxed lists, popover menus,
//! dialogs and toasts.

use std::f32::consts::TAU;
use std::path::Path;

use crate::app::{App, BANNER_H, Dialog, HEADER_H, Popover, SIDEBAR_W, TOOLBAR_H};
use crate::edit::{Aspect, Handle};
use crate::geom::{Affine, Rect};
use crate::icons::Icon;
use crate::meta;
use crate::settings::SLOTS;
use crate::theme::{ACCENTS, BORDER_OPACITY, Color, DIM_OPACITY};
use crate::ui::{Action, Align, CARD_SHADOW, Cmd, Content, DIALOG_SHADOW, POPOVER_SHADOW, Style, TOAST_SHADOW, TextStyle, Ui};

const BTN: f32 = 34.0;
const GAP: f32 = 6.0;
/// An `AdwActionRow` with a subtitle: 2 px row padding, 6 px title margins.
const ROW_H: f32 = 56.0;
/// The single-line minimum: 50 plus the row padding.
const ROW_MIN: f32 = 54.0;
/// Where row text starts: row padding plus header margin.
const ROW_INSET: f32 = 14.0;
const MENU_ROW: f32 = 32.0;
/// A preferences group's heading plus the space below it.
const GROUP_HEAD: f32 = 32.0;

/// Where popovers attach.
#[derive(Default)]
struct Anchors {
    main: Option<Rect>,
    marks: Option<Rect>,
    aspect: Option<Rect>,
    save: Option<Rect>,
}

pub fn build(app: &App) -> Ui<'_> {
    let mut ui = Ui::new(&app.theme, &app.r.text);
    ui.hover = app.hover;
    ui.pressed = app.pressed;
    ui.focus = app.focus;
    let mut anchors = Anchors::default();

    if app.edit.is_some() {
        edit_canvas(app, &mut ui);
    } else {
        canvas(app, &mut ui);
    }
    if app.sidebar_open() {
        sidebar(app, &mut ui);
    }
    if app.edit.is_some() {
        toolbar(app, &mut ui, &mut anchors);
    }
    if app.banner_visible() {
        banner(app, &mut ui);
    }
    if app.header_visible() {
        header(app, &mut ui, &mut anchors);
    }
    toast(app, &mut ui);
    if let Some(p) = app.popover {
        popover(app, &mut ui, p, &anchors);
    }
    if let Some(d) = app.dialog {
        dialog(app, &mut ui, d);
    }
    tooltip(app, &mut ui);
    ui
}

fn file_name(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

// ------------------------------------------------------------------------
// Header bar
// ------------------------------------------------------------------------

fn header(app: &App, ui: &mut Ui, anchors: &mut Anchors) {
    let t = ui.t;
    let w = app.width();
    let bar = Rect::new(0.0, 0.0, w, HEADER_H - 1.0);
    ui.fill(bar, 0.0, if app.active { t.headerbar_bg } else { t.headerbar_backdrop });
    ui.hline(0.0, w, HEADER_H - 1.0, t.headerbar_shade);
    if app.fullscreen {
        // The header floats over the image: keep clicks off the canvas.
        ui.widget(bar, Action::None, None);
    }
    let fg = if app.active { t.headerbar_fg } else { t.headerbar_fg.alpha(0.6) };
    let y = (HEADER_H - 1.0 - BTN) / 2.0;
    let mut right = w - GAP;

    if app.fullscreen {
        right -= BTN;
        let r = Rect::new(right, y, BTN, BTN);
        ui.button(r, Action::Fullscreen, Content::Icon(Icon::LeaveFullscreen), Style::Flat, fg, true, false, Some("Leave Fullscreen"));
        right -= GAP;
    } else {
        let max_icon = if app.maximised { Icon::WindowRestore } else { Icon::WindowMaximize };
        let max_tip = if app.maximised { "Restore" } else { "Maximise" };
        for (action, icon, tip) in
            [(Action::Close, Icon::WindowClose, "Close"), (Action::Maximize, max_icon, max_tip), (Action::Minimize, Icon::WindowMinimize, "Minimise")]
        {
            right -= BTN;
            ui.button(Rect::new(right, y, BTN, BTN), action, Content::Icon(icon), Style::Control, fg, true, false, Some(tip));
        }
        right -= GAP * 2.0;
    }

    let mut left = GAP;
    let (title, subtitle) = if app.edit.is_some() {
        let label = "Cancel";
        let bw = ui.text_width(label, TextStyle::Heading) + 34.0;
        ui.button(Rect::new(left, y, bw, BTN), Action::EditCancel, Content::Label(label), Style::Normal, fg, true, false, None);
        left += bw + GAP;

        // Split button: "Save Copy" plus a menu.
        let label = "Save Copy";
        let lw = ui.text_width(label, TextStyle::Heading) + 30.0;
        let aw = 28.0;
        right -= aw + lw;
        let whole = Rect::new(right, y, lw + aw, BTN);
        let main = Rect::new(right, y, lw, BTN);
        let arrow = Rect::new(right + lw, y, aw, BTN);
        let enabled = !app.busy();
        let k = if enabled { 1.0 } else { 0.5 };
        ui.fill(whole, 9.0, t.accent_bg.alpha(k));
        for (part, action) in [(main, Action::EditSave), (arrow, Action::SaveMenu)] {
            let state = (app.hover == Some(action), app.pressed == Some(action));
            let tint = match state {
                (true, true) => Some(Color::BLACK.alpha(0.2)),
                (true, false) => Some(Color::WHITE.alpha(0.1)),
                _ if app.popover == Some(Popover::Save) && action == Action::SaveMenu => Some(Color::BLACK.alpha(0.15)),
                _ => None,
            };
            if let (Some(c), true) = (tint, enabled) {
                ui.cmds.push(Cmd::Clip(part));
                ui.fill(whole, 9.0, c);
                ui.cmds.push(Cmd::Unclip);
            }
            if app.focus == Some(action) {
                ui.stroke(part.inset(1.0), 8.0, t.accent_fg.alpha(0.6), 2.0);
            }
            ui.widgets.push(crate::ui::Widget { rect: part, action, tooltip: None, enabled, focusable: true });
        }
        ui.text(main, label, TextStyle::Heading, t.accent_fg.alpha(k), Align::Centre);
        ui.vline(arrow.x, y + 8.0, y + BTN - 8.0, t.accent_fg.alpha(0.3 * k));
        ui.icon(Icon::PanDown, arrow.centred(16.0, 16.0), t.accent_fg.alpha(k));
        anchors.save = Some(arrow);
        right -= GAP;
        ("Edit".to_owned(), None)
    } else {
        let open = Rect::new(left, y, BTN, BTN);
        ui.button(open, Action::Open, Content::Icon(Icon::Folder), Style::Flat, fg, true, false, Some("Open… (Ctrl+O)"));
        left += BTN + GAP;

        let has = app.current().is_some();
        let marked = app.current_marks() != 0;
        let buttons: [(Action, Icon, bool, bool, &str); 5] = [
            (Action::MainMenu, Icon::OpenMenu, true, app.popover == Some(Popover::Main), "Main Menu (F10)"),
            (Action::Properties, Icon::Info, has, app.settings.properties, "Properties (F9)"),
            (Action::MarksMenu, if marked { Icon::BookmarkFilled } else { Icon::Bookmark }, true, app.popover == Some(Popover::Marks), "Marks (M)"),
            (Action::Edit, Icon::Edit, app.can_edit(), false, "Edit (E)"),
            (Action::Fullscreen, Icon::Fullscreen, true, false, "Fullscreen (F11)"),
        ];
        for (action, icon, enabled, checked, tip) in buttons {
            if action == Action::Fullscreen && app.fullscreen {
                continue;
            }
            right -= BTN;
            let r = Rect::new(right, y, BTN, BTN);
            ui.button(r, action, Content::Icon(icon), Style::Flat, fg, enabled, checked, Some(tip));
            match action {
                Action::MainMenu => anchors.main = Some(r),
                Action::MarksMenu => anchors.marks = Some(r),
                _ => {}
            }
            right -= GAP;
        }

        match app.current() {
            // A lone image opened on its own needs no position.
            Some(p) if app.selection && app.files.len() == 1 => (file_name(p), None),
            Some(p) => {
                let sel = if app.selection { " selected" } else { "" };
                (file_name(p), Some(format!("{} of {}{sel}", app.index + 1, app.files.len())))
            }
            None => ("Windmolen".to_owned(), None),
        }
    };

    // Centred title, shifted only when it would collide with the sides.
    let pad = 12.0;
    let room = ((w / 2.0 - left).min(right - w / 2.0) - pad).max(0.0) * 2.0;
    let need = ui.text_width(&title, TextStyle::Heading).max(subtitle.as_deref().map_or(0.0, |s| ui.text_width(s, TextStyle::Caption)));
    let area = if need <= room {
        Rect::new(w / 2.0 - room / 2.0, 0.0, room, HEADER_H - 1.0)
    } else {
        Rect::new(left + pad, 0.0, (right - left - 2.0 * pad).max(0.0), HEADER_H - 1.0)
    };
    match subtitle {
        Some(sub) => {
            ui.text(Rect::new(area.x, 6.0, area.w, 20.0), title, TextStyle::Heading, fg, Align::Centre);
            ui.text(Rect::new(area.x, 24.0, area.w, 16.0), sub, TextStyle::Caption, fg.alpha(DIM_OPACITY), Align::Centre);
        }
        None => ui.text(area, title, TextStyle::Heading, fg, Align::Centre),
    }
}

/// An `AdwBanner` announcing an installed update.
fn banner(app: &App, ui: &mut Ui) {
    let crate::app::UpdateState::Ready(v) = &app.update else { return };
    let t = ui.t;
    let r = Rect::new(0.0, app.header_h(), app.width(), BANNER_H);
    ui.fill(r, 0.0, t.view_bg);
    ui.fill(r, 0.0, t.accent_bg.alpha(0.3));
    ui.hline(0.0, r.w, r.bottom() - 1.0, t.headerbar_shade);
    ui.widget(r, Action::None, None);
    let label = "Restart";
    let bw = ui.text_width(label, TextStyle::Heading) + 34.0;
    let b = Rect::new(r.right() - 12.0 - bw, r.y + (BANNER_H - BTN) / 2.0, bw, BTN);
    ui.button(b, Action::Restart, Content::Label(label), Style::Suggested, t.window_fg, true, false, None);
    let text = format!("Windmolen {v} has been installed");
    ui.text(Rect::new(12.0 + bw, r.y, (r.w - 2.0 * (bw + 24.0)).max(0.0), BANNER_H), text, TextStyle::Body, t.window_fg, Align::Centre);
}

// ------------------------------------------------------------------------
// Canvas
// ------------------------------------------------------------------------

fn canvas(app: &App, ui: &mut Ui) {
    let t = ui.t;
    let vp = app.viewport();
    if let (Some(l), Some(o)) = (app.loaded(), app.orient()) {
        let v = app.effective_view();
        let ds = app.dpi_scale();
        let (b, preview) = l.pick(v.s * ds);
        let k = l.meta.width as f32 / b.w as f32;
        let snap = |x: f32| (x * ds).round() / ds;
        let m = o
            .matrix(b.w as f32, b.h as f32)
            .then(Affine::scale(k * v.s, k * v.s))
            .then(Affine::translate(snap(vp.cx() - v.cx * v.s), snap(vp.cy() - v.cy * v.s)));
        ui.cmds.push(Cmd::Clip(vp));
        // Source pixels show as squares from 200 %; a reduced bitmap is
        // always smoothed.
        ui.cmds.push(Cmd::Image { m, smooth: k > 1.0 || v.s * ds < 2.0, preview });
        ui.cmds.push(Cmd::Unclip);
        if app.osd > 0.01 && app.dialog.is_none() {
            osd(app, ui, vp);
        }
    } else if let Some(err) = app.failure() {
        let name = app.current().map(file_name).unwrap_or_default();
        status_page(ui, vp, Icon::ImageMissing, "Could Not Load Image", &format!("{name}\n{err}"), None);
        if app.files.len() > 1 {
            ui.alpha = app.osd.max(0.6);
            nav_buttons(app, ui, vp);
            ui.alpha = 1.0;
        }
    } else if app.files.is_empty() {
        status_page(ui, vp, Icon::Image, "No Image Open", "Drag and drop an image here", Some(("Open Image…", Action::Open)));
    } else if app.show_spinner() {
        let (cx, cy) = (vp.cx(), vp.cy());
        let r = 16.0;
        ui.stroke(Rect::new(cx - r, cy - r, 2.0 * r, 2.0 * r), r, t.view_fg.alpha(0.15), 3.0);
        let a = app.loading_phase() * TAU * 1.2;
        ui.cmds.push(Cmd::Arc { cx, cy, radius: r, start: a, sweep: TAU / 4.0, c: t.accent, w: 3.0 });
    }
    mark_chips(app, ui, vp);
}

fn nav_buttons(app: &App, ui: &mut Ui, vp: Rect) {
    if app.files.len() < 2 {
        return;
    }
    let fg = ui.t.osd_fg;
    let s = 48.0;
    let y = vp.cy() - s / 2.0;
    let prev = Rect::new(vp.x + 18.0, y, s, s);
    let next = Rect::new(vp.right() - 18.0 - s, y, s, s);
    ui.button(prev, Action::Prev, Content::Icon(Icon::GoPrevious), Style::Osd, fg, true, false, Some("Previous (←)"));
    ui.button(next, Action::Next, Content::Icon(Icon::GoNext), Style::Osd, fg, true, false, Some("Next (→)"));
}

fn osd(app: &App, ui: &mut Ui, vp: Rect) {
    ui.alpha = app.osd;
    nav_buttons(app, ui, vp);
    let fg = ui.t.osd_fg;
    let label = if app.effective_view().fit { "Best Fit".to_owned() } else { format!("{:.0}%", app.zoom_percent()) };
    let lw = ui.text_width("Best Fit", TextStyle::Heading).max(ui.text_width(&label, TextStyle::Heading)) + 28.0;
    let y = vp.bottom() - 18.0 - BTN;
    let mut x = vp.right() - 18.0 - BTN;
    ui.button(Rect::new(x, y, BTN, BTN), Action::ZoomIn, Content::Icon(Icon::ZoomIn), Style::Osd, fg, true, false, Some("Zoom In (+)"));
    x -= GAP + lw;
    ui.button(Rect::new(x, y, lw, BTN), Action::ZoomToggle, Content::Label(&label), Style::Osd, fg, true, false, Some("Best Fit / 100%"));
    x -= GAP + BTN;
    ui.button(Rect::new(x, y, BTN, BTN), Action::ZoomOut, Content::Icon(Icon::ZoomOut), Style::Osd, fg, true, false, Some("Zoom Out (−)"));
    ui.alpha = 1.0;
}

fn mark_chips(app: &App, ui: &mut Ui, vp: Rect) {
    let bits = app.current_marks();
    if bits == 0 || app.edit.is_some() {
        return;
    }
    let t = ui.t;
    let mut x = vp.x + 12.0;
    let y = vp.y + 12.0;
    for slot in (0..SLOTS).filter(|s| bits & (1 << s) != 0) {
        let label = format!("{}  {}", slot + 1, app.mark_label(slot).unwrap_or_default());
        let tw = ui.text_width(&label, TextStyle::Caption);
        let r = Rect::new(x, y, tw + 34.0, 28.0);
        ui.fill(r, 14.0, t.osd_bg);
        ui.fill(Rect::new(r.x + 10.0, r.cy() - 5.0, 10.0, 10.0), 5.0, ACCENTS[slot]);
        ui.text(Rect::new(r.x + 26.0, r.y, tw + 1.0, r.h), label, TextStyle::Caption, t.osd_fg, Align::Start);
        x += r.w + GAP;
    }
}

fn status_page(ui: &mut Ui, area: Rect, icon: Icon, title: &str, desc: &str, button: Option<(&str, Action)>) {
    let t = ui.t;
    let w = (area.w - 48.0).clamp(0.0, 520.0);
    let (_, th) = ui.m.measure(title, TextStyle::Title1, w, true);
    let (_, dh) = ui.m.measure(desc, TextStyle::Body, w, true);
    let icon_s = if area.h < 400.0 { 64.0 } else { 128.0 };
    let bh = if button.is_some() { 24.0 + 40.0 } else { 0.0 };
    let total = icon_s + 24.0 + th + 12.0 + dh + bh;
    let x = area.cx() - w / 2.0;
    let mut y = area.cy() - total / 2.0;
    ui.icon(icon, Rect::new(area.cx() - icon_s / 2.0, y, icon_s, icon_s), t.view_fg.alpha(DIM_OPACITY));
    y += icon_s + 24.0;
    y += ui.paragraph(x, y, w, title, TextStyle::Title1, t.view_fg, Align::Centre) + 12.0;
    y += ui.paragraph(x, y, w, desc, TextStyle::Body, t.view_fg, Align::Centre);
    if let Some((label, action)) = button {
        let bw = ui.text_width(label, TextStyle::Heading) + 64.0;
        let r = Rect::new(area.cx() - bw / 2.0, y + 24.0, bw, 40.0);
        ui.button(r, action, Content::Label(label), Style::Pill, t.accent_fg, true, false, None);
    }
}

// ------------------------------------------------------------------------
// Editing
// ------------------------------------------------------------------------

fn edit_canvas(app: &App, ui: &mut Ui) {
    let (Some(l), Some(o), Some(e), Some((r, s))) = (app.loaded(), app.orient(), app.edit.as_ref(), app.edit_geometry()) else {
        return;
    };
    let (b, preview) = l.pick(s * app.dpi_scale());
    let k = l.meta.width as f32 / b.w as f32;
    let m = o.matrix(b.w as f32, b.h as f32).then(Affine::scale(k * s, k * s)).then(Affine::translate(r.x, r.y));
    ui.cmds.push(Cmd::Image { m, smooth: true, preview });

    let c = Rect::new(r.x + e.crop.x * r.w, r.y + e.crop.y * r.h, e.crop.w * r.w, e.crop.h * r.h);
    let shade = Color::BLACK.alpha(0.5);
    ui.fill(Rect::from_edges(r.x, r.y, r.right(), c.y), 0.0, shade);
    ui.fill(Rect::from_edges(r.x, c.bottom(), r.right(), r.bottom()), 0.0, shade);
    ui.fill(Rect::from_edges(r.x, c.y, c.x, c.bottom()), 0.0, shade);
    ui.fill(Rect::from_edges(c.right(), c.y, r.right(), c.bottom()), 0.0, shade);

    let white = Color::WHITE;
    if app.dragging_crop() {
        for i in 1..3 {
            let f = i as f32 / 3.0;
            ui.line(c.x + c.w * f, c.y, c.x + c.w * f, c.bottom(), white.alpha(0.5), 1.0);
            ui.line(c.x, c.y + c.h * f, c.right(), c.y + c.h * f, white.alpha(0.5), 1.0);
        }
    }
    ui.stroke(c, 0.0, white.alpha(0.9), 1.0);

    // Handles sit just inside the crop, with a dark halo for contrast.
    let free = e.aspect == Aspect::Free;
    let len = 18.0f32.min(c.w / 3.0).min(c.h / 3.0);
    let i = 2.0;
    let mut handles: Vec<(f32, f32, f32, f32)> = Vec::new();
    for (x, y, sx, sy) in [
        (c.x + i, c.y + i, 1.0, 1.0),
        (c.right() - i, c.y + i, -1.0, 1.0),
        (c.x + i, c.bottom() - i, 1.0, -1.0),
        (c.right() - i, c.bottom() - i, -1.0, -1.0),
    ] {
        handles.push((x, y, x + sx * len, y));
        handles.push((x, y, x, y + sy * len));
    }
    if free {
        let (mx, my) = (c.cx(), c.cy());
        handles.push((mx - 12.0, c.y + i, mx + 12.0, c.y + i));
        handles.push((mx - 12.0, c.bottom() - i, mx + 12.0, c.bottom() - i));
        handles.push((c.x + i, my - 12.0, c.x + i, my + 12.0));
        handles.push((c.right() - i, my - 12.0, c.right() - i, my + 12.0));
    }
    for &(x0, y0, x1, y1) in &handles {
        ui.line(x0, y0, x1, y1, Color::BLACK.alpha(0.35), 7.0);
    }
    for &(x0, y0, x1, y1) in &handles {
        ui.line(x0, y0, x1, y1, white, 4.0);
    }

    // Hit areas: later entries win.
    ui.widget(c, Action::Crop(Handle::Move), None);
    let g = 10.0;
    if free {
        ui.widget(Rect::new(c.x + g, c.y - g, c.w - 2.0 * g, 2.0 * g), Action::Crop(Handle::N), None);
        ui.widget(Rect::new(c.x + g, c.bottom() - g, c.w - 2.0 * g, 2.0 * g), Action::Crop(Handle::S), None);
        ui.widget(Rect::new(c.x - g, c.y + g, 2.0 * g, c.h - 2.0 * g), Action::Crop(Handle::W), None);
        ui.widget(Rect::new(c.right() - g, c.y + g, 2.0 * g, c.h - 2.0 * g), Action::Crop(Handle::E), None);
    }
    let cs = 28.0;
    for (x, y, h) in [(c.x, c.y, Handle::NW), (c.right(), c.y, Handle::NE), (c.x, c.bottom(), Handle::SW), (c.right(), c.bottom(), Handle::SE)] {
        ui.widget(Rect::new(x - cs / 2.0, y - cs / 2.0, cs, cs), Action::Crop(h), None);
    }
}

fn toolbar(app: &App, ui: &mut Ui, anchors: &mut Anchors) {
    let t = ui.t;
    let (w, h) = (app.width(), app.height());
    let bar = Rect::new(0.0, h - TOOLBAR_H, w, TOOLBAR_H);
    ui.fill(bar, 0.0, if app.active { t.headerbar_bg } else { t.headerbar_backdrop });
    ui.hline(0.0, w, bar.y, t.headerbar_shade);
    let fg = t.headerbar_fg;
    let aspect = app.edit.as_ref().map_or(Aspect::Free, |e| e.aspect).label();
    let aw = ui.text_width(&aspect, TextStyle::Heading) + 16.0 + 6.0 + 24.0;
    let reset = "Reset";
    let rw = ui.text_width(reset, TextStyle::Heading) + 34.0;
    let total = 4.0 * BTN + 2.0 * GAP + 18.0 + aw + 18.0 + rw + GAP;
    let mut x = (w - total) / 2.0;
    let y = bar.y + (TOOLBAR_H - BTN) / 2.0 + 0.5;
    let tools = [
        (Action::RotateCcw, Icon::RotateLeft, "Rotate Left (Shift+R)"),
        (Action::RotateCw, Icon::RotateRight, "Rotate Right (R)"),
        (Action::FlipH, Icon::FlipHorizontal, "Flip Horizontally (H)"),
        (Action::FlipV, Icon::FlipVertical, "Flip Vertically (V)"),
    ];
    for (i, (action, icon, tip)) in tools.into_iter().enumerate() {
        ui.button(Rect::new(x, y, BTN, BTN), action, Content::Icon(icon), Style::Flat, fg, true, false, Some(tip));
        x += BTN + if i == 1 { GAP * 2.0 } else { 0.0 };
    }
    x += 18.0;
    let r = Rect::new(x, y, aw, BTN);
    let open = app.popover == Some(Popover::Aspect);
    ui.button(r, Action::AspectMenu, Content::Dropdown(&aspect), Style::Flat, fg, true, open, Some("Aspect Ratio"));
    anchors.aspect = Some(r);
    x += aw + 18.0;
    ui.button(Rect::new(x, y, rw, BTN), Action::EditReset, Content::Label(reset), Style::Flat, fg, true, false, None);
}

// ------------------------------------------------------------------------
// Properties sidebar
// ------------------------------------------------------------------------

type Group = (&'static str, Vec<(String, String)>);

fn properties(app: &App) -> Vec<Group> {
    let (Some(p), Some(l)) = (app.current(), app.loaded()) else { return Vec::new() };
    let m = &l.meta;
    let mut file = vec![("Name".to_owned(), file_name(p))];
    if let Some(dir) = p.parent() {
        file.push(("Folder".into(), dir.display().to_string()));
    }
    let format = if m.frames > 1 { format!("{} ({} frames)", m.format, m.frames) } else { m.format.clone() };
    file.push(("Format".into(), format));
    file.push(("Size".into(), meta::file_size(m.file_size)));
    if let Some(t) = &m.modified {
        file.push(("Modified".into(), t.clone()));
    }

    let (w, h) = m.orient.apply_size(m.width, m.height);
    let mp = f64::from(w) * f64::from(h) / 1e6;
    let mut image = vec![("Dimensions".to_owned(), format!("{w} × {h} ({mp:.1} MP)"))];
    if let Some(prof) = &m.profile {
        image.push(("Colour Profile".into(), prof.clone()));
    }
    let bits = app.current_marks();
    if bits != 0 {
        let names: Vec<String> =
            (0..SLOTS).filter(|s| bits & (1 << s) != 0).map(|s| app.mark_label(s).unwrap_or_else(|| format!("{}", s + 1))).collect();
        image.push(("Marks".into(), names.join(", ")));
    }

    let mut groups = vec![("File", file), ("Image", image)];
    if !m.exif.is_empty() {
        groups.push(("Camera", m.exif.iter().map(|(k, v)| ((*k).to_owned(), v.clone())).collect()));
    }
    groups
}

fn groups_height(groups: &[Group]) -> f32 {
    12.0 + groups.iter().map(|(_, rows)| GROUP_HEAD + rows.len() as f32 * ROW_H + 24.0).sum::<f32>() - 12.0
}

pub fn sidebar_max_scroll(app: &App) -> f32 {
    max_scroll(app, &properties(app))
}

fn max_scroll(app: &App, groups: &[Group]) -> f32 {
    (groups_height(groups) - (app.height() - app.content_top())).max(0.0)
}

/// A `.boxed-list` on a surface of colour `under`.
fn boxed_list(ui: &mut Ui, r: Rect, under: Color) {
    let t = ui.t;
    ui.box_shadow(r, 12.0, CARD_SHADOW);
    // CSS paints a shadow outside its box only; cover the part beneath,
    // which would show through the translucent dark card colour.
    ui.fill(r, 12.0, under);
    ui.fill(r, 12.0, t.card_bg);
}

/// The border between two rows of a boxed list, at the top of row `y`.
fn row_separator(ui: &mut Ui, x: f32, w: f32, y: f32) {
    ui.hline(x, x + w, y - 1.0, ui.t.card_shade);
}

/// A preferences group's heading.
fn group_title(ui: &mut Ui, x: f32, y: f32, w: f32, title: &str, fg: Color) {
    ui.text(Rect::new(x, y, w, GROUP_HEAD - 12.0), title, TextStyle::Heading, fg, Align::Start);
}

fn sidebar(app: &App, ui: &mut Ui) {
    let t = ui.t;
    let top = app.content_top();
    let area = Rect::new(app.width() - SIDEBAR_W, top, SIDEBAR_W, app.height() - top);
    let bg = if app.active { t.sidebar_bg } else { t.sidebar_backdrop };
    ui.fill(area, 0.0, bg);
    ui.vline(area.x, top, area.bottom(), t.sidebar_border);
    ui.widget(area, Action::None, None);
    let groups = properties(app);
    let scroll = app.props_scroll.min(max_scroll(app, &groups));
    ui.cmds.push(Cmd::Clip(Rect::from_edges(area.x + 1.0, area.y, area.right(), area.bottom())));
    let fg = t.window_fg;
    let x = area.x + 12.0;
    let w = SIDEBAR_W - 24.0;
    let mut y = top + 12.0 - scroll;
    for (title, rows) in groups {
        group_title(ui, x, y, w, title, fg);
        y += GROUP_HEAD;
        let list = Rect::new(x, y, w, rows.len() as f32 * ROW_H);
        boxed_list(ui, list, bg);
        // `.property` rows: a small dimmed title above the value.
        for (i, (label, value)) in rows.iter().enumerate() {
            let ry = y + i as f32 * ROW_H;
            if i > 0 {
                row_separator(ui, x, w, ry);
            }
            let tw = w - 2.0 * ROW_INSET;
            ui.text(Rect::new(x + ROW_INSET, ry + 8.0, tw, 17.0), label.as_str(), TextStyle::Caption, fg.alpha(DIM_OPACITY), Align::Start);
            ui.text(Rect::new(x + ROW_INSET, ry + 28.0, tw, 20.0), value.as_str(), TextStyle::Body, fg, Align::Start);
        }
        y += list.h + 24.0;
    }
    ui.cmds.push(Cmd::Unclip);
}

// ------------------------------------------------------------------------
// Toast and tooltip
// ------------------------------------------------------------------------

/// An `AdwToast`: a pill with its close button, 24 px above the bottom.
fn toast(app: &App, ui: &mut Ui) {
    let Some(toast) = &app.toast else { return };
    let max_w = (app.width() - 24.0).max(120.0);
    // Padding 12 + label margin 6, then 6 + spacing 6 + button 34 + padding 6.
    let tw = ui.text_width(&toast.text, TextStyle::Body).min(max_w - 70.0);
    let (w, h) = (tw + 70.0, 46.0);
    let bottom = app.height() - 24.0 - if app.edit.is_some() { TOOLBAR_H } else { 0.0 };
    let lift = (1.0 - app.toast_alpha()) * 12.0;
    let r = Rect::new(((app.width() - w) / 2.0).round(), bottom - h + lift, w, h);
    ui.alpha = app.toast_alpha();
    ui.box_shadow(r, h / 2.0, TOAST_SHADOW);
    ui.fill(r, h / 2.0, Color::hex(0x505053));
    ui.widget(r, Action::None, None);
    ui.text(Rect::new(r.x + 18.0, r.y, tw, h), toast.text.as_str(), TextStyle::Body, Color::WHITE, Align::Start);
    let close = Rect::new(r.right() - 6.0 - BTN, r.y + 6.0, BTN, BTN);
    ui.radius = BTN / 2.0;
    ui.button(close, Action::CloseToast, Content::Icon(Icon::WindowClose), Style::Flat, Color::WHITE, true, false, Some("Dismiss"));
    ui.radius = 9.0;
    ui.alpha = 1.0;
}

fn tooltip(app: &App, ui: &mut Ui) {
    if !app.tooltip_ready() {
        return;
    }
    let (mx, my) = app.mouse;
    let Some((text, anchor)) = ui.widgets.iter().rev().find(|w| w.rect.contains(mx, my)).and_then(|w| Some((w.tooltip.clone()?, w.rect))) else {
        return;
    };
    let (tw, th) = ui.m.measure(&text, TextStyle::Body, 360.0, false);
    let (w, h) = (tw + 20.0, th + 12.0);
    let mut x = (anchor.cx() - w / 2.0).clamp(6.0, (app.width() - w - 6.0).max(6.0));
    let mut y = anchor.bottom() + 6.0;
    if y + h > app.height() - 6.0 {
        y = anchor.y - 6.0 - h;
    }
    x = x.round();
    y = y.round();
    let r = Rect::new(x, y, w, h);
    ui.fill(r, 9.0, Color::rgba(0, 0, 6, 0.8));
    ui.stroke(r.inset(0.5), 8.5, Color::WHITE.alpha(0.1), 1.0);
    ui.text(r, text, TextStyle::Body, Color::WHITE, Align::Centre);
}

// ------------------------------------------------------------------------
// Popover menus
// ------------------------------------------------------------------------

enum Item<'s> {
    Entry { action: Action, label: String, accel: Option<&'s str>, check: Option<bool>, enabled: bool },
    Separator,
}

fn entry<'s>(action: Action, label: impl Into<String>, accel: Option<&'s str>) -> Item<'s> {
    Item::Entry { action, label: label.into(), accel, check: None, enabled: true }
}

/// GTK's popover arrow: 24 wide at the base, 12 tall.
const TAIL_W: f32 = 24.0;
const TAIL_H: f32 = 12.0;

fn popover(app: &App, ui: &mut Ui, p: Popover, anchors: &Anchors) {
    const DIGITS: [&str; 9] = ["1", "2", "3", "4", "5", "6", "7", "8", "9"];
    let (anchor, items, above): (Option<Rect>, Vec<Item>, bool) = match p {
        // App-wide actions, then the HIG's standard items.
        Popover::Main => (
            anchors.main,
            vec![
                Item::Entry {
                    action: Action::OpenOnly,
                    label: "Open Image Only…".into(),
                    accel: Some("Ctrl+Shift+O"),
                    check: None,
                    enabled: app.edit.is_none(),
                },
                Item::Entry {
                    action: Action::ShowFolder,
                    label: "Show All Images in Folder".into(),
                    accel: Some("Ctrl+B"),
                    check: None,
                    enabled: app.selection && app.current().is_some() && app.edit.is_none(),
                },
                Item::Separator,
                entry(Action::Preferences, "Preferences", Some("Ctrl+,")),
                entry(Action::Shortcuts, "Keyboard Shortcuts", Some("Ctrl+?")),
                entry(Action::About, "About Windmolen", None),
            ],
            false,
        ),
        Popover::Marks => {
            let bits = app.current_marks();
            let has = app.current().is_some();
            let mut v: Vec<Item> = (0..SLOTS)
                .map(|s| {
                    let label = app.mark_label(s).unwrap_or_else(|| format!("Set Up Mark {}…", s + 1));
                    Item::Entry {
                        action: Action::ToggleMark(s as u8),
                        label,
                        accel: Some(DIGITS[s]),
                        check: Some(bits & (1 << s) != 0),
                        enabled: has,
                    }
                })
                .collect();
            v.push(Item::Separator);
            v.push(Item::Entry {
                action: Action::SendMarked,
                label: "Send Marked Files…".into(),
                accel: Some("Ctrl+M"),
                check: None,
                enabled: !app.busy() && app.settings.counts().iter().any(|&c| c > 0),
            });
            v.push(entry(Action::ManageMarks, "Manage Marks…", None));
            (anchors.marks, v, false)
        }
        Popover::Aspect => {
            let cur = app.edit.as_ref().map_or(Aspect::Free, |e| e.aspect);
            let v = Aspect::ALL
                .iter()
                .map(|&a| Item::Entry { action: Action::SetAspect(a), label: a.label(), accel: None, check: Some(a == cur), enabled: true })
                .collect();
            (anchors.aspect, v, true)
        }
        Popover::Save => {
            let replace = app.loaded().is_some_and(|l| l.meta.can_encode) && !app.busy();
            (
                anchors.save,
                vec![
                    entry(Action::EditSave, "Save Copy", Some("Ctrl+S")),
                    Item::Entry { action: Action::ReplaceOriginal, label: "Replace Original…".into(), accel: None, check: None, enabled: replace },
                ],
                false,
            )
        }
    };
    let Some(anchor) = anchor else { return };
    let t = ui.t;
    let full = Rect::new(0.0, 0.0, app.width(), app.height());
    ui.modal(None, full, Action::Dismiss);

    let fg = t.window_fg;
    let width = items
        .iter()
        .map(|i| match i {
            Item::Entry { label, accel, check, .. } => {
                ui.text_width(label, TextStyle::Body)
                    + accel.map_or(0.0, |a| ui.text_width(a, TextStyle::Body) + 12.0)
                    + if check.is_some() { 20.0 } else { 0.0 }
                    + 24.0
            }
            Item::Separator => 0.0,
        })
        .fold(160.0f32, f32::max)
        + 12.0;
    let height = 12.0 + items.iter().map(|i| if matches!(i, Item::Separator) { 13.0 } else { MENU_ROW }).sum::<f32>();
    // Centred on the button, kept inside the window.
    let x = (anchor.cx() - width / 2.0).min(app.width() - width - 6.0).max(6.0).round();
    let y = if above { anchor.y - 2.0 - TAIL_H - height } else { anchor.bottom() + 2.0 + TAIL_H };
    let r = Rect::new(x, y.max(6.0), width, height);
    let tx = anchor.cx().clamp(r.x + 15.0 + TAIL_W / 2.0, r.right() - 15.0 - TAIL_W / 2.0);
    let (base, tip) = if above { (r.bottom(), r.bottom() + TAIL_H) } else { (r.y, r.y - TAIL_H) };
    let tail = |grow: f32| {
        let d = if above { grow } else { -grow };
        vec![(tx - TAIL_W / 2.0 - grow, base), (tx, tip + d), (tx + TAIL_W / 2.0 + grow, base)]
    };

    ui.box_shadow(r, 15.0, POPOVER_SHADOW);
    ui.cmds.push(Cmd::Polygon { points: tail(1.0), c: Color::BLACK.alpha(0.05 * ui.alpha) });
    ui.fill(r, 15.0, t.popover_bg);
    ui.cmds.push(Cmd::Polygon { points: tail(0.0), c: t.popover_bg.alpha(ui.alpha) });
    ui.widget(r, Action::None, None);
    let mut iy = r.y + 6.0;
    for item in items {
        match item {
            Item::Entry { action, label, accel, check, enabled } => {
                ui.menu_item(Rect::new(r.x + 6.0, iy, r.w - 12.0, MENU_ROW), action, &label, accel, check, enabled, fg);
                iy += MENU_ROW;
            }
            Item::Separator => {
                ui.hline(r.x + 6.0, r.right() - 6.0, iy + 6.0, fg.alpha(BORDER_OPACITY));
                iy += 13.0;
            }
        }
    }
}

// ------------------------------------------------------------------------
// Dialogs
// ------------------------------------------------------------------------

struct Frame {
    content: Rect,
    scroll: f32,
}

fn is_alert(d: Dialog) -> bool {
    matches!(d, Dialog::Send | Dialog::Replace)
}

/// An `AdwDialog` (or `AdwAlertDialog`): dimmed backdrop, floating sheet,
/// and a header bar with a close button.
fn dialog_frame(app: &App, ui: &mut Ui, width: f32, content_h: f32, title: Option<&str>, alert: bool) -> Frame {
    let t = ui.t;
    let full = Rect::new(0.0, 0.0, app.width(), app.height());
    ui.modal(Some(t.dimming), full, if alert { Action::None } else { Action::Dismiss });
    let header = if alert { 0.0 } else { HEADER_H };
    let radius = if alert { 18.0 } else { 15.0 };
    let w = width.min(app.width() - 24.0);
    let h = (header + content_h).min(app.height() - 24.0);
    let r = full.centred(w, h).round();
    ui.box_shadow(r, radius, DIALOG_SHADOW);
    ui.fill(r, radius, t.dialog_bg);
    ui.stroke(r.inset(0.5), radius - 0.5, Color::WHITE.alpha(0.07), 1.0);
    ui.widget(r, Action::None, None);
    if !alert {
        let fg = t.window_fg;
        if let Some(title) = title {
            ui.text(Rect::new(r.x + 48.0, r.y, r.w - 96.0, HEADER_H), title, TextStyle::Heading, fg, Align::Centre);
        }
        let b = Rect::new(r.right() - GAP - BTN, r.y + (HEADER_H - BTN) / 2.0, BTN, BTN);
        ui.button(b, Action::Dismiss, Content::Icon(Icon::WindowClose), Style::Control, fg, true, false, Some("Close"));
    }
    let content = Rect::new(r.x, r.y + header, r.w, h - header);
    let scroll = app.scroll.min((content_h - content.h).max(0.0));
    Frame { content, scroll }
}

pub fn dialog_max_scroll(app: &App) -> f32 {
    let Some(d) = app.dialog else { return 0.0 };
    let (_, h) = dialog_size(app, d);
    let header = if is_alert(d) { 0.0 } else { HEADER_H };
    (header + h - (app.height() - 24.0)).max(0.0)
}

const SHORTCUTS: &[(&str, &[(&str, &str)])] = &[
    (
        "General",
        &[
            ("Open", "Ctrl+O"),
            ("Open image only", "Ctrl+Shift+O"),
            ("Show all images in folder", "Ctrl+B"),
            ("Main menu", "F10"),
            ("Preferences", "Ctrl+,"),
            ("Keyboard shortcuts", "Ctrl+?"),
            ("Properties", "F9 / I"),
            ("Fullscreen", "F11"),
            ("Close window", "Ctrl+Q"),
        ],
    ),
    ("Browsing", &[("Next image", "→ / D / PgDn"), ("Previous image", "← / A / PgUp"), ("First / last image", "Home / End")]),
    (
        "Zoom",
        &[
            ("Zoom in / out", "+ / − / Wheel"),
            ("Best fit", "F / Ctrl+0"),
            ("Original size", "0 / Ctrl+1"),
            ("Toggle best fit", "Double-click"),
            ("Move around", "Drag / Arrows"),
        ],
    ),
    ("Marks", &[("Toggle mark", "1 – 9"), ("Marks menu", "M"), ("Remove marks from image", "U"), ("Send marked files", "Ctrl+M")]),
    (
        "Editing",
        &[
            ("Edit image", "E"),
            ("Rotate right / left", "R / Shift+R"),
            ("Flip horizontally / vertically", "H / V"),
            ("Save copy", "Ctrl+S"),
            ("Cancel", "Esc"),
        ],
    ),
];

/// `.shortcut-row`: padding 9 around a 36 px line.
const SHORTCUT_ROW: f32 = 54.0;

fn alert_body(app: &App, d: Dialog) -> (String, String) {
    match d {
        Dialog::Send => {
            let summary = app.send_summary();
            if summary.is_empty() {
                (
                    "Nothing to Send".into(),
                    "No marked images have a destination folder. Press 1 – 9 to mark images, and choose folders in Manage Marks.".into(),
                )
            } else {
                let lines: Vec<String> = summary
                    .iter()
                    .map(|(slot, _, n)| {
                        let label = app.mark_label(*slot).unwrap_or_default();
                        format!("{n} {} → {label}", if *n == 1 { "file" } else { "files" })
                    })
                    .collect();
                ("Send Marked Files?".into(), format!("{}\n\nMarks are removed once their files have been sent.", lines.join("\n")))
            }
        }
        Dialog::Replace => {
            let name = app.current().map(file_name).unwrap_or_default();
            ("Replace Original?".into(), format!("“{name}” will be overwritten with the edited image. This can't be undone."))
        }
        _ => (String::new(), String::new()),
    }
}

/// What sits at the end of a preferences row.
enum Suffix {
    Button { label: &'static str, action: Action, style: Style, enabled: bool },
    Switch { action: Action, on: bool },
}

struct PrefRow {
    title: &'static str,
    subtitle: String,
    suffix: Suffix,
}

fn preferences(app: &App) -> Vec<(&'static str, Vec<PrefRow>)> {
    let status = match &app.update {
        crate::app::UpdateState::Checking { .. } => "Checking…".to_owned(),
        crate::app::UpdateState::Ready(v) => format!("Windmolen {v} is ready; restart to use it"),
        crate::app::UpdateState::Idle => concat!("This is version ", env!("CARGO_PKG_VERSION")).to_owned(),
    };
    let updates = vec![
        PrefRow {
            title: "Automatic Updates",
            subtitle: "Checks GitHub once a day and installs new versions".into(),
            suffix: Suffix::Switch { action: Action::ToggleAutoUpdate, on: app.settings.auto_update },
        },
        PrefRow {
            title: "Check for Updates",
            subtitle: status,
            suffix: Suffix::Button {
                label: "Check Now",
                action: Action::CheckUpdates,
                style: Style::Normal,
                enabled: matches!(app.update, crate::app::UpdateState::Idle),
            },
        },
    ];
    vec![("Updates", updates)]
}

/// Preferences page width and its lists' inset (`AdwPreferencesPage`).
const PREFS_W: f32 = 560.0;
const PAGE_MARGIN: f32 = 24.0;

fn suffix_width(m: &dyn crate::ui::Measure, s: &Suffix) -> f32 {
    match s {
        Suffix::Button { label, .. } => m.measure(label, TextStyle::Heading, f32::MAX, false).0 + 34.0,
        Suffix::Switch { .. } => 46.0,
    }
}

/// Row height and subtitle height of a preferences row `w` wide.
fn pref_row_height(m: &dyn crate::ui::Measure, row: &PrefRow, w: f32) -> (f32, f32) {
    let text_w = pref_text_width(m, row, w);
    let (_, th) = m.measure(row.title, TextStyle::Body, text_w, true);
    let (_, sh) = m.measure(&row.subtitle, TextStyle::Caption, text_w, true);
    ((2.0 + 6.0 + th + 3.0 + sh + 6.0 + 2.0).max(ROW_MIN), sh)
}

/// Room for a row's title and subtitle, before the 6 px suffix spacing.
fn pref_text_width(m: &dyn crate::ui::Measure, row: &PrefRow, w: f32) -> f32 {
    w - 2.0 * ROW_INSET - suffix_width(m, &row.suffix) - 6.0
}

/// (width, content height) of each dialog.
fn dialog_size(app: &App, d: Dialog) -> (f32, f32) {
    let m = &app.r.text;
    use crate::ui::Measure;
    match d {
        Dialog::Send | Dialog::Replace => {
            let w = 372.0f32.min(app.width() - 24.0);
            let (head, body) = alert_body(app, d);
            let (_, hh) = m.measure(&head, TextStyle::Title2, w - 48.0, true);
            let (_, bh) = m.measure(&body, TextStyle::Body, w - 48.0, true);
            (w, ALERT_TOP + hh + 10.0 + bh + ALERT_GAP + ALERT_BUTTON + 24.0)
        }
        Dialog::Preferences => {
            let w = PREFS_W.min(app.width() - 24.0);
            let lw = w - 2.0 * PAGE_MARGIN;
            let groups = preferences(app);
            let lists: f32 = groups.iter().map(|(_, rows)| GROUP_HEAD + rows.iter().map(|r| pref_row_height(m, r, lw).0).sum::<f32>()).sum();
            (w, PAGE_MARGIN + lists + 24.0 * (groups.len() as f32 - 1.0) + PAGE_MARGIN)
        }
        Dialog::Shortcuts => {
            (480.0, 12.0 + SHORTCUTS.iter().map(|(_, rows)| GROUP_HEAD + rows.len() as f32 * SHORTCUT_ROW + 18.0).sum::<f32>() - 18.0 + 24.0)
        }
        Dialog::About => (360.0, ABOUT_H),
        Dialog::ManageMarks => {
            let w = 520.0f32.min(app.width() - 24.0);
            let (_, ph) = m.measure(MARKS_INTRO, TextStyle::Body, w - 2.0 * PAGE_MARGIN, true);
            (w, 6.0 + ph + 18.0 + SLOTS as f32 * ROW_H + PAGE_MARGIN)
        }
    }
}

/// `AdwAlertDialog`: message padding 32 above, then 9 + 12 before the
/// 44 px responses.
const ALERT_TOP: f32 = 32.0;
const ALERT_GAP: f32 = 21.0;
const ALERT_BUTTON: f32 = 44.0;

/// `AdwAboutDialog`'s main page: icon, name, developer, version, links.
const ABOUT_H: f32 = 12.0 + 128.0 + 6.0 + 34.0 + 6.0 + 20.0 + 9.0 + 30.0 + 24.0 + ROW_MIN + 18.0 + ROW_MIN + 18.0;

const MARKS_INTRO: &str =
    "Press 1 – 9 to mark the current image. Each mark sends its files to the folder chosen here; an image can carry several marks.";

/// An activatable row's hover and press highlight, clipped to its list.
fn row_highlight(ui: &mut Ui, r: Rect, list: Rect, action: Action, fg: Color) {
    let a = match (ui.hover == Some(action), ui.pressed == Some(action)) {
        (true, true) => 0.08,
        (true, false) => 0.04,
        _ => return,
    };
    let top = if r.y <= list.y { 12.0 } else { 0.0 };
    let bottom = if r.bottom() >= list.bottom() { 12.0 } else { 0.0 };
    // Round only the list's own corners.
    ui.cmds.push(Cmd::Clip(r));
    let grown = Rect::from_edges(r.x, r.y - (12.0 - top), r.right(), r.bottom() + (12.0 - bottom));
    ui.fill(grown, 12.0, fg.alpha(a));
    ui.cmds.push(Cmd::Unclip);
}

/// An `AdwLinkRow`: the title and an external-link icon.
fn link_row(ui: &mut Ui, r: Rect, list: Rect, title: &str, action: Action, url: &str, fg: Color) {
    row_highlight(ui, r, list, action, fg);
    ui.text(Rect::new(r.x + ROW_INSET, r.y, r.w - 2.0 * ROW_INSET - 22.0, r.h), title, TextStyle::Body, fg, Align::Start);
    ui.icon(Icon::ExternalLink, Rect::new(r.right() - ROW_INSET - 16.0, r.cy() - 8.0, 16.0, 16.0), fg);
    focus_row(ui, r, action);
    ui.widgets.push(crate::ui::Widget { rect: r, action, tooltip: Some(url.into()), enabled: true, focusable: true });
}

fn focus_row(ui: &mut Ui, r: Rect, action: Action) {
    if ui.focus == Some(action) {
        ui.stroke(r.inset(1.0), 8.0, ui.t.accent.alpha(0.5), 2.0);
    }
}

fn dialog(app: &App, ui: &mut Ui, d: Dialog) {
    let t = ui.t;
    let fg = t.window_fg;
    let (width, content_h) = dialog_size(app, d);
    match d {
        Dialog::Send | Dialog::Replace => {
            let f = dialog_frame(app, ui, width, content_h, None, true);
            let r = f.content;
            let (head, body) = alert_body(app, d);
            let x = r.x + 24.0;
            let w = r.w - 48.0;
            let mut y = r.y + ALERT_TOP;
            y += ui.paragraph(x, y, w, &head, TextStyle::Title2, fg, Align::Centre) + 10.0;
            y += ui.paragraph(x, y, w, &body, TextStyle::Body, fg, Align::Centre) + ALERT_GAP;
            // Cancel stays neutral: only the response that destroys data is red.
            let buttons: Vec<(&str, Action, Style)> = match d {
                Dialog::Send if app.send_summary().is_empty() => vec![("Close", Action::Dismiss, Style::Normal)],
                Dialog::Send => {
                    vec![
                        ("Cancel", Action::Dismiss, Style::Normal),
                        ("Move", Action::SendMove, Style::Normal),
                        ("Copy", Action::SendCopy, Style::Suggested),
                    ]
                }
                _ => vec![("Cancel", Action::Dismiss, Style::Normal), ("Replace", Action::ConfirmReplace, Style::Destructive)],
            };
            let n = buttons.len() as f32;
            let bw = (w - 12.0 * (n - 1.0)) / n;
            ui.radius = 12.0;
            for (i, (label, action, style)) in buttons.into_iter().enumerate() {
                let b = Rect::new(x + i as f32 * (bw + 12.0), y.min(r.bottom() - 24.0 - ALERT_BUTTON), bw, ALERT_BUTTON);
                ui.button(b, action, Content::Label(label), style, fg, true, false, None);
            }
            ui.radius = 9.0;
        }
        Dialog::Preferences => {
            let f = dialog_frame(app, ui, width, content_h, Some("Preferences"), false);
            ui.cmds.push(Cmd::Clip(f.content));
            let x = f.content.x + PAGE_MARGIN;
            let w = f.content.w - 2.0 * PAGE_MARGIN;
            let mut y = f.content.y + PAGE_MARGIN - f.scroll;
            for (title, rows) in preferences(app) {
                group_title(ui, x, y, w, title, fg);
                y += GROUP_HEAD;
                let heights: Vec<(f32, f32)> = rows.iter().map(|r| pref_row_height(&app.r.text, r, w)).collect();
                let list = Rect::new(x, y, w, heights.iter().map(|h| h.0).sum());
                boxed_list(ui, list, t.dialog_bg);
                for (i, (row, (rh, sh))) in rows.iter().zip(heights).enumerate() {
                    let rr = Rect::new(x, y, w, rh);
                    if i > 0 {
                        row_separator(ui, x, w, y);
                    }
                    let sw = suffix_width(&app.r.text, &row.suffix);
                    let text_w = pref_text_width(&app.r.text, row, w);
                    if let Suffix::Switch { action, .. } = row.suffix {
                        // An `AdwSwitchRow` toggles from anywhere on it.
                        row_highlight(ui, rr, list, action, fg);
                        focus_row(ui, rr, action);
                        ui.widgets.push(crate::ui::Widget { rect: rr, action, tooltip: None, enabled: true, focusable: true });
                    }
                    let (_, th) = ui.m.measure(row.title, TextStyle::Body, text_w, true);
                    let ty = rr.cy() - (th + 3.0 + sh) / 2.0;
                    ui.paragraph(x + ROW_INSET, ty, text_w, row.title, TextStyle::Body, fg, Align::Start);
                    ui.paragraph(x + ROW_INSET, ty + th + 3.0, text_w, &row.subtitle, TextStyle::Caption, fg.alpha(DIM_OPACITY), Align::Start);
                    let sx = x + w - ROW_INSET - sw;
                    match row.suffix {
                        Suffix::Button { label, action, style, enabled } => {
                            let b = Rect::new(sx, rr.cy() - BTN / 2.0, sw, BTN);
                            ui.button(b, action, Content::Label(label), style, fg, enabled, false, None);
                        }
                        Suffix::Switch { action, on } => ui.switch(sx, rr.cy() - 13.0, action, on, fg),
                    }
                    y += rh;
                }
                y += 24.0;
            }
            ui.cmds.push(Cmd::Unclip);
        }
        Dialog::Shortcuts => {
            let f = dialog_frame(app, ui, width, content_h, Some("Keyboard Shortcuts"), false);
            ui.cmds.push(Cmd::Clip(f.content));
            let x = f.content.x + PAGE_MARGIN;
            let w = f.content.w - 2.0 * PAGE_MARGIN;
            let mut y = f.content.y + 12.0 - f.scroll;
            for (section, rows) in SHORTCUTS {
                group_title(ui, x, y, w, section, fg);
                y += GROUP_HEAD;
                let list = Rect::new(x, y, w, rows.len() as f32 * SHORTCUT_ROW);
                boxed_list(ui, list, t.dialog_bg);
                for (i, (desc, keys)) in rows.iter().enumerate() {
                    let ry = y + i as f32 * SHORTCUT_ROW;
                    if i > 0 {
                        row_separator(ui, x, w, ry);
                    }
                    ui.text(Rect::new(x + 12.0, ry, w * 0.5, SHORTCUT_ROW), *desc, TextStyle::Body, fg, Align::Start);
                    keycaps(ui, keys, x + w - 12.0, ry + (SHORTCUT_ROW - KEYCAP_H) / 2.0, fg);
                }
                y += list.h + 18.0;
            }
            ui.cmds.push(Cmd::Unclip);
        }
        Dialog::About => {
            let f = dialog_frame(app, ui, width, content_h, None, false);
            ui.cmds.push(Cmd::Clip(f.content));
            let r = f.content;
            let mut y = r.y + 12.0 - f.scroll;
            ui.cmds.push(Cmd::AppIcon(Rect::new(r.cx() - 64.0, y, 128.0, 128.0)));
            y += 128.0 + 6.0;
            ui.text(Rect::new(r.x, y, r.w, 34.0), "Windmolen", TextStyle::Title1, fg, Align::Centre);
            y += 34.0 + 6.0;
            ui.text(Rect::new(r.x, y, r.w, 20.0), crate::release::OWNER, TextStyle::Body, fg, Align::Centre);
            y += 20.0 + 9.0;
            // `.app-version`: an accent-coloured pill.
            let ver = env!("CARGO_PKG_VERSION");
            let vw = ui.text_width(ver, TextStyle::Heading) + 36.0;
            let vr = Rect::new((r.cx() - vw / 2.0).round(), y, vw, 30.0);
            ui.fill(vr, 15.0, t.accent.alpha(0.1));
            ui.text(vr, ver, TextStyle::Heading, t.accent, Align::Centre);
            y += 30.0 + 24.0;
            let x = r.x + 12.0;
            let w = r.w - 24.0;
            for (title, action, url) in [
                ("Website", Action::OpenProject, crate::release::project_url()),
                ("Report an Issue", Action::ReportIssue, crate::release::issues_url()),
            ] {
                let list = Rect::new(x, y, w, ROW_MIN);
                boxed_list(ui, list, t.dialog_bg);
                link_row(ui, list, list, title, action, &url, fg);
                y += ROW_MIN + 18.0;
            }
            ui.cmds.push(Cmd::Unclip);
        }
        Dialog::ManageMarks => {
            let f = dialog_frame(app, ui, width, content_h, Some("Manage Marks"), false);
            ui.cmds.push(Cmd::Clip(f.content));
            let x = f.content.x + PAGE_MARGIN;
            let w = f.content.w - 2.0 * PAGE_MARGIN;
            let mut y = f.content.y + 6.0 - f.scroll;
            y += ui.paragraph(x, y, w, MARKS_INTRO, TextStyle::Body, fg.alpha(DIM_OPACITY), Align::Start) + 18.0;
            let list = Rect::new(x, y, w, SLOTS as f32 * ROW_H);
            boxed_list(ui, list, t.dialog_bg);
            let counts = app.settings.counts();
            for slot in 0..SLOTS {
                let ry = y + slot as f32 * ROW_H;
                if slot > 0 {
                    row_separator(ui, x, w, ry);
                }
                ui.fill(Rect::new(x + ROW_INSET, ry + ROW_H / 2.0 - 7.0, 14.0, 14.0), 7.0, ACCENTS[slot]);
                let dir = app.settings.mark_dirs[slot].as_deref();
                let mut right = x + w - ROW_INSET;
                let by = ry + (ROW_H - BTN) / 2.0;
                if dir.is_some() {
                    right -= BTN;
                    let b = Rect::new(right, by, BTN, BTN);
                    ui.button(b, Action::ClearMarkDir(slot as u8), Content::Icon(Icon::Remove), Style::Flat, fg, true, false, Some("Forget Folder"));
                    right -= GAP;
                }
                right -= BTN;
                let b = Rect::new(right, by, BTN, BTN);
                ui.button(b, Action::PickMarkDir(slot as u8), Content::Icon(Icon::Folder), Style::Flat, fg, true, false, Some("Choose Folder…"));
                right -= 6.0;
                if counts[slot] > 0 {
                    let s = format!("{} marked", counts[slot]);
                    let cw = ui.text_width(&s, TextStyle::Caption);
                    ui.text(Rect::new(right - cw, ry, cw + 1.0, ROW_H), s, TextStyle::Caption, fg.alpha(DIM_OPACITY), Align::End);
                    right -= cw + 6.0;
                }
                let tx = x + ROW_INSET + 14.0 + 6.0;
                let tw = (right - tx).max(0.0);
                let title = match app.mark_label(slot) {
                    Some(l) => format!("{}  ·  {l}", slot + 1),
                    None => format!("{}", slot + 1),
                };
                ui.text(Rect::new(tx, ry + 8.0, tw, 20.0), title, TextStyle::Body, fg, Align::Start);
                let sub = dir.map_or_else(|| "No folder chosen".to_owned(), |d| d.display().to_string());
                ui.text(Rect::new(tx, ry + 31.0, tw, 17.0), sub, TextStyle::Caption, fg.alpha(DIM_OPACITY), Align::Start);
            }
            ui.cmds.push(Cmd::Unclip);
        }
    }
}

/// `shortcut-label`: each key a keycap, alternatives split by a dimmed slash.
const KEYCAP_H: f32 = 30.0;

fn keycaps(ui: &mut Ui, keys: &str, right: f32, y: f32, fg: Color) {
    let t = ui.t;
    let mut x = right;
    let alternatives: Vec<&str> = keys.split(" / ").collect();
    for (i, alt) in alternatives.iter().enumerate().rev() {
        let parts: Vec<&str> = if alt.len() > 1 && alt.contains('+') && !alt.ends_with('+') { alt.split('+').collect() } else { vec![alt] };
        for part in parts.into_iter().rev() {
            let w = (ui.text_width(part, TextStyle::Body) + 12.0).max(32.0);
            x -= w;
            let r = Rect::new(x, y, w, KEYCAP_H);
            ui.fill(r, 6.0, fg.alpha(0.1));
            // inset 0 -2px card-shade: a lip along the bottom.
            ui.cmds.push(Cmd::Clip(Rect::new(r.x, r.bottom() - 2.0, r.w, 2.0)));
            ui.fill(r, 6.0, t.card_shade);
            ui.cmds.push(Cmd::Unclip);
            ui.text(r, part, TextStyle::Body, fg, Align::Centre);
            x -= 6.0;
        }
        if i > 0 {
            let sw = ui.text_width("/", TextStyle::Body);
            x -= sw;
            ui.text(Rect::new(x, y, sw + 1.0, KEYCAP_H), "/", TextStyle::Body, fg.alpha(DIM_OPACITY), Align::Start);
            x -= 6.0;
        }
    }
}
