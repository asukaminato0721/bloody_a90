use super::{App, DEFAULT_DPI};
use crate::{skin, worker::Job};
use bloody_a90::sensitivity;
use bloody_a90::{
    i18n::{tr, tr_text},
    tf,
};
use eframe::egui::{self, RichText, vec2};

impl App {
    /// Active presets in order and the selected index among them.
    fn active_presets(&self) -> (Vec<[u32; 2]>, usize) {
        let presets = (0..5)
            .filter(|index| self.enabled[*index])
            .map(|index| self.dpi[index])
            .collect();
        let selected = self.enabled[..self.selected].iter().filter(|e| **e).count();
        (presets, selected)
    }

    pub(super) fn sensitivity_block(&self) -> Result<Vec<u16>, String> {
        self.device.as_ref().ok_or(tr("鼠标未连接"))?;
        let template = self
            .sensitivity_template
            .as_deref()
            .ok_or(tr("尚未读取灵敏度设置"))?;
        let (presets, selected) = self.active_presets();
        sensitivity::block(template, &presets, selected, None)
    }

    pub(super) fn lift_block(&self) -> Result<Vec<u16>, String> {
        let template = self
            .device
            .as_ref()
            .ok_or(tr("鼠标未连接"))?
            .backup
            .sensitivity_template()?;
        sensitivity::with_lift_height(&template, self.lift_enabled.then_some(self.lift_level))
    }

    pub(super) fn rate_block(&self, hz: u32) -> Result<Vec<u16>, String> {
        let template = self
            .device
            .as_ref()
            .ok_or(tr("鼠标未连接"))?
            .backup
            .sensitivity_template()?;
        sensitivity::with_rate(&template, hz)
    }

    /// SensitivitySensorLOD: 预设高度 leaves the sensor default, 微精校准
    /// uses the level of the vertical bar. 校准 measures the surface.
    pub(super) fn lift(&mut self, ui: &mut egui::Ui) {
        ui.add_space(66.0);
        let help = tr("鼠标抬起后，传感器停止报告位移。等级不是毫米数值。\n\
                    校准时在使用表面上持续移动鼠标，保持鼠标接触表面。校准结果先载入此页，再点击“应用”保存。");
        skin::card(ui, None, tr("传感器 [微精校准LOD]"), help, |ui| {
            ui.add_space(-20.0);
            ui.horizontal_top(|ui| {
                ui.add_space(20.0);
                ui.vertical(|ui| {
                    ui.add_space(28.0);
                    sensor(ui);
                    ui.add_space(16.0);
                    ui.horizontal(|ui| {
                        ui.add_space(28.0);
                        if skin::button(ui, tr("⇣  校准"), 114.0, self.connected() && !self.pending)
                            .clicked()
                        {
                            self.start(Job::CalibrateLift);
                            self.status =
                                tr("正在校准。请在使用表面上持续移动鼠标，保持鼠标接触表面。")
                                    .into();
                        }
                    });
                });
                ui.add_space(24.0);
                ui.vertical(|ui| {
                    ui.add_space(96.0);
                    if skin::option(ui, !self.lift_enabled, tr("预设高度")).clicked() {
                        self.lift_enabled = false;
                    }
                    ui.add_space(14.0);
                    if skin::option(ui, self.lift_enabled, tr("微精校准")).clicked() {
                        self.lift_enabled = true;
                    }
                });
                ui.add_space(50.0);
                ui.add_enabled_ui(self.lift_enabled, |ui| {
                    ui.add_space(4.0);
                    ladder(ui, &mut self.lift_level);
                });
                ui.add_space(64.0);
                ui.vertical(|ui| {
                    ui.add_space(-4.0);
                    side_view(ui, self.lift_enabled.then_some(self.lift_level));
                });
                ui.vertical(|ui| {
                    ui.add_space(332.0);
                    let text = if self.lift_enabled {
                        self.lift_level.to_string()
                    } else {
                        "—".into()
                    };
                    skin::value(ui, &text, false, [60.0, 24.0]);
                });
            });
        });
        let connected = self.connected() && !self.pending;
        let clicked = skin::footer(
            ui,
            self.footer_area,
            &[
                (tr("恢复默认"), true),
                (tr("取消"), true),
                (tr("应用"), connected),
            ],
        );
        if clicked[0] {
            self.lift_enabled = false;
            self.lift_level = 16;
        }
        if clicked[1]
            && let Some(source) = &self.source
            && let Ok(level) = source
                .sensitivity_template()
                .and_then(|words| sensitivity::lift_height(&words))
        {
            self.lift_enabled = level.is_some();
            self.lift_level = level.unwrap_or(16);
        }
        if clicked[2] {
            match self.lift_block() {
                Ok(words) => self.start(Job::SaveLift(words)),
                Err(error) => self.report(Err(error)),
            }
        }
    }

