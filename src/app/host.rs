//! 按键响应 (double-click), 滚轮 and 指针 pages. The official program writes
//! these to Windows; here they go to the desktop through `bloody_a90::host`.
use super::App;
use crate::{skin, worker::Job};
use bloody_a90::host::{Desktop, Settings};
use eframe::egui::{self, RichText};

/// Which desktop value a page edits.
#[derive(Clone, Copy)]
enum Field {
    DoubleClick,
    Lines,
    Pointer,
}

impl App {
    /// The desktop value of one page as a trackbar position, with its range,
    /// step and value text. None when the desktop lacks the setting.
    fn host_value(&self, field: Field) -> Option<(u32, (u32, u32), u32, String)> {
        let draft = self.host_draft.as_ref()?;
        // Track bars run from slow on the left to fast on the right.
        Some(match field {
            Field::DoubleClick => {
                let ms = draft.double_click_ms?;
                (1100 - ms.clamp(200, 900), (200, 900), 10, format!("{ms} ms"))
            }
            Field::Lines => {
                let lines = draft.scroll_lines?;
                (lines.clamp(1, 30), (1, 30), 1, lines.to_string())
            }
            Field::Pointer => {
                let speed = draft.pointer?;
                let step = ((speed.clamp(-1.0, 1.0) + 1.0) * 10.0).round() as u32;
                (step, (0, 20), 1, format!("{}", step + 1))
            }
        })
    }

    fn set_host_value(&mut self, field: Field, value: u32) {
        let Some(draft) = self.host_draft.as_mut() else {
            return;
        };
        match field {
            Field::DoubleClick => draft.double_click_ms = Some(1100 - value),
            Field::Lines => draft.scroll_lines = Some(value),
            Field::Pointer => draft.pointer = Some(value as f64 / 10.0 - 1.0),
        }
    }

