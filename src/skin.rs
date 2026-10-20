//! Widgets drawn after the official Bloody 8 skin. Colors and sizes come from
//! Data/Mouse/Forms/*/UI.txt and the four-state sprites beside them. The
//! project draws them itself and does not redistribute the official images.
use eframe::egui::{
    self, Align2, Color32, FontFamily, FontId, Pos2, Rect, Response, Sense, Stroke, StrokeKind, Ui,
    pos2, vec2,
};

const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

// PublicStyle.inf MainBackgroundColor, Dashboard advgb_Dashboard.
pub const BACKGROUND: Color32 = rgb(26, 27, 29);
pub const DASHBOARD: Color32 = rgb(18, 19, 21);
// KeySet advgb_Sheet; also the page background below the banner.
pub const SHEET: Color32 = rgb(37, 38, 42);
// Group panels inside a page, such as the CPI and report rate groups.
pub const PANEL: Color32 = rgb(48, 49, 53);
const LOGO: Color32 = rgb(37, 38, 40);
// lb_FunctionDescription on the orange description bar.
const BANNER_TEXT: Color32 = rgb(58, 58, 58);
const BANNER_TOP: Color32 = rgb(247, 173, 38);
const BANNER_BOTTOM: Color32 = rgb(222, 151, 30);
const TRACK: Color32 = rgb(158, 158, 158);
const TICK: Color32 = rgb(138, 138, 138);
pub const TEXT: Color32 = rgb(254, 254, 254);
pub const GRAY: Color32 = rgb(128, 128, 128);
pub const LIGHT_GRAY: Color32 = rgb(150, 150, 150);
pub const DISABLED: Color32 = rgb(138, 138, 138);
pub const CAPTION_RED: Color32 = rgb(255, 50, 50);
// Pressed sprite fill of SubTab.png and Button.png.
pub const RED: Color32 = rgb(180, 9, 17);
// SensitivityCPI SelectedCPIBackgroundColor and lb_CurrentCPIX color.
pub const SELECTED: Color32 = rgb(200, 0, 0);
pub const VALUE: Color32 = rgb(33, 33, 33);
// PublicItem lb_DongleNotExists.
pub const ERROR: Color32 = rgb(255, 0, 0);
const MAIN_HOVER: Color32 = rgb(52, 53, 57);
const MAIN_BAR: Color32 = rgb(179, 10, 13);
const PROFILE: Color32 = rgb(29, 30, 34);
// Oscar editor memo and gutter.
pub const MEMO: Color32 = rgb(200, 200, 200);
pub const GUTTER: Color32 = rgb(236, 233, 216);
pub const FIELD: Color32 = rgb(20, 20, 20);

pub fn bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("bold".into()))
}

pub fn regular(size: f32) -> FontId {
    FontId::proportional(size)
}

fn state(ui: &Ui, response: &Response) -> (bool, bool) {
    let enabled = ui.is_enabled() && response.sense.senses_click();
    (
        enabled,
        enabled && (response.hovered() || response.is_pointer_button_down_on()),
    )
}

/// Dashboard main tab, 174 × 48. Hover and selection add a red left edge.
pub fn main_tab(ui: &mut Ui, caption: &str, selected: bool, enabled: bool) -> Response {
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(vec2(174.0, 48.0), sense);
    let (enabled, active) = state(ui, &response);
    let lit = enabled && (selected || active);
    let painter = ui.painter();
    painter.rect_filled(rect, 0, if lit { MAIN_HOVER } else { DASHBOARD });
    if lit {
        painter.rect_filled(Rect::from_min_size(rect.min, vec2(3.0, 48.0)), 0, MAIN_BAR);
    }
    let color = match (enabled, lit) {
        (false, _) => DISABLED,
        (true, true) => CAPTION_RED,
        _ => TEXT,
    };
    painter.text(
        rect.left_center() + vec2(30.0, 0.0),
        Align2::LEFT_CENTER,
        caption,
        bold(19.0),
        color,
    );
    response
}

/// Dashboard sub tab, 198 × 48. Hover and selection fill it red.
pub fn sub_tab(ui: &mut Ui, caption: &str, selected: bool, enabled: bool) -> Response {
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(vec2(198.0, 48.0), sense);
    let (enabled, active) = state(ui, &response);
    let lit = enabled && (selected || active);
    let painter = ui.painter();
    painter.rect_filled(rect, 0, if lit { RED } else { DASHBOARD });
    let color = match (enabled, lit) {
        (false, _) => rgb(70, 70, 72),
        (true, true) => TEXT,
        _ => GRAY,
    };
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        caption,
        bold(19.0),
        color,
    );
    response
}

