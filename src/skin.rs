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
// KeySet advgb_Sheet.
pub const SHEET: Color32 = rgb(37, 38, 42);
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
const THUMB: Color32 = rgb(179, 25, 29);
const PROFILE: Color32 = rgb(29, 30, 34);
const PROFILE_RED: Color32 = rgb(171, 10, 16);
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

/// Dashboard main tab, 175 × 50. Hover and selection add a red left edge.
pub fn main_tab(ui: &mut Ui, caption: &str, selected: bool, enabled: bool) -> Response {
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(vec2(175.0, 50.0), sense);
    let (enabled, active) = state(ui, &response);
    let lit = enabled && (selected || active);
    let painter = ui.painter();
    painter.rect_filled(rect, 0, if lit { MAIN_HOVER } else { DASHBOARD });
    if lit {
        painter.rect_filled(Rect::from_min_size(rect.min, vec2(3.0, 50.0)), 0, MAIN_BAR);
    }
    let color = match (enabled, lit) {
        (false, _) => DISABLED,
        (true, true) => CAPTION_RED,
        _ => TEXT,
    };
    painter.text(
        rect.left_center() + vec2(40.0, 0.0),
        Align2::LEFT_CENTER,
        caption,
        bold(18.0),
        color,
    );
    response
}

/// Dashboard device entry, 175 × 131, with a drawn mouse outline.
pub fn device(ui: &mut Ui, caption: &str, series: &str, state: &str) {
    let (rect, _) = ui.allocate_exact_size(vec2(175.0, 131.0), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 0, MAIN_HOVER);
    painter.rect_filled(Rect::from_min_size(rect.min, vec2(3.0, 131.0)), 0, MAIN_BAR);
    let body = Rect::from_center_size(rect.left_center() + vec2(33.0, -12.0), vec2(30.0, 42.0));
    let stroke = Stroke::new(2.0, rgb(200, 200, 200));
    painter.rect_stroke(body, 14, stroke, StrokeKind::Inside);
    painter.line_segment(
        [body.center_top(), body.center_top() + vec2(0.0, 14.0)],
        stroke,
    );
    painter.text(
        pos2(rect.left() + 60.0, rect.center().y - 22.0),
        Align2::LEFT_CENTER,
        caption,
        bold(18.0),
        CAPTION_RED,
    );
    painter.text(
        pos2(rect.left() + 60.0, rect.center().y + 2.0),
        Align2::LEFT_CENTER,
        series,
        bold(16.0),
        TEXT,
    );
    painter.text(
        pos2(rect.left() + 14.0, rect.bottom() - 18.0),
        Align2::LEFT_CENTER,
        state,
        regular(13.0),
        LIGHT_GRAY,
    );
}

/// Dashboard sub tab, 201 × 51. Hover and selection fill it red.
pub fn sub_tab(ui: &mut Ui, caption: &str, selected: bool, enabled: bool) -> Response {
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(vec2(198.0, 51.0), sense);
    let (enabled, active) = state(ui, &response);
    let lit = enabled && (selected || active);
    let painter = ui.painter();
    painter.rect_filled(rect, 0, if lit { RED } else { DASHBOARD });
    painter.line_segment(
        [rect.right_top(), rect.right_bottom()],
        Stroke::new(1.0, BACKGROUND),
    );
    let color = match (enabled, lit) {
        (false, _) => rgb(70, 70, 72),
        (true, true) => TEXT,
        _ => GRAY,
    };
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        caption,
        bold(18.0),
        color,
    );
    response
}

/// KeySet profile tab, 157 × 54. Selection fades to red at the bottom.
pub fn profile_tab(ui: &mut Ui, caption: &str, selected: bool, enabled: bool) -> Response {
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(vec2(154.0, 54.0), sense);
    let (enabled, active) = state(ui, &response);
    let painter = ui.painter();
    if enabled && selected {
        gradient(ui, rect, PROFILE, PROFILE_RED);
    } else {
        painter.rect_filled(rect, 0, if active { SHEET } else { PROFILE });
    }
    painter.line_segment(
        [rect.right_top(), rect.right_bottom()],
        Stroke::new(2.0, BACKGROUND),
    );
    let color = if enabled { TEXT } else { DISABLED };
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        caption,
        bold(18.0),
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
        gradient(ui, rect, rgb(70, 71, 73), rgb(37, 38, 42));
    }
    let color = if enabled { TEXT } else { rgb(100, 100, 102) };
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        caption,
        bold(16.0),
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