    fn host_target(&self) -> &'static str {
        match self.host_draft.as_ref().map(|draft| draft.desktop) {
            Some(Desktop::Kde) => "KDE Plasma",
            Some(Desktop::Gnome) => "GNOME",
            _ => "当前桌面",
        }
    }

    /// 慢 ── trackbar ── 快 of the double-click and pointer groups.
    fn host_slider(&mut self, ui: &mut egui::Ui, field: Field) {
        if self.host.is_none() && !self.pending {
            self.start(Job::HostRead);
        }
        let Some((mut value, range, step, _)) = self.host_value(field) else {
            let text = if self.host_draft.is_none() {
                "正在读取桌面设置…"
            } else {
                "当前桌面不提供此设置，或没有找到 A90。"
            };
            ui.label(RichText::new(text).color(skin::GRAY));
            return;
        };
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 18.0;
            ui.add_space(28.0);
            ui.label(RichText::new("慢").font(skin::regular(17.0)));
            let width = ui.available_width() - 80.0;
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                if skin::trackbar(ui, &mut value, range, step, width).changed() {
                    self.set_host_value(field, value);
                }
                let marks: Vec<(f32, &str)> = (0..=10).map(|i| (i as f32 / 10.0, "")).collect();
                skin::ticks(ui, width, &marks);
            });
            ui.label(RichText::new("快").font(skin::regular(17.0)));
        });
    }

    /// 恢复默认 / 取消 / 应用 of the desktop pages. `hardware` also saves
    /// the button response time of the 按键响应 page.
    fn host_footer(&mut self, ui: &egui::Ui, field: Field, hardware: bool) {
        let changed = self.host_draft != self.host;
        let save_keys = hardware && self.keys_dirty && self.device.is_some();
        let clicked = skin::footer(
            ui,
            self.footer_area,
            &[
                ("恢复默认", self.host_draft.is_some() || hardware),
                ("取消", changed || save_keys),
                ("应用", (changed || save_keys) && !self.pending),
            ],
        );
        if clicked[0] {
            if let Some(draft) = self.host_draft.as_mut() {
                reset(draft, field);
            }
            if hardware && let Some(config) = &mut self.keys {
                let _ = config.set_debounce_ms(0);
                self.keys_dirty = true;
            }
        }
        if clicked[1] {
            self.host_draft = self.host.clone();
            if hardware
                && let Some(source) = self.source.clone()
            {
                self.load_keys(&source);
            }
        }
        if !clicked[2] {
            return;
        }
        if save_keys && let Some(config) = self.keys.clone() {
            self.start(Job::SaveButtons(Box::new(config)));
            return;
        }
        if let Some(draft) = self.host_draft.clone() {
            // Write only what this page edits; other values stay as read.
            let mut settings = Settings {
                pointer: None,
                double_click_ms: None,
                scroll_lines: None,
                ..draft.clone()
            };
            match field {
                Field::DoubleClick => settings.double_click_ms = draft.double_click_ms,
                Field::Lines => settings.scroll_lines = draft.scroll_lines,
                Field::Pointer => settings.pointer = draft.pointer,
            }
            self.start(Job::HostWrite(settings));
        }
    }

    /// SensitivityDebounce: the hardware response time and the desktop
    /// double-click speed.
    pub(super) fn response(&mut self, ui: &mut egui::Ui) {
        ui.add_space(66.0);
        let help = "按键响应：较长的响应时间可以过滤按键触点的抖动。此设置写入鼠标，\
                    并同时保存“按键”页中尚未应用的修改。";
        let mut dirty = false;
        skin::card(ui, Some(skin::Icon::Speed), "按键响应 (ms)", help, |ui| {
            ui.add_space(10.0);
            let Some(config) = &mut self.keys else {
                ui.label(RichText::new("请先连接鼠标或读取备份。").color(skin::GRAY));
                return;
            };
            let previous = config.debounce_ms();
            let mut ms = u32::from(previous.unwrap_or(0));
            ui.horizontal(|ui| {
                ui.add_space(42.0);
                let width = ui.available_width() - 160.0;
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    if skin::trackbar(ui, &mut ms, (0, 16), 1, width).changed() {
                        let _ = config.set_debounce_ms(ms as u8);
                        dirty = true;
                    }
                    let labels: Vec<String> = (0..=16).map(|i| i.to_string()).collect();
                    let marks: Vec<(f32, &str)> = labels
                        .iter()
                        .enumerate()
                        .map(|(i, text)| (i as f32 / 16.0, text.as_str()))
                        .collect();
                    skin::ticks(ui, width, &marks);
                });
                ui.add_space(42.0);
                ui.vertical(|ui| {
                    ui.add_space(-6.0);
                    skin::value(ui, &ms.to_string(), false, [76.0, 30.0]);
                });
            });
            if previous.is_none() {
                ui.label(
                    RichText::new("当前编码不在已确认的范围内。选择时间后可替换此设置。")
                        .color(skin::GRAY),
                );
            }
        });
        if dirty {
            self.keys_dirty = true;
        }
        ui.add_space(56.0);
        let help = format!(
            "双击速度：写入 {} 的鼠标设置，不写入鼠标。",
            self.host_target()
        );
        skin::card(ui, Some(skin::Icon::DoubleClick), "双击速度", &help, |ui| {
            ui.add_space(4.0);
            self.host_slider(ui, Field::DoubleClick);
        });
        self.host_footer(ui, Field::DoubleClick, true);
    }

    /// SensitivityWheel: lines per notch go to the desktop.
    pub(super) fn wheel(&mut self, ui: &mut egui::Ui) {
        if self.host.is_none() && !self.pending {
            self.start(Job::HostRead);
        }
        ui.add_space(66.0);
        let help = format!(
            "滚轮调节：写入 {} 的鼠标设置，不写入鼠标。",
            self.host_target()
        );
        let value = self.host_value(Field::Lines);
        skin::card(ui, None, "滚轮调节", &help, |ui| {
            ui.add_space(-4.0);
            ui.horizontal(|ui| {
                ui.add_space(68.0);
                ui.label(RichText::new("垂直滚动").font(skin::regular(16.0)));
            });
            ui.horizontal(|ui| {
                ui.add_space(48.0);
                egui::Frame::new()
                    .stroke(egui::Stroke::new(3.0, egui::Color32::from_rgb(58, 59, 63)))
                    .corner_radius(12)
                    .inner_margin(egui::Margin::symmetric(14, 8))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            skin::icon(ui, skin::Icon::Wheel);
                            ui.add_space(8.0);
                            ui.vertical(|ui| {
                                ui.add_space(14.0);
                                skin::option(ui, value.is_some(), "一次滚动行数");
                                ui.add_space(6.0);
                                ui.add_enabled_ui(false, |ui| {
                                    skin::option(ui, false, "一次滚动屏幕")
                                        .on_disabled_hover_text("桌面没有提供整页滚动设置。")
                                });
                            });
                            ui.add_space(30.0);
                            match value {
                                Some((mut lines, _, _, _)) => {
                                    if ui
                                        .add(
                                            egui::DragValue::new(&mut lines)
                                                .range(1..=30)
                                                .speed(0.1),
                                        )
                                        .changed()
                                    {
                                        self.set_host_value(Field::Lines, lines);
                                    }
                                }
                                None => {
                                    ui.label(RichText::new("—").color(skin::GRAY));
                                }
                            }
                            ui.add_space(40.0);
                        });
                    });
            });
        });
        self.host_footer(ui, Field::Lines, false);
        ui.add_space(24.0);
        self.evolutionary_wheel(ui);
    }

    /// SensitivityPointer: the pointer speed of this mouse on the desktop.
    pub(super) fn pointer(&mut self, ui: &mut egui::Ui) {
        ui.add_space(66.0);
        let help = format!(
            "指针移动速度：写入 {} 中这只鼠标的加速度设置，不写入鼠标。",
            self.host_target()
        );
        skin::card(ui, Some(skin::Icon::Pointer), "指针移动速度", &help, |ui| {
            ui.add_space(10.0);
            self.host_slider(ui, Field::Pointer);
        });
        self.host_footer(ui, Field::Pointer, false);
    }
}

// Desktop defaults: KDE uses 400 ms and three lines; libinput uses 0.
fn reset(settings: &mut Settings, field: Field) {
    match field {
        Field::DoubleClick => settings.double_click_ms = Some(400),
        Field::Lines => settings.scroll_lines = Some(3),
        Field::Pointer => settings.pointer = Some(0.0),
    }
}