/// KeySet profile tab, 154 × 48. The selected tab takes the sheet color.
pub fn profile_tab(ui: &mut Ui, caption: &str, selected: bool, enabled: bool) -> Response {
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(vec2(154.0, 48.0), sense);
    let (enabled, active) = state(ui, &response);
    let fill = if selected {
        SHEET
    } else if active {
        rgb(33, 34, 38)
    } else {
        PROFILE
    };
    ui.painter().rect_filled(rect, 0, fill);
    let color = if enabled { TEXT } else { DISABLED };
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        caption,
        regular(19.0),
        color,
    );
    response
}

fn gradient(ui: &Ui, rect: Rect, top: Color32, bottom: Color32) {
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(rect.left_top(), top);
    mesh.colored_vertex(rect.right_top(), top);
    mesh.colored_vertex(rect.left_bottom(), bottom);
    mesh.colored_vertex(rect.right_bottom(), bottom);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(1, 2, 3);
    ui.painter().add(mesh);
}

/// PublicSkin Button.png: a dark gradient that turns red on hover and press.
pub fn button(ui: &mut Ui, caption: &str, width: f32, enabled: bool) -> Response {
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(vec2(width, 40.0), sense);
    let (enabled, active) = state(ui, &response);
    if active {
        ui.painter().rect_filled(rect, 0, RED);
    } else {
        gradient(ui, rect, rgb(72, 73, 75), rgb(40, 41, 45));
    }
    let color = if enabled { TEXT } else { rgb(100, 100, 102) };
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        caption,
        bold(18.0),
        color,
    );
    response
}

/// Small toolbar button of the Oscar editor.
pub fn tool(ui: &mut Ui, caption: &str, selected: bool) -> Response {
    let font = regular(14.0);
    let width = ui
        .painter()
        .layout_no_wrap(caption.into(), font.clone(), TEXT)
        .size()
        .x
        + 18.0;
    let (rect, response) = ui.allocate_exact_size(vec2(width, 26.0), Sense::click());
    let (_, active) = state(ui, &response);
    let fill = if selected || active {
        RED
    } else {
        rgb(25, 25, 25)
    };
    ui.painter().rect_filled(rect, 0, fill);
    ui.painter()
        .text(rect.center(), Align2::CENTER_CENTER, caption, font, TEXT);
    response
}

/// PublicSkin Check.png: a dark box with a light frame and a white check.
pub fn check(ui: &mut Ui, value: &mut bool, caption: &str) -> Response {
    let font = regular(15.0);
    let text = ui
        .painter()
        .layout_no_wrap(caption.into(), font.clone(), TEXT);
    let gap = if caption.is_empty() { 0.0 } else { 8.0 };
    let size = vec2(18.0 + gap + text.size().x, 20.0f32.max(text.size().y));
    let (rect, mut response) = ui.allocate_exact_size(size, Sense::click());
    if response.clicked() {
        *value = !*value;
        response.mark_changed();
    }
    let enabled = ui.is_enabled();
    let light = if enabled { rgb(220, 220, 220) } else { rgb(90, 90, 92) };
    let square = Rect::from_center_size(pos2(rect.left() + 9.0, rect.center().y), vec2(16.0, 16.0));
    let painter = ui.painter();
    painter.rect_filled(square, 0, rgb(30, 31, 34));
    painter.rect_stroke(square, 0, Stroke::new(1.0, light), StrokeKind::Inside);
    if *value {
        let stroke = Stroke::new(1.6, light);
        let points = [
            square.left_center() + vec2(3.5, 0.0),
            square.center_bottom() + vec2(-1.0, -4.0),
            square.right_top() + vec2(-3.0, 3.5),
        ];
        painter.line_segment([points[0], points[1]], stroke);
        painter.line_segment([points[1], points[2]], stroke);
    }
    painter.text(
        pos2(square.right() + gap, rect.center().y),
        Align2::LEFT_CENTER,
        caption,
        font,
        if enabled { TEXT } else { DISABLED },
    );
    response
}