    pub(super) fn resolution(&mut self, ui: &mut egui::Ui) {
        if !self.separate {
            for axes in &mut self.dpi {
                axes[1] = axes[0];
            }
        }
        let current = self.dpi[self.selected];
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 15.0;
            ui.add_space(95.0);
            ui.label(RichText::new(tr("目前 CPI :")).font(skin::regular(16.0)));
            ui.add_space(20.0);
            skin::value(ui, &current[0].to_string(), true, [100.0, 24.0]);
            skin::value(ui, &current[1].to_string(), true, [100.0, 24.0]);
        });
        ui.add_space(14.0);
        // The Y group appears only with 启用X/Y, as in SensitivityCPI.
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 16.0;
            ui.vertical(|ui| self.sheet(ui, 0));
            if self.separate {
                ui.vertical(|ui| self.sheet(ui, 1));
            }
        });
        let block = self.sensitivity_block();
        let enabled = [true, true, block.is_ok() && !self.pending];
        let clicked = skin::footer(
            ui,
            self.footer_area,
            &[
                (tr("恢复默认"), enabled[0]),
                (tr("取消"), enabled[1]),
                (tr("应用"), enabled[2]),
            ],
        );
        if clicked[0] {
            self.reset_dpi();
        }
        if clicked[1] {
            match self
                .device
                .as_ref()
                .map(|d| d.backup.clone())
                .or(self.source.clone())
            {
                Some(backup) => self.load_dpi(&backup),
                None => self.reset_dpi(),
            }
        }
        if clicked[2]
            && let Ok(words) = &block
        {
            self.start(Job::SaveSensitivity(words.clone()));
        }
        if let Err(error) = &block {
            ui.label(
                RichText::new(tr_text(error))
                    .font(skin::regular(13.0))
                    .color(skin::GRAY),
            );
        }
        ui.add_space(8.0);
        egui::CollapsingHeader::new(tr("查看编码参数")).show(ui, |ui| {
            egui::Grid::new("dpi-encoded")
                .striped(true)
                .min_col_width(90.0)
                .show(ui, |ui| {
                    for label in [tr("档位"), tr("X 比例"), tr("Y 比例"), tr("传感器编码")]
                    {
                        ui.strong(label);
                    }
                    ui.end_row();
                    for (index, [x, y]) in self.dpi.iter().copied().enumerate() {
                        if !self.enabled[index] {
                            continue;
                        }
                        ui.label((index + 1).to_string());
                        match sensitivity::encode(x, y) {
                            Ok(record) => {
                                ui.label(record.x_scale.to_string());
                                ui.label(record.y_scale.to_string());
                                ui.monospace(format!("{:04x}", record.sensor_code));
                            }
                            Err(error) => {
                                ui.colored_label(skin::ERROR, tr_text(&error));
                            }
                        }
                        ui.end_row();
                    }
                });
        });
    }

    // Factory presets of firmware 07.
    fn reset_dpi(&mut self) {
        self.dpi = DEFAULT_DPI;
        self.enabled = [true; 5];
        self.selected = 0;
        self.separate = false;
    }

    /// One axis group of SensitivityCPI, 685 px wide.
    fn sheet(&mut self, ui: &mut egui::Ui, axis: usize) {
        egui::Frame::new()
            .fill(skin::PANEL)
            .inner_margin(egui::Margin {
                left: 12,
                right: 12,
                top: 12,
                bottom: 20,
            })
            .show(ui, |ui| {
                ui.set_width(661.0);
                ui.horizontal(|ui| {
                    skin::icon(
                        ui,
                        if axis == 0 {
                            skin::Icon::MouseX
                        } else {
                            skin::Icon::MouseY
                        },
                    );
                    if self.separate {
                        ui.add_space(18.0);
                        ui.label(
                            RichText::new(if axis == 0 {
                                tr("X轴 设定 :")
                            } else {
                                tr("Y轴 设定 :")
                            })
                            .font(skin::regular(16.0)),
                        );
                    }
                    if axis == 0 {
                        let used = ui.min_rect().width();
                        ui.add_space(439.0 - used);
                        skin::check(ui, &mut self.separate, tr("启用X/Y"));
                    }
                });
                ui.add_space(14.0);
                let bar = 534.0;
                for index in 0..5 {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        ui.add_space(8.0);
                        let mut enabled = self.enabled[index];
                        if skin::check(ui, &mut enabled, "").changed() {
                            self.toggle(index, enabled);
                        }
                        ui.add_space(15.0);
                        let active = self.enabled[index];
                        ui.add_enabled_ui(active, |ui| {
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = 2.0;
                                skin::trackbar(
                                    ui,
                                    &mut self.dpi[index][axis],
                                    (100, 6200),
                                    100,
                                    bar,
                                );
                                ui.horizontal(|ui| {
                                    ui.set_width(bar);
                                    let gray = |text| {
                                        RichText::new(text)
                                            .font(skin::regular(13.0))
                                            .color(skin::DISABLED)
                                    };
                                    ui.label(gray("100"));
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| ui.label(gray("6200")),
                                    );
                                });
                            });
                            ui.add_space(12.0);
                            let text = self.dpi[index][axis].to_string();
                            if skin::value(ui, &text, self.selected == index, [76.0, 30.0])
                                .on_hover_text(tr("点击设为当前档位"))
                                .clicked()
                            {
                                self.selected = index;
                            }
                        });
                    });
                    ui.add_space(30.0);
                }
            });
    }

    // At least one preset stays enabled. The current preset must be enabled.
    fn toggle(&mut self, index: usize, enabled: bool) {
        if !enabled && self.enabled.iter().filter(|e| **e).count() == 1 {
            return;
        }
        self.enabled[index] = enabled;
        if !self.enabled[self.selected] {
            self.selected = self.enabled.iter().position(|e| *e).unwrap_or(0);
        }
    }

    /// Report rate stored in sensitivity word 1e. See docs/research/WRITES.md.
    pub(super) fn rate(&mut self, ui: &mut egui::Ui) {
        const RATES: [u32; 4] = [125, 250, 500, 1000];
        ui.add_space(66.0);
        let saved = self.device.as_ref().and_then(|device| {
            let code = device.backup.sensitivity_template().ok()?[0x1e];
            RATES.into_iter().find(|hz| u32::from(code) * hz == 1000)
        });
        let help = tr("报告率：鼠标每秒向电脑报告位置的次数。点击“应用”保存并立即生效。");
        let mut notes = Vec::new();
        if let Some(device) = &self.device {
            let (code, register) = device.rate;
            let running = match code {
                1 | 2 | 4 | 8 => format!("{} Hz", 1000 / u32::from(code)),
                other => tf!("编码 {other:02x}"),
            };
            notes.push((tf!("当前运行：{running}"), skin::GRAY));
            if register & 0xc0 == 0x80 {
                notes.push((
                    tr("硬件切换键的回报率设置仍启用。重新插入后，鼠标会使用该设置。").into(),
                    skin::CAPTION_RED,
                ));
            }
        } else {
            notes.push((tr("请先连接鼠标。").into(), skin::GRAY));
        }
        let shown = self.rate_draft.or(saved);
        skin::card(
            ui,
            Some(skin::Icon::Rate),
            tr("报告率（Hz)"),
            help,
            |ui| {
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.add_space(30.0);
                    let width = ui.available_width() - 150.0;
                    ui.add_enabled_ui(self.device.is_some(), |ui| {
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 2.0;
                            let mut index = shown
                                .and_then(|hz| RATES.iter().position(|r| *r == hz))
                                .unwrap_or(3) as u32;
                            if skin::trackbar(ui, &mut index, (0, 3), 1, width).changed() {
                                self.rate_draft = Some(RATES[index as usize]);
                            }
                            skin::ticks(
                                ui,
                                width,
                                &[
                                    (0.0, "125"),
                                    (1.0 / 3.0, "250"),
                                    (2.0 / 3.0, "500"),
                                    (1.0, "1000"),
                                ],
                            );
                        });
                    });
                    ui.add_space(40.0);
                    let text = shown.map_or("—".into(), |hz| hz.to_string());
                    skin::value(ui, &text, false, [76.0, 30.0]);
                });
                for (note, color) in &notes {
                    ui.label(
                        RichText::new(tr_text(note))
                            .font(skin::regular(13.0))
                            .color(*color),
                    );
                }
            },
        );
        let changed = self.rate_draft.is_some() && self.rate_draft != saved;
        let clicked = skin::footer(
            ui,
            self.footer_area,
            &[
                (tr("恢复默认"), self.device.is_some()),
                (tr("取消"), self.rate_draft.is_some()),
                (tr("应用"), changed && !self.pending),
            ],
        );
        if clicked[0] {
            self.rate_draft = Some(1000);
        }
        if clicked[1] {
            self.rate_draft = None;
        }
        if clicked[2]
            && let Some(hz) = self.rate_draft
        {
            match self.rate_block(hz) {
                Ok(words) => {
                    self.rate_draft = None;
                    self.start(Job::SaveRate(words));
                }
                Err(error) => self.report(Err(error)),
            }
        }
    }

    /// MouseLedBrightness: levels 0 to 3, applied immediately.
    pub(super) fn brightness(&mut self, ui: &mut egui::Ui) {
        ui.add_space(66.0);
        let current = self.device.as_ref().map(|device| device.brightness);
        let help = tr(
            "灯光亮度调整：\n提供鼠标四种背光亮度的设置。A90 是单色灯光，RGB 与灯效编辑不适用于此型号。",
        );
        let mut chosen = None;
        skin::card(ui, Some(skin::Icon::Light), tr("灯光亮度"), help, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let width = 806.0;
                ui.add_space(((ui.available_width() - width) / 2.0).max(0.0));
                ui.add_enabled_ui(current.is_some() && !self.pending, |ui| {
                    chosen = skin::levels(ui, current, width);
                });
            });
            ui.add_space(110.0);
            if current.is_none() {
                ui.label(RichText::new(tr("请先连接鼠标。")).color(skin::GRAY));
            }
        });
        if skin::footer(
            ui,
            self.footer_area,
            &[(tr("恢复默认"), current.is_some() && !self.pending)],
        )[0]
        {
            chosen = Some(3);
        }
        if let Some(level) = chosen
            && Some(level) != current
        {
            self.start(Job::Brightness(level));
        }
    }
}

