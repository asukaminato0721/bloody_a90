use super::App;
use crate::{skin, worker::Job};
use bloody_a90::associations::Rule;
use eframe::egui::{self, RichText};

fn profile(ui: &mut egui::Ui, id: impl std::hash::Hash + std::fmt::Debug, value: &mut u8) {
    egui::ComboBox::from_id_salt(id)
        .selected_text(format!("配置 {}", *value + 1))
        .show_ui(ui, |ui| {
            for index in 0..7 {
                ui.selectable_value(value, index, format!("配置 {}", index + 1));
            }
        });
}

impl App {
    pub(super) fn associations_page(&mut self, ui: &mut egui::Ui) {
        ui.heading("应用程序配置关联");
        ui.label("支持 KDE Plasma Wayland 和 X11。使用窗口的应用标识，不使用窗口标题。");
        ui.label("无匹配项时使用默认配置。应用保持不变时，可以用鼠标按键手动切换配置。");
        ui.horizontal(|ui| {
            ui.label(format!(
                "前台应用：{}",
                self.foreground.as_deref().unwrap_or("尚未读取")
            ));
            if skin::button(ui, "读取前台应用", 130.0, !self.pending).clicked() {
                self.association_error = None;
                self.start(Job::Foreground);
            }
        });
        if let Some(error) = &self.association_error {
            ui.label(RichText::new(error).color(skin::CAPTION_RED));
            ui.label("自动切换已停止。处理错误后，点击“读取前台应用”重试。");
        }
        ui.add_space(12.0);
        ui.checkbox(&mut self.associations_draft.enabled, "启用自动配置切换");
        ui.horizontal(|ui| {
            ui.label("默认配置");
            profile(
                ui,
                "association-default",
                &mut self.associations_draft.fallback,
            );
            if let Some(device) = &self.device {
                ui.label(format!(
                    "鼠标当前配置：{} / {}",
                    device.profile.0 + 1,
                    device.profile.1 + 1
                ));
            }
        });
        let mut remove = None;
        egui::Grid::new("association-rules")
            .striped(true)
            .show(ui, |ui| {
                ui.strong("应用标识");
                ui.strong("鼠标配置");
                ui.end_row();
                for (index, rule) in self.associations_draft.entries.iter_mut().enumerate() {
                    ui.add(egui::TextEdit::singleline(&mut rule.app).desired_width(320.0));
                    profile(ui, ("association-profile", index), &mut rule.profile);
                    if ui.button("删除").clicked() {
                        remove = Some(index);
                    }
                    ui.end_row();
                }
            });
        if let Some(index) = remove {
            self.associations_draft.entries.remove(index);
        }
        if skin::button(ui, "添加应用", 110.0, true).clicked() {
            self.associations_draft.entries.push(Rule {
                app: self.foreground.clone().unwrap_or_default(),
                profile: self.profile.min(6) as u8,
            });
        }
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            if skin::button(ui, "应用", 110.0, !self.pending).clicked() {
                match self.associations_draft.validate() {
                    Ok(()) => {
                        self.associations = self.associations_draft.clone();
                        self.association_error = None;
                        self.start(Job::Associations(self.associations.clone()));
                        self.save_state();
                    }
                    Err(error) => self.report(Err(error)),
                }
            }
            if skin::button(ui, "取消修改", 110.0, !self.pending).clicked() {
                self.associations_draft = self.associations.clone();
            }
            ui.label(if self.association_error.is_some() {
                "自动切换已停止"
            } else if self.associations.enabled && !self.connected() {
                "等待鼠标连接"
            } else if self.associations.enabled {
                "自动切换已启用"
            } else {
                "自动切换已关闭"
            });
        });
    }
}