/// TrackBar of the sensitivity pages: a gray 10 px groove, a red fill that
/// darkens towards the thumb and a small red thumb. Values snap to the step.
pub fn trackbar(
    ui: &mut Ui,
    value: &mut u32,
    range: (u32, u32),
    step: u32,
    width: f32,
) -> Response {
    let (rect, mut response) = ui.allocate_exact_size(vec2(width, 22.0), Sense::click_and_drag());
    let enabled = ui.is_enabled();
    let left = rect.left() + 5.0;
    let span = rect.width() - 10.0;
    if enabled
        && (response.dragged() || response.clicked())
        && let Some(pointer) = response.interact_pointer_pos()
    {
        let fraction = ((pointer.x - left) / span).clamp(0.0, 1.0);
        let raw = range.0 as f32 + fraction * (range.1 - range.0) as f32;
        let snapped = (((raw - range.0 as f32) / step as f32).round() as u32 * step + range.0)
            .clamp(range.0, range.1);
        if snapped != *value {
            *value = snapped;
            response.mark_changed();
        }
    }
    let fraction = (*value - range.0) as f32 / (range.1 - range.0).max(1) as f32;
    let x = left + fraction * span;
    let y = rect.center().y;
    let dim = |color: Color32| {
        if enabled {
            color
        } else {
            color.gamma_multiply(0.35)
        }
    };
    let painter = ui.painter();
    let track = Rect::from_min_max(pos2(rect.left(), y - 5.0), pos2(rect.right(), y + 5.0));
    painter.rect_filled(track, 5, dim(TRACK));
    let filled = Rect::from_min_max(
        pos2(track.left() + 1.0, track.top() + 1.0),
        pos2(x, track.bottom() - 1.0),
    );
    if filled.width() > 0.0 {
        let mut mesh = egui::Mesh::default();
        let (from, to) = (dim(rgb(180, 9, 18)), dim(rgb(93, 5, 10)));
        mesh.colored_vertex(filled.left_top(), from);
        mesh.colored_vertex(filled.right_top(), to);
        mesh.colored_vertex(filled.left_bottom(), from);
        mesh.colored_vertex(filled.right_bottom(), to);
        mesh.add_triangle(0, 1, 2);
        mesh.add_triangle(1, 2, 3);
        painter.add(mesh);
    }
    let thumb = Rect::from_center_size(Pos2::new(x, y), vec2(9.0, 12.0));
    painter.rect_filled(thumb, 1, dim(rgb(210, 23, 30)));
    painter.rect_stroke(
        thumb,
        1,
        Stroke::new(1.0, dim(rgb(150, 12, 18))),
        StrokeKind::Inside,
    );
    if enabled && (response.hovered() || response.dragged()) {
        painter.rect_stroke(thumb, 1, Stroke::new(1.0, CAPTION_RED), StrokeKind::Outside);
    }
    response
}

/// Tick marks and their captions below a trackbar of the given rect.
/// Each entry is a fraction of the track and an optional caption.
pub fn ticks(ui: &mut Ui, width: f32, marks: &[(f32, &str)]) {
    let captions = marks.iter().any(|(_, text)| !text.is_empty());
    let height = if captions { 30.0 } else { 12.0 };
    let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    let left = rect.left() + 5.0;
    let span = rect.width() - 10.0;
    let painter = ui.painter();
    for (fraction, text) in marks {
        let x = (left + fraction * span).round() + 0.5;
        painter.line_segment(
            [pos2(x, rect.top() + 2.0), pos2(x, rect.top() + 9.0)],
            Stroke::new(1.0, TICK),
        );
        painter.text(
            pos2(x, rect.top() + 21.0),
            Align2::CENTER_CENTER,
            *text,
            regular(12.0),
            TICK,
        );
    }
}

/// Value label of one CPI preset. The selected preset uses a red background.
pub fn value(ui: &mut Ui, text: &str, selected: bool, size: [f32; 2]) -> Response {
    let (rect, response) = ui.allocate_exact_size(size.into(), Sense::click());
    let enabled = ui.is_enabled();
    let fill = if selected && enabled { SELECTED } else { VALUE };
    ui.painter().rect_filled(rect, 0, fill);
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        text,
        regular(15.0),
        if enabled { TEXT } else { DISABLED },
    );
    response
}

/// Group frame of a settings sheet.
pub fn sheet() -> egui::Frame {
    egui::Frame::new()
        .fill(SHEET)
        .inner_margin(egui::Margin::same(16))
}

/// Top-left logo box of the dashboard, 173 × 83. The official image is not
/// shipped; the brand name is drawn as text.
pub fn logo(ui: &Ui, rect: Rect) {
    let painter = ui.painter();
    let rect = Rect::from_min_size(rect.min + vec2(1.0, 1.0), vec2(172.0, 82.0));
    painter.rect_filled(rect, 0, LOGO);
    painter.text(
        rect.center() + vec2(0.0, -2.0),
        Align2::CENTER_CENTER,
        "bloody",
        bold(34.0),
        rgb(201, 34, 39),
    );
}

