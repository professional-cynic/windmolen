//! Layout of every surface, following libadwaita's widgets and metrics:
//! header bar, status pages, OSD controls, boxed lists, popover menus,
//! dialogs and toasts.

use std::f32::consts::TAU;
use std::path::Path;

use crate::app::{App, Dialog, HEADER_H, Popover, SIDEBAR_W, TOOLBAR_H};
use crate::edit::{Aspect, Handle};
use crate::geom::{Affine, Rect};
use crate::icons::Icon;
use crate::meta;
use crate::settings::SLOTS;
use crate::sys;
use crate::theme::{ACCENTS, BORDER_OPACITY, Color, DIM_OPACITY};
use crate::ui::{Action, Align, Cmd, Content, Style, TextStyle, Ui};

const BTN: f32 = 34.0;
const GAP: f32 = 6.0;
const ROW_H: f32 = 58.0;
const MENU_ROW: f32 = 34.0;

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
    let (title, subtitle) = if let Some(e) = &app.edit {
        let label = "Cancel";
        let bw = ui.text_width(label, TextStyle::Body) + 34.0;
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
        let enabled = !app.busy;
        let k = if enabled { 1.0 } else { 0.5 };
        ui.fill(whole, 6.0, t.accent_bg.alpha(k));
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
                ui.fill(whole, 6.0, c);
                ui.cmds.push(Cmd::Unclip);
            }
            if app.focus == Some(action) {
                ui.stroke(part.inset(1.0), 5.0, t.accent_fg.alpha(0.6), 2.0);
            }
            ui.widgets.push(crate::ui::Widget { rect: part, action, tooltip: None, enabled, focusable: true });
        }
        ui.text(main, label, TextStyle::Heading, t.accent_fg.alpha(k), Align::Centre);
        ui.vline(arrow.x, y + 8.0, y + BTN - 8.0, t.accent_fg.alpha(0.3 * k));
        ui.icon(Icon::PanDown, arrow.centred(16.0, 16.0), t.accent_fg.alpha(k));
        anchors.save = Some(arrow);
        right -= GAP;
        let _ = e;
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
            (
                Action::MarksMenu,
                if marked { Icon::BookmarkFilled } else { Icon::Bookmark },
                true,
                app.popover == Some(Popover::Marks),
                "Marks (M)",
            ),
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
            Some(p) => (file_name(p), Some(format!("{} of {}", app.index + 1, app.files.len()))),
            None => ("Het Plantsoen".to_owned(), None),
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

// ------------------------------------------------------------------------
// Canvas
// ------------------------------------------------------------------------

