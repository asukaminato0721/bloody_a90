use super::{App, DEFAULT_DPI};
use crate::{skin, worker::Job};
use bloody_a90::sensitivity;
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
        self.device.as_ref().ok_or("鼠标未连接")?;
        let template = self
            .sensitivity_template
            .as_deref()
            .ok_or("尚未读取灵敏度设置")?;
        let (presets, selected) = self.active_presets();
        sensitivity::block(template, &presets, selected, None)
    }

    pub(super) fn lift_block(&self) -> Result<Vec<u16>, String> {
        let template = self
            .device
            .as_ref()
            .ok_or("鼠标未连接")?
            .backup
            .sensitivity_template()?;
        sensitivity::with_lift_height(&template, self.lift_enabled.then_some(self.lift_level))
    }

    pub(super) fn rate_block(&self, hz: u32) -> Result<Vec<u16>, String> {
        let template = self
            .device
            .as_ref()
            .ok_or("鼠标未连接")?
            .backup
            .sensitivity_template()?;
        sensitivity::with_rate(&template, hz)
    }

    pub(super) fn lift(&mut self, ui: &mut egui::Ui) {
        skin::sheet().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new("抬起高度 :").font(skin::bold(17.0)));
            skin::check(ui, &mut self.lift_enabled, "启用抬起高度限制");
            ui.add_enabled_ui(self.lift_enabled, |ui| {
                ui.horizontal(|ui| {
                    ui.label("低");
                    ui.add(egui::Slider::new(&mut self.lift_level, 1..=16).suffix(" 级"));
                    ui.label("高");
                });
            });
            ui.label("鼠标抬起后，传感器停止报告位移。等级不是毫米数值。");
            ui.label("将鼠标放回使用表面后检查移动。点击“应用”保存并立即生效。");
            ui.add_space(8.0);
            if skin::button(
                ui,
                "开始 5 秒校准",
                170.0,
                self.connected() && !self.pending,
            )
            .clicked()
            {
                self.start(Job::CalibrateLift);
                self.status = "正在校准。请在使用表面上持续移动鼠标，保持鼠标接触表面。".into();
            }
            ui.label("校准时保持鼠标接触使用表面。校准结果先载入此页，再点击“应用”保存。");
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if skin::button(ui, "应用", 140.0, self.connected() && !self.pending).clicked() {
                    match self.lift_block() {
                        Ok(words) => self.start(Job::SaveLift(words)),
                        Err(error) => self.report(Err(error)),
                    }
                }
                if skin::button(ui, "取消", 140.0, true).clicked()
                    && let Some(source) = &self.source
                    && let Ok(level) = source
                        .sensitivity_template()
                        .and_then(|words| sensitivity::lift_height(&words))
                {
                    self.lift_enabled = level.is_some();
                    self.lift_level = level.unwrap_or(16);
                }
                if skin::button(ui, "恢复默认", 140.0, true).clicked() {
                    self.lift_enabled = false;
                    self.lift_level = 16;
                }
            });
        });
    }

    pub(super) fn resolution(&mut self, ui: &mut egui::Ui) {
        if !self.separate {
            for axes in &mut self.dpi {
                axes[1] = axes[0];
            }
        }
        let current = self.dpi[self.selected];
        ui.horizontal(|ui| {
            ui.label(RichText::new("目前 CPI :").font(skin::regular(16.0)));
            skin::value(ui, &format!("X {}", current[0]), false, [110.0, 28.0]);
            skin::value(ui, &format!("Y {}", current[1]), false, [110.0, 28.0]);
        });
        ui.add_space(8.0);
        let width = ((ui.available_width() - 16.0) / 2.0).max(420.0);
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 16.0;
            for axis in 0..2 {
                ui.vertical(|ui| self.sheet(ui, axis, width));
            }
        });
        ui.add_space(14.0);
        let block = self.sensitivity_block();
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let hint = match &block {
                    Ok(_) => String::new(),
                    Err(error) => error.clone(),
                };
                if skin::button(ui, "应用", 140.0, block.is_ok() && !self.pending)
                    .on_disabled_hover_text(hint)
                    .clicked()
                    && let Ok(words) = &block
                {
                    self.start(Job::SaveSensitivity(words.clone()));
                }
                if skin::button(ui, "取消", 140.0, true)
                    .on_hover_text("恢复为鼠标中的设置")
                    .clicked()
                {
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
                if skin::button(ui, "恢复默认", 140.0, true).clicked() {
                    self.reset_dpi();
                }
            });
        });
        ui.add_space(8.0);
        egui::CollapsingHeader::new("查看编码参数").show(ui, |ui| {
            egui::Grid::new("dpi-encoded")
                .striped(true)
                .min_col_width(90.0)
                .show(ui, |ui| {
                    for label in ["档位", "X 比例", "Y 比例", "传感器编码"] {
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
                                ui.colored_label(skin::ERROR, error);
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

    fn sheet(&mut self, ui: &mut egui::Ui, axis: usize, width: f32) {
        skin::sheet().show(ui, |ui| {
            ui.set_width(width - 32.0);
            ui.horizontal(|ui| {
                skin::axis(ui, axis == 0);
                ui.add_space(6.0);
                ui.label(
                    RichText::new(if axis == 0 {
                        "X轴 设定 :"
                    } else {
                        "Y轴 设定 :"
                    })
                    .font(skin::bold(17.0)),
                );
                if axis == 0 {
                    ui.add_space(24.0);
                    skin::check(ui, &mut self.separate, "启用X/Y");
                }
            });
            ui.add_space(10.0);
            let bar = (width - 32.0 - 30.0 - 96.0).max(200.0);
            ui.add_enabled_ui(axis == 0 || self.separate, |ui| {
                for index in 0..5 {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 12.0;
                        let mut enabled = self.enabled[index];
                        if skin::check(ui, &mut enabled, "").changed() {
                            self.toggle(index, enabled);
                        }
                        let active = self.enabled[index];
                        ui.add_enabled_ui(active, |ui| {
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = 0.0;
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
                                            .color(skin::LIGHT_GRAY)
                                    };
                                    ui.label(gray("100"));
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| ui.label(gray("6200")),
                                    );
                                });
                            });
                            let text = self.dpi[index][axis].to_string();
                            if skin::value(ui, &text, self.selected == index, [77.0, 31.0])
                                .on_hover_text("点击设为当前档位")
                                .clicked()
                            {
                                self.selected = index;
                            }
                        });
                    });
                    ui.add_space(14.0);
                }
            });
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
        let mut save = None;
        skin::sheet().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new("回报率 :").font(skin::bold(17.0)));
            ui.add_space(8.0);
            let Some(device) = &self.device else {
                ui.label(RichText::new("请先连接鼠标。").color(skin::GRAY));
                return;
            };
            let template = device.backup.sensitivity_template();
            let saved = template.as_ref().map_or(0, |words| words[0x1e]);
            let (code, register) = device.rate;
            let running = match code {
                1 | 2 | 4 | 8 => format!("{} Hz", 1000 / u32::from(code)),
                other => format!("编码 {other:02x}"),
            };
            ui.label(format!("当前运行：{running}"));
            if register & 0xc0 == 0x80 {
                ui.label(
                    RichText::new("硬件切换键的回报率设置仍启用。重新插入后，鼠标会使用该设置。")
                        .color(skin::CAPTION_RED),
                );
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = vec2(10.0, 0.0);
                for hz in [125, 250, 500, 1000] {
                    let selected = u32::from(saved) * hz == 1000;
                    if skin::value(ui, &format!("{hz} Hz"), selected, [110.0, 36.0]).clicked()
                        && !selected
                        && !self.pending
                    {
                        save = Some(self.rate_block(hz));
                    }
                }
            });
            ui.add_space(8.0);
            ui.label(
                RichText::new("红色为灵敏度表中保存的值。点击后保存并立即生效。")
                    .font(skin::regular(13.0))
                    .color(skin::GRAY),
            );
        });
        match save {
            Some(Ok(words)) => self.start(Job::SaveRate(words)),
            Some(Err(error)) => self.report(Err(error)),
            None => {}
        }
    }

    /// MouseLedBrightness: levels 0 to 3, applied immediately.
    pub(super) fn brightness(&mut self, ui: &mut egui::Ui) {
        skin::sheet().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new("灯光亮度 :").font(skin::bold(17.0)));
            ui.add_space(8.0);
            let Some(current) = self.device.as_ref().map(|device| device.brightness) else {
                ui.label(RichText::new("请先连接鼠标。").color(skin::GRAY));
                return;
            };
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = vec2(10.0, 0.0);
                for (level, caption) in [(0, "关闭"), (1, "暗"), (2, "中"), (3, "亮")] {
                    let selected = current == level;
                    if skin::value(ui, caption, selected, [110.0, 36.0]).clicked()
                        && !selected
                        && !self.pending
                    {
                        self.start(Job::Brightness(level));
                    }
                }
            });
            ui.add_space(8.0);
            ui.label(
                RichText::new("A90 是单色灯光。RGB 与灯效编辑不适用于此型号。")
                    .font(skin::regular(13.0))
                    .color(skin::GRAY),
            );
        });
    }
}