/// cbbt_MacroTemplate: the red template selector in place of the sub tabs.
pub fn template(ui: &mut Ui, caption: &str) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(274.0, 48.0), Sense::click());
    let painter = ui.painter();
    painter.rect_filled(rect, 0, RED);
    painter.text(
        rect.left_center() + vec2(6.0, 0.0),
        Align2::LEFT_CENTER,
        caption,
        regular(19.0),
        TEXT,
    );
    let c = rect.right_center() + vec2(-12.0, 1.0);
    painter.add(egui::Shape::convex_polygon(
        vec![c + vec2(-7.0, -3.0), c + vec2(7.0, -3.0), c + vec2(0.0, 4.0)],
        rgb(60, 0, 4),
        Stroke::NONE,
    ));
    response
}

/// lb_FunctionDescription: the orange bar below the sub tabs, 65 px high.
pub fn banner(ui: &mut Ui, text: &str) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 65.0), Sense::hover());
    gradient(ui, rect, BANNER_TOP, BANNER_BOTTOM);
    let galley = ui.painter().layout(
        text.to_owned(),
        bold(15.0),
        BANNER_TEXT,
        rect.width() - 40.0,
    );
    let pos = pos2(rect.left() + 20.0, rect.center().y - galley.size().y / 2.0);
    ui.painter().galley(pos, galley, BANNER_TEXT);
}

/// The 恢复默认 / 取消 / 应用 row at the bottom right of a page. `area` is
/// the page background; the buttons keep their place while the page scrolls.
pub fn footer(ui: &Ui, area: Rect, buttons: &[(&str, bool)]) -> Vec<bool> {
    let count = buttons.len() as f32;
    let left = area.right() - 12.0 - 137.0 * count - 13.0 * (count - 1.0);
    let top = area.bottom() - 50.0;
    egui::Area::new(ui.scope_id().with("footer"))
        .fixed_pos(pos2(left, top))
        .constrain(false)
        .show(ui.ctx(), |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 13.0;
                buttons
                    .iter()
                    .map(|(caption, enabled)| button(ui, caption, 137.0, *enabled).clicked())
                    .collect()
            })
            .inner
        })
        .inner
}

/// Pictograms of the red icon tiles.
#[derive(Clone, Copy)]
pub enum Icon {
    MouseX,
    MouseY,
    Speed,
    DoubleClick,
    Rate,
    Pointer,
    Light,
    Wheel,
    Startup,
    Language,
}