/// The glowing sensor picture of SensitivitySensorLOD, 164 × 158.
fn sensor(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(vec2(164.0, 158.0), egui::Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 10, egui::Color32::from_rgb(60, 61, 65));
    let inner = rect.shrink(2.0);
    painter.rect_filled(inner, 9, egui::Color32::from_rgb(40, 14, 16));
    for i in 1..16 {
        let t = i as f32 / 16.0;
        let x = inner.left() + inner.width() * t;
        let y = inner.top() + inner.height() * t;
        let grid = egui::Stroke::new(1.0, egui::Color32::from_rgb(60, 18, 20));
        painter.line_segment(
            [egui::pos2(x, inner.top()), egui::pos2(x, inner.bottom())],
            grid,
        );
        painter.line_segment(
            [egui::pos2(inner.left(), y), egui::pos2(inner.right(), y)],
            grid,
        );
    }
    let c = rect.center();
    for (radius, alpha) in [(70.0, 30), (52.0, 50), (40.0, 80), (30.0, 140)] {
        painter.circle_filled(
            c,
            radius,
            egui::Color32::from_rgba_unmultiplied(220, 20, 30, alpha),
        );
    }
    painter.circle_stroke(
        c,
        30.0,
        egui::Stroke::new(1.5, egui::Color32::from_rgb(240, 60, 70)),
    );
    painter.circle_filled(c, 10.0, egui::Color32::from_rgb(235, 20, 30));
}