/// PublicSkin Check.png: a dark box with a red frame and red check.
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
    let red = if enabled { THUMB } else { rgb(90, 40, 42) };
    let square = Rect::from_center_size(pos2(rect.left() + 9.0, rect.center().y), vec2(17.0, 17.0));
    let painter = ui.painter();
    painter.rect_filled(square, 0, rgb(12, 12, 13));
    painter.rect_stroke(square, 0, Stroke::new(1.0, red), StrokeKind::Inside);
    if *value {
        let stroke = Stroke::new(2.5, red);
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

/// CPI track bar with the red TrackBar_Thumb. Values snap to the step.
pub fn trackbar(
    ui: &mut Ui,
    value: &mut u32,
    range: (u32, u32),
    step: u32,
    width: f32,
) -> Response {
    let (rect, mut response) = ui.allocate_exact_size(vec2(width, 26.0), Sense::click_and_drag());
    let enabled = ui.is_enabled();
    let left = rect.left() + 5.0;
    let span = rect.width() - 10.0;
    if enabled
        && (response.dragged() || response.clicked())
        && let Some(pointer) = response.interact_pointer_pos()
    {
        let fraction = ((pointer.x - left) / span).clamp(0.0, 1.0);
        let raw = range.0 as f32 + fraction * (range.1 - range.0) as f32;
        let snapped = ((raw / step as f32).round() as u32 * step).clamp(range.0, range.1);
        if snapped != *value {
            *value = snapped;
            response.mark_changed();
        }
    }
    let fraction = (*value - range.0) as f32 / (range.1 - range.0) as f32;
    let x = left + fraction * span;
    let y = rect.center().y;
    let painter = ui.painter();
    let track = Rect::from_min_max(pos2(left, y - 2.0), pos2(left + span, y + 2.0));
    painter.rect_filled(track, 0, rgb(12, 12, 13));
    painter.rect_stroke(
        track,
        0,
        Stroke::new(1.0, rgb(60, 61, 64)),
        StrokeKind::Outside,
    );
    let filled = Rect::from_min_max(track.min, pos2(x, track.max.y));
    painter.rect_filled(filled, 0, if enabled { RED } else { rgb(70, 30, 32) });
    let thumb = Rect::from_center_size(Pos2::new(x, y), vec2(10.0, 16.0));
    painter.rect_filled(thumb, 1, if enabled { THUMB } else { rgb(90, 40, 42) });
    if enabled && (response.hovered() || response.dragged()) {
        painter.rect_stroke(thumb, 1, Stroke::new(1.0, CAPTION_RED), StrokeKind::Outside);
    }
    response
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

/// SensitivityCPI DiagramAxis image: a red mouse with axis arrows.
pub fn axis(ui: &mut Ui, horizontal: bool) {
    let (rect, _) = ui.allocate_exact_size(vec2(64.0, 62.0), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 8, rgb(40, 14, 16));
    painter.rect_stroke(
        rect,
        8,
        Stroke::new(1.0, rgb(110, 20, 24)),
        StrokeKind::Inside,
    );
    let red = Stroke::new(2.0, THUMB);
    let body = Rect::from_center_size(rect.center(), vec2(18.0, 26.0));
    painter.rect_stroke(body, 9, red, StrokeKind::Inside);
    painter.line_segment([body.center_top(), body.center_top() + vec2(0.0, 8.0)], red);
    let c = rect.center();
    let arrows = if horizontal {
        [
            (c + vec2(-17.0, 0.0), -1.0, true),
            (c + vec2(17.0, 0.0), 1.0, true),
        ]
    } else {
        [
            (c + vec2(0.0, -21.0), -1.0, false),
            (c + vec2(0.0, 21.0), 1.0, false),
        ]
    };
    for (tip, direction, along_x) in arrows {
        let back = if along_x {
            vec2(-4.0 * direction, 4.0)
        } else {
            vec2(4.0, -4.0 * direction)
        };
        let other = if along_x {
            vec2(-4.0 * direction, -4.0)
        } else {
            vec2(-4.0, -4.0 * direction)
        };
        painter.line_segment([tip, tip + back], red);
        painter.line_segment([tip, tip + other], red);
    }
}

/// Group frame of a settings sheet.
pub fn sheet() -> egui::Frame {
    egui::Frame::new()
        .fill(SHEET)
        .inner_margin(egui::Margin::same(16))
}