/// The 70 × 70 icon tile at the top left of a group panel: a dark red glow
/// with a red pictogram.
pub fn icon(ui: &mut Ui, icon: Icon) {
    let (rect, _) = ui.allocate_exact_size(vec2(70.0, 70.0), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 4, rgb(50, 51, 55));
    let inner = rect.shrink(2.0);
    let mut mesh = egui::Mesh::default();
    let center = inner.center();
    mesh.colored_vertex(center, rgb(96, 12, 16));
    let corners = [
        inner.left_top(),
        inner.right_top(),
        inner.right_bottom(),
        inner.left_bottom(),
    ];
    for corner in corners {
        mesh.colored_vertex(corner, rgb(45, 18, 20));
    }
    for i in 0..4 {
        mesh.add_triangle(0, 1 + i, 1 + (i + 1) % 4);
    }
    painter.add(mesh);
    let red = rgb(220, 20, 30);
    let stroke = Stroke::new(2.5, red);
    let c = center;
    match icon {
        Icon::MouseX | Icon::MouseY => {
            let body = Rect::from_center_size(c, vec2(22.0, 32.0));
            painter.rect_stroke(body, 11, stroke, StrokeKind::Inside);
            painter.line_segment([body.center_top(), body.center_top() + vec2(0.0, 9.0)], stroke);
            let thin = Stroke::new(2.0, red);
            let chevron = |tip: Pos2, a: egui::Vec2, b: egui::Vec2| {
                painter.line_segment([tip, tip + a], thin);
                painter.line_segment([tip, tip + b], thin);
            };
            if matches!(icon, Icon::MouseX) {
                chevron(c + vec2(-24.0, 0.0), vec2(4.0, -4.0), vec2(4.0, 4.0));
                chevron(c + vec2(24.0, 0.0), vec2(-4.0, -4.0), vec2(-4.0, 4.0));
            } else {
                chevron(c + vec2(0.0, -26.0), vec2(-4.0, 4.0), vec2(4.0, 4.0));
                chevron(c + vec2(0.0, 26.0), vec2(-4.0, -4.0), vec2(4.0, -4.0));
            }
        }
        Icon::Speed => {
            let points: Vec<Pos2> = (0..=20)
                .map(|i| {
                    let angle = std::f32::consts::PI * (1.0 + i as f32 / 20.0);
                    c + vec2(angle.cos(), angle.sin()) * 20.0 + vec2(0.0, 8.0)
                })
                .collect();
            painter.add(egui::Shape::line(points, stroke));
            painter.line_segment([c + vec2(0.0, 8.0), c + vec2(6.0, -10.0)], Stroke::new(3.5, red));
            painter.circle_filled(c + vec2(0.0, 8.0), 3.5, red);
        }
        Icon::DoubleClick => {
            let points = vec![
                c + vec2(-20.0, 4.0),
                c + vec2(-12.0, 14.0),
                c + vec2(-4.0, -2.0),
                c + vec2(4.0, 12.0),
                c + vec2(18.0, -12.0),
            ];
            painter.add(egui::Shape::line(points, Stroke::new(3.0, red)));
            let tip = c + vec2(20.0, -14.0);
            painter.add(egui::Shape::convex_polygon(
                vec![tip, tip + vec2(-11.0, 1.0), tip + vec2(-1.0, 10.0)],
                red,
                Stroke::NONE,
            ));
        }
        Icon::Rate => {
            painter.circle_stroke(c, 20.0, stroke);
            painter.circle_stroke(c, 9.0, Stroke::new(2.0, red));
            painter.line_segment([c + vec2(-24.0, 0.0), c + vec2(24.0, 0.0)], Stroke::new(1.5, red));
            painter.line_segment([c + vec2(0.0, -24.0), c + vec2(0.0, 24.0)], Stroke::new(1.5, red));
        }
        Icon::Pointer => {
            let tip = c + vec2(-16.0, -16.0);
            painter.add(egui::Shape::convex_polygon(
                vec![tip, tip + vec2(22.0, 8.0), tip + vec2(8.0, 22.0)],
                red,
                Stroke::NONE,
            ));
            painter.line_segment([tip + vec2(12.0, 12.0), tip + vec2(26.0, 26.0)], Stroke::new(4.0, red));
            let ghost = Stroke::new(1.5, rgb(160, 20, 28));
            let tip = tip + vec2(12.0, 6.0);
            painter.line_segment([tip, tip + vec2(22.0, 8.0)], ghost);
            painter.line_segment([tip, tip + vec2(8.0, 22.0)], ghost);
        }
        Icon::Light => {
            painter.circle_stroke(c + vec2(0.0, -2.0), 11.0, stroke);
            painter.text(c + vec2(0.0, -2.0), Align2::CENTER_CENTER, "A", bold(14.0), red);
            for i in 0..8 {
                let angle = std::f32::consts::TAU * i as f32 / 8.0;
                let d = vec2(angle.cos(), angle.sin());
                if d.y > 0.6 {
                    continue;
                }
                painter.line_segment(
                    [c + vec2(0.0, -2.0) + d * 15.0, c + vec2(0.0, -2.0) + d * 21.0],
                    Stroke::new(2.0, red),
                );
            }
            painter.line_segment([c + vec2(-6.0, 13.0), c + vec2(6.0, 13.0)], stroke);
            painter.line_segment([c + vec2(-4.0, 18.0), c + vec2(4.0, 18.0)], stroke);
        }
        Icon::Startup => {
            painter.line_segment([c + vec2(-12.0, 0.0), c + vec2(10.0, 0.0)], Stroke::new(5.0, red));
            painter.add(egui::Shape::convex_polygon(
                vec![c + vec2(14.0, 0.0), c + vec2(2.0, -11.0), c + vec2(2.0, 11.0)],
                red,
                Stroke::NONE,
            ));
        }
        Icon::Language => {
            let back = Rect::from_min_size(c + vec2(-20.0, -18.0), vec2(24.0, 22.0));
            let front = Rect::from_min_size(c + vec2(-4.0, -4.0), vec2(24.0, 22.0));
            painter.rect_stroke(back, 3, Stroke::new(2.0, red), StrokeKind::Inside);
            painter.text(back.center(), Align2::CENTER_CENTER, "A", bold(13.0), red);
            painter.rect_filled(front, 3, rgb(70, 14, 18));
            painter.rect_stroke(front, 3, Stroke::new(2.0, red), StrokeKind::Inside);
            painter.text(front.center(), Align2::CENTER_CENTER, "中", bold(13.0), red);
        }
        Icon::Wheel => {
            let body = Rect::from_center_size(c, vec2(16.0, 34.0));
            painter.rect_stroke(body, 8, stroke, StrokeKind::Inside);
            for i in 0..4 {
                let y = body.top() + 8.0 + i as f32 * 6.0;
                painter.line_segment([pos2(c.x - 4.0, y), pos2(c.x, y + 3.0)], Stroke::new(1.5, red));
                painter.line_segment([pos2(c.x + 4.0, y), pos2(c.x, y + 3.0)], Stroke::new(1.5, red));
            }
        }
    }
}