/// Vertical LOD bar: 16 rungs, level 16 at the top.
fn ladder(ui: &mut egui::Ui, level: &mut u8) {
    let (rect, response) =
        ui.allocate_exact_size(vec2(136.0, 336.0), egui::Sense::click_and_drag());
    let enabled = ui.is_enabled();
    let top = rect.top() + 12.0;
    let span = rect.height() - 24.0;
    if enabled
        && (response.dragged() || response.clicked())
        && let Some(pointer) = response.interact_pointer_pos()
    {
        let fraction = ((pointer.y - top) / span).clamp(0.0, 1.0);
        *level = 16 - (fraction * 15.0).round() as u8;
    }
    let painter = ui.painter();
    painter.rect_filled(rect, 0, egui::Color32::from_rgb(44, 45, 49));
    let red = if enabled {
        egui::Color32::from_rgb(120, 10, 16)
    } else {
        egui::Color32::from_rgb(70, 30, 32)
    };
    for i in 0..16 {
        let y = top + span * i as f32 / 15.0;
        painter.line_segment(
            [
                egui::pos2(rect.left() + 14.0, y),
                egui::pos2(rect.right() - 14.0, y),
            ],
            egui::Stroke::new(1.5, red),
        );
    }
    let x = rect.center().x;
    let groove = egui::Rect::from_min_max(
        egui::pos2(x - 5.0, top - 6.0),
        egui::pos2(x + 5.0, top + span + 6.0),
    );
    painter.rect_filled(groove, 5, egui::Color32::from_rgb(25, 25, 27));
    let y = top + span * f32::from(16 - (*level).clamp(1, 16)) / 15.0;
    let thumb = egui::Rect::from_center_size(egui::pos2(x, y), vec2(9.0, 12.0));
    let fill = if enabled {
        egui::Color32::from_rgb(210, 23, 30)
    } else {
        egui::Color32::from_rgb(90, 40, 42)
    };
    painter.rect_filled(thumb, 1, fill);
}

