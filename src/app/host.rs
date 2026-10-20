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
    fn host_page(&mut self, ui: &mut egui::Ui, field: Field) {
        if self.host.is_none() && !self.pending {
            self.start(Job::HostRead);
        }
        let mut apply = false;
        skin::sheet().show(ui, |ui| {
            ui.set_width(ui.available_width());
            let (title, unit) = match field {
                Field::DoubleClick => ("双击速度", "ms"),
                Field::Lines => ("一次滚动行数", "行"),
                Field::Pointer => ("指针移动速度", ""),
            };
            ui.label(RichText::new(format!("{title} :")).font(skin::bold(17.0)));
            ui.add_space(8.0);
            let Some(draft) = self.host_draft.as_mut() else {
                ui.label(RichText::new("正在读取桌面设置…").color(skin::GRAY));
                return;
            };
            // Track bars run from slow on the left to fast on the right.
            let (mut value, range, step, text) = match field {
                Field::DoubleClick => {
                    let Some(ms) = draft.double_click_ms else {
                        ui.label(RichText::new("当前桌面不提供此设置。").color(skin::GRAY));
                        return;
                    };
                    (
                        1100 - ms.clamp(200, 900),
                        (200, 900),
                        10,
                        format!("{ms} {unit}"),
                    )
                }
                Field::Lines => {
                    let Some(lines) = draft.scroll_lines else {
                        ui.label(RichText::new("当前桌面不提供此设置。").color(skin::GRAY));
                        return;
                    };
                    (lines.clamp(1, 30), (1, 30), 1, format!("{lines} {unit}"))
                }
                Field::Pointer => {
                    let Some(speed) = draft.pointer else {
                        ui.label(
                            RichText::new("当前桌面不提供此设置，或没有找到 A90。")
                                .color(skin::GRAY),
                        );
                        return;
                    };
                    let step = ((speed.clamp(-1.0, 1.0) + 1.0) * 10.0).round() as u32;
                    (step, (0, 20), 1, format!("{}", step + 1))
                }
            };
            ui.horizontal(|ui| {
                ui.label(RichText::new("慢").color(skin::LIGHT_GRAY));
                if skin::trackbar(ui, &mut value, range, step, 520.0).changed() {
                    match field {
                        Field::DoubleClick => draft.double_click_ms = Some(1100 - value),
                        Field::Lines => draft.scroll_lines = Some(value),
                        Field::Pointer => draft.pointer = Some(value as f64 / 10.0 - 1.0),
                    }
                }
                ui.label(RichText::new("快").color(skin::LIGHT_GRAY));
                skin::value(ui, &text, true, [90.0, 31.0]);
            });
            ui.add_space(8.0);
            let target = match draft.desktop {
                Desktop::Kde => "KDE Plasma",
                Desktop::Gnome => "GNOME",
                Desktop::Other => "当前桌面",
            };
            ui.label(
                RichText::new(format!("写入 {target} 的鼠标设置，不写入鼠标。"))
                    .font(skin::regular(13.0))
                    .color(skin::GRAY),
            );
        });
        ui.add_space(14.0);
        let changed = self.host_draft != self.host;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                apply = skin::button(ui, "应用", 140.0, changed && !self.pending).clicked();
                if skin::button(ui, "取消", 140.0, changed).clicked() {
                    self.host_draft = self.host.clone();
                }
                if skin::button(ui, "恢复默认", 140.0, self.host_draft.is_some()).clicked()
                    && let Some(draft) = self.host_draft.as_mut()
                {
                    reset(draft, field);
                }
            });
        });
        if apply && let Some(draft) = self.host_draft.clone() {
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

    pub(super) fn response(&mut self, ui: &mut egui::Ui) {
        skin::sheet().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new("硬件按键响应时间 :").font(skin::bold(17.0)));
            let Some(config) = &mut self.keys else {
                ui.label("请先连接鼠标或读取备份。");
                return;
            };
            let previous = config.debounce_ms();
            let mut ms = previous.unwrap_or(0);
            if previous.is_none() {
                ui.label("当前编码不在已确认的范围内。选择时间后可替换此设置。");
            }
            ui.horizontal(|ui| {
                if ui
                    .add(egui::Slider::new(&mut ms, 0..=16).suffix(" ms"))
                    .changed()
                {
                    let _ = config.set_debounce_ms(ms);
                    self.keys_dirty = true;
                }
                if skin::button(ui, "默认 0 ms", 120.0, !self.pending).clicked() {
                    let _ = config.set_debounce_ms(0);
                    self.keys_dirty = true;
                }
            });
            ui.label("较长的响应时间可以过滤按键触点的抖动。此设置写入鼠标。");
            if skin::button(
                ui,
                "应用硬件设置",
                160.0,
                self.device.is_some() && !self.pending,
            )
            .on_hover_text("同时保存“按键”页中尚未应用的修改。")
            .clicked()
            {
                let config = config.clone();
                self.start(Job::SaveButtons(Box::new(config)));
            }
        });
        ui.add_space(12.0);
        self.host_page(ui, Field::DoubleClick);
    }

    pub(super) fn wheel(&mut self, ui: &mut egui::Ui) {
        self.host_page(ui, Field::Lines);
        ui.add_space(14.0);
        self.evolutionary_wheel(ui);
    }

    pub(super) fn pointer(&mut self, ui: &mut egui::Ui) {
        self.host_page(ui, Field::Pointer);
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