/// The gray "i" badge at the top right of a group panel. Its tooltip holds
/// the help text.
pub fn info(ui: &Ui, center: Pos2, help: &str) {
    let rect = Rect::from_center_size(center, vec2(24.0, 24.0));
    let response = ui.interact(rect, ui.scope_id().with(("info", help)), Sense::hover());
    let painter = ui.painter();
    painter.circle_filled(center, 12.0, rgb(102, 102, 102));
    painter.text(center, Align2::CENTER_CENTER, "i", bold(17.0), PANEL);
    if !help.is_empty() {
        response.on_hover_text(help);
    }
}

/// A group panel of a page: icon tile, title and info badge, then the body.
/// Panels are 1200 px wide and centered, as on the sensitivity pages.
pub fn card<R>(
    ui: &mut Ui,
    icon: Option<Icon>,
    title: &str,
    help: &str,
    body: impl FnOnce(&mut Ui) -> R,
) -> R {
    let margin = ((ui.available_width() - 1200.0) / 2.0).max(0.0);
    ui.horizontal(|ui| {
        ui.add_space(margin);
        ui.vertical(|ui| {
            ui.set_width(ui.available_width() - margin);
            card_frame(ui, icon, title, help, body)
        })
        .inner
    })
    .inner
}

fn card_frame<R>(
    ui: &mut Ui,
    icon: Option<Icon>,
    title: &str,
    help: &str,
    body: impl FnOnce(&mut Ui) -> R,
) -> R {
    egui::Frame::new()
        .fill(PANEL)
        .inner_margin(egui::Margin {
            left: 12,
            right: 12,
            top: 12,
            bottom: 24,
        })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            let top = ui.max_rect().right_top();
            info(ui, top + vec2(-12.0, 11.0), help);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 18.0;
                match icon {
                    Some(icon) => {
                        self::icon(ui, icon);
                        ui.label(egui::RichText::new(title).font(regular(17.0)));
                    }
                    None => {
                        ui.label(egui::RichText::new(title).font(regular(17.0)));
                    }
                }
            });
            body(ui)
        })
        .inner
}

/// Small square option of the LOD and wheel groups: red when chosen.
pub fn option(ui: &mut Ui, selected: bool, caption: &str) -> Response {
    let font = regular(16.0);
    let text = ui
        .painter()
        .layout_no_wrap(caption.into(), font.clone(), TEXT);
    let size = vec2(20.0 + text.size().x, 22.0f32.max(text.size().y));
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let enabled = ui.is_enabled();
    let square = Rect::from_center_size(pos2(rect.left() + 6.0, rect.center().y), vec2(12.0, 12.0));
    let painter = ui.painter();
    painter.rect_filled(square, 0, rgb(25, 25, 27));
    let fill = if selected { rgb(200, 10, 20) } else { rgb(90, 90, 92) };
    painter.rect_filled(square.shrink(1.0), 0, fill);
    painter.text(
        pos2(square.right() + 7.0, rect.center().y),
        Align2::LEFT_CENTER,
        caption,
        font,
        if enabled { TEXT } else { DISABLED },
    );
    response
}