/// Side view of a mouse above the surface lines. The lit band marks the
/// chosen lift height.
fn side_view(ui: &mut egui::Ui, level: Option<u8>) {
    let (rect, _) = ui.allocate_exact_size(vec2(560.0, 370.0), egui::Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 0, egui::Color32::from_rgb(44, 45, 49));
    let base = rect.left_top() + vec2(45.0, 0.0);
    let line = egui::Stroke::new(1.0, egui::Color32::from_rgb(70, 71, 75));
    painter.line_segment([base + vec2(0.0, 16.0), base + vec2(0.0, 360.0)], line);
    for i in 0..9 {
        let y = rect.top() + 196.0 + i as f32 * 20.0;
        let stroke = match level {
            Some(level) if i as u8 == 8 - (level - 1) / 2 => {
                egui::Stroke::new(2.0, egui::Color32::from_rgb(200, 10, 20))
            }
            _ => line,
        };
        painter.line_segment(
            [egui::pos2(base.x, y), egui::pos2(rect.right() - 140.0, y)],
            stroke,
        );
    }
    let outline = egui::Stroke::new(1.5, egui::Color32::from_rgb(190, 190, 192));
    let p = |x: f32, y: f32| rect.left_top() + vec2(x, y);
    let curve = |points: [egui::Pos2; 4]| {
        egui::Shape::CubicBezier(egui::epaint::CubicBezierShape::from_points_stroke(
            points,
            false,
            egui::Color32::TRANSPARENT,
            outline,
        ))
    };
    painter.add(curve([
        p(80.0, 110.0),
        p(160.0, 20.0),
        p(380.0, 10.0),
        p(490.0, 130.0),
    ]));
    painter.add(curve([
        p(490.0, 130.0),
        p(510.0, 160.0),
        p(480.0, 185.0),
        p(420.0, 190.0),
    ]));
    painter.line_segment([p(420.0, 190.0), p(150.0, 190.0)], outline);
    painter.add(curve([
        p(150.0, 190.0),
        p(110.0, 185.0),
        p(70.0, 140.0),
        p(80.0, 110.0),
    ]));
    painter.add(curve([
        p(140.0, 100.0),
        p(220.0, 70.0),
        p(300.0, 80.0),
        p(360.0, 100.0),
    ]));
    painter.add(curve([
        p(200.0, 125.0),
        p(240.0, 110.0),
        p(300.0, 110.0),
        p(340.0, 125.0),
    ]));
    painter.line_segment([p(20.0, 152.0), p(80.0, 152.0)], outline);
    painter.line_segment([p(20.0, 160.0), p(78.0, 160.0)], outline);
}