fn canvas(app: &App, ui: &mut Ui) {
    let t = ui.t;
    let vp = app.viewport();
    if let (Some(l), Some(o)) = (app.loaded(), app.orient()) {
        let v = app.effective_view();
        let ds = app.dpi_scale();
        let k = l.meta.width as f32 / l.bw as f32;
        let snap = |x: f32| (x * ds).round() / ds;
        let m = o
            .matrix(l.bw as f32, l.bh as f32)
            .then(Affine::scale(k * v.s, k * v.s))
            .then(Affine::translate(snap(vp.cx() - v.cx * v.s), snap(vp.cy() - v.cy * v.s)));
        ui.cmds.push(Cmd::Clip(vp));
        ui.cmds.push(Cmd::Image { m, smooth: v.s * ds < 2.0 });
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
        status_page(
            ui,
            vp,
            Icon::Image,
            "No Image Open",
            "Open an image or drag one onto this window to start browsing its folder",
            Some(("Open Image…", Action::Open)),
        );
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
    let lw = ui.text_width("Best Fit", TextStyle::Numeric).max(ui.text_width(&label, TextStyle::Numeric)) + 28.0;
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
    let k = l.meta.width as f32 / l.bw as f32;
    let m = o.matrix(l.bw as f32, l.bh as f32).then(Affine::scale(k * s, k * s)).then(Affine::translate(r.x, r.y));
    ui.cmds.push(Cmd::Image { m, smooth: true });

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
    for (x, y, sx, sy) in [(c.x + i, c.y + i, 1.0, 1.0), (c.right() - i, c.y + i, -1.0, 1.0), (c.x + i, c.bottom() - i, 1.0, -1.0), (c.right() - i, c.bottom() - i, -1.0, -1.0)] {
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
    let aw = ui.text_width(&aspect, TextStyle::Body) + 16.0 + 6.0 + 24.0;
    let reset = "Reset";
    let rw = ui.text_width(reset, TextStyle::Body) + 34.0;
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
    if let Some(t) = m.modified {
        file.push(("Modified".into(), sys::format_time(t)));
    }

    let (w, h) = m.orient.apply_size(m.width, m.height);
    let mp = f64::from(w) * f64::from(h) / 1e6;
    let mut image = vec![("Dimensions".to_owned(), format!("{w} × {h} ({mp:.1} MP)"))];
    if let Some(prof) = &m.profile {
        image.push(("Colour Profile".into(), prof.clone()));
    }
    let bits = app.current_marks();
    if bits != 0 {
        let names: Vec<String> = (0..SLOTS).filter(|s| bits & (1 << s) != 0).map(|s| app.mark_label(s).unwrap_or_else(|| format!("{}", s + 1))).collect();
        image.push(("Marks".into(), names.join(", ")));
    }

    let mut groups = vec![("File", file), ("Image", image)];
    if !m.exif.is_empty() {
        groups.push(("Camera", m.exif.iter().map(|(k, v)| ((*k).to_owned(), v.clone())).collect()));
    }
    groups
}

fn groups_height(groups: &[Group]) -> f32 {
    groups.iter().map(|(_, rows)| 30.0 + rows.len() as f32 * ROW_H + 24.0).sum::<f32>() + 12.0
}

pub fn sidebar_max_scroll(app: &App) -> f32 {
    let visible = app.height() - app.header_h();
    (groups_height(&properties(app)) - visible).max(0.0)
}

fn boxed_list(ui: &mut Ui, r: Rect) {
    let t = ui.t;
    ui.shadow(r, 12.0, 0.6);
    ui.fill(r, 12.0, if t.dark { t.sidebar_bg } else { Color::WHITE });
    ui.fill(r, 12.0, t.card_bg);
    ui.stroke(r.inset(0.5), 11.5, t.card_shade, 1.0);
}

fn sidebar(app: &App, ui: &mut Ui) {
    let t = ui.t;
    let top = app.header_h();
    let area = Rect::new(app.width() - SIDEBAR_W, top, SIDEBAR_W, app.height() - top);
    ui.fill(area, 0.0, t.sidebar_bg);
    ui.vline(area.x - 1.0, top, area.bottom(), t.headerbar_shade);
    ui.widget(area, Action::None, None);
    let groups = properties(app);
    let scroll = app.props_scroll.min(sidebar_max_scroll(app));
    ui.cmds.push(Cmd::Clip(area));
    let fg = t.window_fg;
    let x = area.x + 12.0;
    let w = SIDEBAR_W - 24.0;
    let mut y = top + 12.0 - scroll;
    for (title, rows) in groups {
        ui.text(Rect::new(x + 6.0, y, w, 24.0), title, TextStyle::Heading, fg, Align::Start);
        y += 30.0;
        let list = Rect::new(x, y, w, rows.len() as f32 * ROW_H);
        boxed_list(ui, list);
        for (i, (label, value)) in rows.iter().enumerate() {
            let ry = y + i as f32 * ROW_H;
            if i > 0 {
                ui.hline(x, x + w, ry - 0.5, fg.alpha(BORDER_OPACITY));
            }
            ui.text(Rect::new(x + 12.0, ry + 9.0, w - 24.0, 18.0), label.as_str(), TextStyle::Caption, fg.alpha(DIM_OPACITY), Align::Start);
            ui.text(Rect::new(x + 12.0, ry + 27.0, w - 24.0, 22.0), value.as_str(), TextStyle::Body, fg, Align::Start);
        }
        y += list.h + 24.0;
    }
    ui.cmds.push(Cmd::Unclip);
}

// ------------------------------------------------------------------------
// Toast and tooltip
// ------------------------------------------------------------------------

fn toast(app: &App, ui: &mut Ui) {
    let Some(toast) = &app.toast else { return };
    let t = ui.t;
    let max_w = (app.width() - 48.0).max(100.0);
    let tw = ui.text_width(&toast.text, TextStyle::Body).min(max_w - 36.0);
    let r = Rect::new((app.width() - tw - 36.0) / 2.0, 0.0, tw + 36.0, 40.0);
    let bottom = app.height() - 12.0 - if app.edit.is_some() { TOOLBAR_H } else { 0.0 };
    let lift = (1.0 - app.toast_alpha()) * 12.0;
    let r = Rect::new(r.x, bottom - r.h + lift, r.w, r.h);
    ui.alpha = app.toast_alpha();
    ui.shadow(r, 20.0, 1.0);
    ui.fill(r, 20.0, Color::rgba(0, 0, 6, 0.85));
    ui.text(Rect::new(r.x + 18.0, r.y, tw, r.h), toast.text.as_str(), TextStyle::Body, t.osd_fg, Align::Centre);
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
    let tw = ui.text_width(&text, TextStyle::Caption);
    let (w, h) = (tw + 20.0, 28.0);
    let mut x = (anchor.cx() - w / 2.0).clamp(6.0, (app.width() - w - 6.0).max(6.0));
    let mut y = anchor.bottom() + 6.0;
    if y + h > app.height() - 6.0 {
        y = anchor.y - 6.0 - h;
    }
    x = x.round();
    y = y.round();
    let r = Rect::new(x, y, w, h);
    ui.fill(r, 6.0, Color::rgba(0, 0, 6, 0.8));
    ui.text(r, text, TextStyle::Caption, Color::WHITE, Align::Centre);
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

fn popover(app: &App, ui: &mut Ui, p: Popover, anchors: &Anchors) {
    const DIGITS: [&str; 9] = ["1", "2", "3", "4", "5", "6", "7", "8", "9"];
    let can_send = !app.busy && app.settings.counts().iter().any(|&c| c > 0);
    let (anchor, items, above): (Option<Rect>, Vec<Item>, bool) = match p {
        Popover::Main => (
            anchors.main,
            vec![
                entry(Action::Open, "Open…", Some("Ctrl+O")),
                Item::Separator,
                Item::Entry { action: Action::SendMarked, label: "Send Marked Files…".into(), accel: Some("Ctrl+M"), check: None, enabled: can_send },
                entry(Action::ManageMarks, "Manage Marks…", None),
                Item::Separator,
                entry(Action::Shortcuts, "Keyboard Shortcuts", Some("Ctrl+?")),
                entry(Action::About, "About Het Plantsoen", None),
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
            v.push(Item::Entry { action: Action::SendMarked, label: "Send Marked Files…".into(), accel: Some("Ctrl+M"), check: None, enabled: can_send });
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
            let replace = app.loaded().is_some_and(|l| l.meta.can_encode) && !app.busy;
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
                    + accel.map_or(0.0, |a| ui.text_width(a, TextStyle::Body) + 24.0)
                    + if check.is_some() { 24.0 } else { 0.0 }
                    + 24.0
            }
            Item::Separator => 0.0,
        })
        .fold(180.0f32, f32::max)
        + 12.0;
    let height = 12.0 + items.iter().map(|i| if matches!(i, Item::Separator) { 13.0 } else { MENU_ROW }).sum::<f32>();
    let x = (anchor.right() - width).max(6.0).min(app.width() - width - 6.0);
    let x = if anchor.cx() < app.width() / 2.0 { anchor.x.min(app.width() - width - 6.0).max(6.0) } else { x };
    let y = if above { anchor.y - 6.0 - height } else { anchor.bottom() + 6.0 };
    let r = Rect::new(x, y.max(6.0), width, height);

    ui.shadow(r, 12.0, 1.4);
    ui.fill(r, 12.0, t.popover_bg);
    if t.dark {
        ui.stroke(r.inset(0.5), 11.5, Color::WHITE.alpha(0.07), 1.0);
    }
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

/// An `AdwDialog`: dimmed backdrop, rounded sheet, header with close button.
fn dialog_frame(app: &App, ui: &mut Ui, width: f32, content_h: f32, title: Option<&str>, alert: bool) -> Frame {
    let t = ui.t;
    let full = Rect::new(0.0, 0.0, app.width(), app.height());
    ui.modal(Some(t.dimming), full, if alert { Action::None } else { Action::Dismiss });
    let header = if alert { 0.0 } else { HEADER_H };
    let w = width.min(app.width() - 24.0);
    let h = (header + content_h).min(app.height() - 24.0);
    let r = full.centred(w, h).round();
    ui.shadow(r, 12.0, 2.0);
    ui.fill(r, 12.0, t.dialog_bg);
    if t.dark {
        ui.stroke(r.inset(0.5), 11.5, Color::WHITE.alpha(0.07), 1.0);
    }
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
    let (w, h) = dialog_size(app, d);
    let header = if matches!(d, Dialog::Send | Dialog::Replace) { 0.0 } else { HEADER_H };
    let _ = w;
    (header + h - (app.height() - 24.0)).max(0.0)
}

const SHORTCUTS: &[(&str, &[(&str, &str)])] = &[
    ("General", &[
        ("Open", "Ctrl+O"),
        ("Main menu", "F10"),
        ("Keyboard shortcuts", "Ctrl+?"),
        ("Properties", "F9 / I"),
        ("Fullscreen", "F11"),
        ("Close window", "Ctrl+Q"),
    ]),
    ("Browsing", &[
        ("Next image", "→ / D / PgDn"),
        ("Previous image", "← / A / PgUp"),
        ("First / last image", "Home / End"),
    ]),
    ("Zoom", &[
        ("Zoom in / out", "+ / − / wheel"),
        ("Best fit", "F / Ctrl+0"),
        ("Original size", "0 / Ctrl+1"),
        ("Toggle best fit", "Double-click"),
        ("Move around", "Drag / arrows"),
    ]),
    ("Marks", &[
        ("Toggle mark", "1 – 9"),
        ("Marks menu", "M"),
        ("Remove marks from image", "U"),
        ("Send marked files", "Ctrl+M"),
    ]),
    ("Editing", &[
        ("Edit image", "E"),
        ("Rotate right / left", "R / Shift+R"),
        ("Flip horizontally / vertically", "H / V"),
        ("Save copy", "Ctrl+S"),
        ("Cancel", "Esc"),
    ]),
];

fn alert_body(app: &App, d: Dialog) -> (String, String) {
    match d {
        Dialog::Send => {
            let summary = app.send_summary();
            if summary.is_empty() {
                ("Nothing to Send".into(), "No marked images have a destination folder. Press 1 – 9 to mark images, and choose folders in Manage Marks.".into())
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
            (w, 32.0 + hh + 10.0 + bh + 24.0 + 40.0 + 24.0)
        }
        Dialog::Shortcuts => (480.0, SHORTCUTS.iter().map(|(_, rows)| 30.0 + rows.len() as f32 * 40.0 + 18.0).sum::<f32>() + 6.0),
        Dialog::About => (360.0, 272.0),
        Dialog::ManageMarks => {
            let w = 520.0f32.min(app.width() - 24.0);
            let (_, ph) = m.measure(MARKS_INTRO, TextStyle::Body, w - 48.0, true);
            (w, 6.0 + ph + 18.0 + SLOTS as f32 * ROW_H + 24.0)
        }
    }
}

const MARKS_INTRO: &str =
    "Press 1 – 9 to mark the current image. Each mark sends its files to the folder chosen here; an image can carry several marks.";

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
            let mut y = r.y + 32.0;
            y += ui.paragraph(x, y, w, &head, TextStyle::Title2, fg, Align::Centre) + 10.0;
            y += ui.paragraph(x, y, w, &body, TextStyle::Body, fg, Align::Centre) + 24.0;
            let buttons: Vec<(&str, Action, Style)> = match d {
                Dialog::Send if app.send_summary().is_empty() => vec![("Close", Action::Dismiss, Style::Normal)],
                Dialog::Send => {
                    vec![("Cancel", Action::Dismiss, Style::Normal), ("Move", Action::SendMove, Style::Normal), ("Copy", Action::SendCopy, Style::Suggested)]
                }
                _ => vec![("Cancel", Action::Dismiss, Style::Normal), ("Replace", Action::ConfirmReplace, Style::Destructive)],
            };
            let n = buttons.len() as f32;
            let bw = (w - 12.0 * (n - 1.0)) / n;
            for (i, (label, action, style)) in buttons.into_iter().enumerate() {
                let b = Rect::new(x + i as f32 * (bw + 12.0), y.min(r.bottom() - 64.0), bw, 40.0);
                ui.button(b, action, Content::Label(label), style, fg, true, false, None);
            }
        }
        Dialog::Shortcuts => {
            let f = dialog_frame(app, ui, width, content_h, Some("Keyboard Shortcuts"), false);
            ui.cmds.push(Cmd::Clip(f.content));
            let x = f.content.x + 18.0;
            let w = f.content.w - 36.0;
            let mut y = f.content.y + 6.0 - f.scroll;
            for (section, rows) in SHORTCUTS {
                ui.text(Rect::new(x + 6.0, y, w, 24.0), *section, TextStyle::Heading, fg, Align::Start);
                y += 30.0;
                let list = Rect::new(x, y, w, rows.len() as f32 * 40.0);
                boxed_list(ui, list);
                for (i, (desc, keys)) in rows.iter().enumerate() {
                    let ry = y + i as f32 * 40.0;
                    if i > 0 {
                        ui.hline(x, x + w, ry - 0.5, fg.alpha(BORDER_OPACITY));
                    }
                    ui.text(Rect::new(x + 12.0, ry, w * 0.55, 40.0), *desc, TextStyle::Body, fg, Align::Start);
                    keycaps(ui, keys, x + w - 12.0, ry + 8.0, fg);
                }
                y += list.h + 18.0;
            }
            ui.cmds.push(Cmd::Unclip);
        }
        Dialog::About => {
            let f = dialog_frame(app, ui, width, content_h, None, false);
            let r = f.content;
            let mut y = r.y;
            let icon = Rect::new(r.cx() - 48.0, y, 96.0, 96.0);
            ui.fill(icon, 24.0, t.accent_bg);
            ui.icon(Icon::Image, icon.centred(56.0, 56.0), t.accent_fg);
            y += 96.0 + 18.0;
            ui.text(Rect::new(r.x, y, r.w, 36.0), "Het Plantsoen", TextStyle::Title1, fg, Align::Centre);
            y += 40.0;
            ui.text(Rect::new(r.x, y, r.w, 20.0), concat!("Version ", env!("CARGO_PKG_VERSION")), TextStyle::Caption, fg.alpha(DIM_OPACITY), Align::Centre);
            y += 32.0;
            ui.paragraph(r.x + 24.0, y, r.w - 48.0, "A lean, fast photo viewer.\nBuilt in Rust on Direct2D and the Windows Imaging Component.", TextStyle::Body, fg, Align::Centre);
        }
        Dialog::ManageMarks => {
            let f = dialog_frame(app, ui, width, content_h, Some("Manage Marks"), false);
            ui.cmds.push(Cmd::Clip(f.content));
            let x = f.content.x + 24.0;
            let w = f.content.w - 48.0;
            let mut y = f.content.y + 6.0 - f.scroll;
            y += ui.paragraph(x, y, w, MARKS_INTRO, TextStyle::Body, fg.alpha(DIM_OPACITY), Align::Start) + 18.0;
            let list = Rect::new(x, y, w, SLOTS as f32 * ROW_H);
            boxed_list(ui, list);
            let counts = app.settings.counts();
            for slot in 0..SLOTS {
                let ry = y + slot as f32 * ROW_H;
                if slot > 0 {
                    ui.hline(x, x + w, ry - 0.5, fg.alpha(BORDER_OPACITY));
                }
                ui.fill(Rect::new(x + 14.0, ry + ROW_H / 2.0 - 7.0, 14.0, 14.0), 7.0, ACCENTS[slot]);
                let dir = app.settings.mark_dirs[slot].as_deref();
                let mut right = x + w - 12.0;
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
                right -= 12.0;
                if counts[slot] > 0 {
                    let s = format!("{} marked", counts[slot]);
                    let cw = ui.text_width(&s, TextStyle::Caption);
                    ui.text(Rect::new(right - cw, ry, cw + 1.0, ROW_H), s, TextStyle::Caption, fg.alpha(DIM_OPACITY), Align::End);
                    right -= cw + 12.0;
                }
                let tx = x + 40.0;
                let tw = (right - tx).max(0.0);
                let title = match app.mark_label(slot) {
                    Some(l) => format!("{}  ·  {l}", slot + 1),
                    None => format!("{}", slot + 1),
                };
                ui.text(Rect::new(tx, ry + 9.0, tw, 22.0), title, TextStyle::Body, fg, Align::Start);
                let sub = dir.map_or_else(|| "No folder chosen".to_owned(), |d| d.display().to_string());
                ui.text(Rect::new(tx, ry + 30.0, tw, 18.0), sub, TextStyle::Caption, fg.alpha(DIM_OPACITY), Align::Start);
            }
            ui.cmds.push(Cmd::Unclip);
        }
    }
}

fn keycaps(ui: &mut Ui, keys: &str, right: f32, y: f32, fg: Color) {
    let mut x = right;
    for part in keys.split(" / ").collect::<Vec<_>>().into_iter().rev() {
        let w = ui.text_width(part, TextStyle::Caption) + 14.0;
        x -= w;
        let r = Rect::new(x, y, w, 24.0);
        ui.fill(r, 6.0, fg.alpha(0.1));
        ui.text(r, part, TextStyle::Caption, fg, Align::Centre);
        x -= 6.0;
    }
}