/// OFF ⬤ ON switch of the 其他设置 page.
pub fn toggle(ui: &mut Ui, on: &mut bool) -> Response {
    let (rect, mut response) = ui.allocate_exact_size(vec2(136.0, 26.0), Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    let painter = ui.painter();
    let off_color = if *on { rgb(80, 80, 82) } else { rgb(160, 160, 162) };
    let on_color = if *on { CAPTION_RED } else { rgb(80, 80, 82) };
    painter.text(rect.left_center(), Align2::LEFT_CENTER, "OFF", bold(17.0), off_color);
    painter.text(rect.right_center(), Align2::RIGHT_CENTER, "ON", bold(17.0), on_color);
    let groove = Rect::from_center_size(rect.center(), vec2(58.0, 22.0));
    painter.rect_filled(groove, 11, rgb(30, 31, 34));
    let knob_x = if *on { groove.right() - 21.0 } else { groove.left() + 21.0 };
    let knob = Rect::from_center_size(pos2(knob_x, groove.center().y), vec2(40.0, 18.0));
    painter.rect_filled(knob, 9, if *on { RED } else { rgb(110, 110, 112) });
    response
}

/// Four-stop brightness selector with square knobs and bulbs below.
pub fn levels(ui: &mut Ui, current: Option<u8>, width: f32) -> Option<u8> {
    let (rect, _) = ui.allocate_exact_size(vec2(width, 86.0), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 0, rgb(44, 45, 49));
    let left = rect.left() + 42.0;
    let span = rect.width() - 84.0;
    let y = rect.top() + 14.0;
    painter.rect_filled(
        Rect::from_min_max(pos2(left, y - 3.0), pos2(left + span, y + 4.0)),
        0,
        rgb(28, 29, 31),
    );
    let mut chosen = None;
    for level in 0..4u8 {
        let x = left + span * f32::from(level) / 3.0;
        let knob = Rect::from_center_size(pos2(x, y), vec2(22.0, 22.0));
        let response = ui.interact(knob.expand(6.0), ui.scope_id().with(("level", level)), Sense::click());
        let painter = ui.painter();
        painter.rect_filled(knob, 2, rgb(30, 31, 33));
        let lit = current == Some(level);
        let hovered = response.hovered() && ui.is_enabled();
        let fill = if lit {
            rgb(200, 10, 20)
        } else if hovered {
            rgb(150, 150, 152)
        } else {
            rgb(100, 100, 102)
        };
        painter.rect_filled(Rect::from_center_size(knob.center(), vec2(10.0, 10.0)), 0, fill);
        if response.clicked() && ui.is_enabled() {
            chosen = Some(level);
        }
        let bulb = pos2(x, y + 40.0);
        let stroke = Stroke::new(2.0, rgb(150, 150, 152));
        painter.circle_stroke(bulb, 8.0, stroke);
        painter.line_segment([bulb + vec2(-4.0, 11.0), bulb + vec2(4.0, 11.0)], stroke);
        painter.line_segment([bulb + vec2(-3.0, 15.0), bulb + vec2(3.0, 15.0)], stroke);
        let rays = match level {
            0 => 0,
            1 => 2,
            2 => 4,
            _ => 6,
        };
        for i in 0..rays {
            let angle = -std::f32::consts::FRAC_PI_2
                + (i as f32 - (rays - 1) as f32 / 2.0) * std::f32::consts::FRAC_PI_4 * 0.9;
            let d = vec2(angle.cos(), angle.sin());
            painter.line_segment([bulb + d * 12.0, bulb + d * 17.0], stroke);
        }
        if level == 3 {
            painter.text(bulb, Align2::CENTER_CENTER, "A", regular(10.0), rgb(150, 150, 152));
        }
    }
    chosen
}

/// KeySet button combo box, 255 × 25, with the chevron on the right.
pub fn combo(ui: &mut Ui, caption: &str, width: f32, open: bool) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(width, 25.0), Sense::click());
    let hovered = response.hovered() && ui.is_enabled();
    let painter = ui.painter();
    let fill = if open || hovered {
        rgb(72, 73, 78)
    } else {
        rgb(59, 60, 64)
    };
    painter.rect_filled(rect, 0, fill);
    let text = painter.layout(
        caption.to_owned(),
        regular(15.0),
        TEXT,
        f32::INFINITY,
    );
    let clip = painter.with_clip_rect(Rect::from_min_max(rect.min, pos2(rect.right() - 26.0, rect.bottom())));
    clip.galley(pos2(rect.left() + 5.0, rect.center().y - text.size().y / 2.0), text, TEXT);
    let c = rect.right_center() + vec2(-12.0, 0.0);
    let stroke = Stroke::new(1.2, rgb(200, 200, 202));
    painter.line_segment([c + vec2(-5.0, -2.5), c + vec2(0.0, 2.5)], stroke);
    painter.line_segment([c + vec2(5.0, -2.5), c + vec2(0.0, 2.5)], stroke);
    response
}

/// Caption box of the button diagram, 38 × 30. Red when selected.
pub fn key_box(ui: &mut Ui, rect: Rect, caption: &str, selected: bool) -> Response {
    let response = ui.interact(rect, ui.scope_id().with(("key-box", caption)), Sense::click());
    let painter = ui.painter();
    let hovered = response.hovered();
    if selected {
        painter.rect_filled(rect, 3, rgb(225, 20, 30));
    } else {
        painter.rect_filled(rect, 3, if hovered { rgb(60, 61, 66) } else { SHEET });
        painter.rect_stroke(rect, 3, Stroke::new(1.5, rgb(164, 164, 166)), StrokeKind::Inside);
    }
    painter.text(rect.center(), Align2::CENTER_CENTER, caption, bold(20.0), TEXT);
    response
}

/// Top view of the mouse for the KeySet diagram, drawn in a 560 × 520
/// area whose origin is `origin`.
pub fn mouse_top(ui: &Ui, origin: Pos2) {
    let painter = ui.painter();
    let p = |x: f32, y: f32| origin + vec2(x, y);
    let line = Stroke::new(1.5, rgb(130, 130, 134));
    let curve = |points: [Pos2; 4], stroke: Stroke| {
        painter.add(egui::Shape::CubicBezier(
            egui::epaint::CubicBezierShape::from_points_stroke(
                points,
                false,
                Color32::TRANSPARENT,
                stroke,
            ),
        ));
    };
    // Body, mirrored around x 272.
    let mut body = Vec::new();
    let half = [
        [p(272.0, 48.0), p(230.0, 50.0), p(190.0, 58.0), p(175.0, 80.0)],
        [p(175.0, 80.0), p(150.0, 160.0), p(155.0, 200.0), p(140.0, 250.0)],
        [p(140.0, 250.0), p(115.0, 330.0), p(120.0, 400.0), p(180.0, 450.0)],
        [p(180.0, 450.0), p(220.0, 480.0), p(250.0, 500.0), p(272.0, 508.0)],
    ];
    for segment in half {
        let shape = egui::epaint::CubicBezierShape::from_points_stroke(
            segment,
            false,
            Color32::TRANSPARENT,
            line,
        );
        body.extend(shape.flatten(Some(0.5)));
    }
    let mirrored: Vec<Pos2> = body
        .iter()
        .rev()
        .map(|point| pos2(2.0 * (origin.x + 272.0) - point.x, point.y))
        .collect();
    let mut outline = body.clone();
    outline.extend(mirrored);
    painter.add(egui::Shape::convex_polygon(
        outline.clone(),
        rgb(44, 45, 49),
        Stroke::NONE,
    ));
    painter.add(egui::Shape::closed_line(outline, line));
    // Cable.
    painter.line_segment([p(268.0, 14.0), p(268.0, 50.0)], Stroke::new(2.0, rgb(110, 110, 114)));
    painter.line_segment([p(276.0, 14.0), p(276.0, 50.0)], Stroke::new(2.0, rgb(110, 110, 114)));
    // Button seams and the center ridge.
    curve([p(255.0, 50.0), p(250.0, 120.0), p(252.0, 200.0), p(250.0, 270.0)], line);
    curve([p(289.0, 50.0), p(294.0, 120.0), p(292.0, 200.0), p(294.0, 270.0)], line);
    curve([p(175.0, 80.0), p(190.0, 140.0), p(215.0, 230.0), p(250.0, 270.0)], line);
    curve([p(369.0, 80.0), p(354.0, 140.0), p(329.0, 230.0), p(294.0, 270.0)], line);
    curve([p(250.0, 270.0), p(255.0, 310.0), p(265.0, 320.0), p(272.0, 330.0)], line);
    curve([p(294.0, 270.0), p(289.0, 310.0), p(279.0, 320.0), p(272.0, 330.0)], line);
    curve([p(160.0, 420.0), p(210.0, 440.0), p(240.0, 455.0), p(272.0, 465.0)], line);
    curve([p(384.0, 420.0), p(334.0, 440.0), p(304.0, 455.0), p(272.0, 465.0)], line);
    // Wheel.
    let wheel = Rect::from_min_size(p(258.0, 88.0), vec2(28.0, 74.0));
    painter.rect_filled(wheel, 13, rgb(70, 71, 75));
    painter.rect_stroke(wheel, 13, line, StrokeKind::Inside);
    for i in 0..9 {
        let y = wheel.top() + 12.0 + i as f32 * 6.0;
        painter.line_segment(
            [pos2(wheel.left() + 7.0, y), pos2(wheel.center().x, y + 3.0)],
            Stroke::new(1.0, rgb(150, 150, 154)),
        );
        painter.line_segment(
            [pos2(wheel.right() - 7.0, y), pos2(wheel.center().x, y + 3.0)],
            Stroke::new(1.0, rgb(150, 150, 154)),
        );
    }
    // Top buttons 1, N, 3 as chevrons along the ridge.
    for y in [182.0, 214.0, 246.0] {
        painter.add(egui::Shape::line(
            vec![p(256.0, y), p(272.0, y + 12.0), p(288.0, y)],
            line,
        ));
    }
    // Side buttons.
    for y in [178.0, 246.0] {
        let rect = Rect::from_min_size(p(147.0, y), vec2(10.0, 30.0));
        painter.rect_stroke(rect, 4, line, StrokeKind::Inside);
    }
    painter.text(p(272.0, 395.0), Align2::CENTER_CENTER, "bloody", bold(15.0), rgb(110, 110, 114));
}
