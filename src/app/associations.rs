use super::App;
use crate::{skin, worker::Job};
use bloody_a90::associations::Rule;
use bloody_a90::{
    i18n::{tr, tr_text},
    tf,
};
use eframe::egui::{self, RichText};

fn profile(ui: &mut egui::Ui, id: impl std::hash::Hash + std::fmt::Debug, value: &mut u8) {
    egui::ComboBox::from_id_salt(id)
        .selected_text(tf!("配置 {}", *value + 1))
        .show_ui(ui, |ui| {
            for index in 0..7 {
                ui.selectable_value(value, index, tf!("配置 {}", index + 1));
            }
        });
}

impl App {
    pub(super) fn associations_page(&mut self, ui: &mut egui::Ui) {
        skin::sheet().show(ui, |ui| {
            ui.set_width(ui.available_width());
            self.associations_body(ui);
        });
    }

    fn associations_body(&mut self, ui: &mut egui::Ui) {
        ui.label(egui::RichText::new(tr("应用程序配置关联")).font(skin::regular(17.0)));
        ui.label(tr(
            "支持 KDE Plasma Wayland 和 X11。使用窗口的应用标识，不使用窗口标题。",
        ));
        ui.label(tr(
            "无匹配项时使用默认配置。应用保持不变时，可以用鼠标按键手动切换配置。",
        ));
        ui.horizontal(|ui| {
            ui.label(tf!(
                "前台应用：{}",
                self.foreground.as_deref().unwrap_or(tr("尚未读取"))
            ));
            if skin::button(ui, tr("读取前台应用"), 130.0, !self.pending).clicked() {
                self.association_error = None;
                self.start(Job::Foreground);
            }
        });
        if let Some(error) = &self.association_error {
            ui.label(RichText::new(tr_text(error)).color(skin::CAPTION_RED));
            ui.label(tr("自动切换已停止。处理错误后，点击“读取前台应用”重试。"));
        }
        ui.add_space(12.0);
        ui.checkbox(&mut self.associations_draft.enabled, tr("启用自动配置切换"));
        ui.horizontal(|ui| {
            ui.label(tr("默认配置"));
            profile(
                ui,
                "association-default",
                &mut self.associations_draft.fallback,
            );
            if let Some(device) = &self.device {
                ui.label(tf!(
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
                ui.strong(tr("应用标识"));
                ui.strong(tr("鼠标配置"));
                ui.end_row();
                for (index, rule) in self.associations_draft.entries.iter_mut().enumerate() {
                    ui.add(egui::TextEdit::singleline(&mut rule.app).desired_width(320.0));
                    profile(ui, ("association-profile", index), &mut rule.profile);
                    if ui.button(tr("删除")).clicked() {
                        remove = Some(index);
                    }
                    ui.end_row();
                }
            });
        if let Some(index) = remove {
            self.associations_draft.entries.remove(index);
        }
        if skin::button(ui, tr("添加应用"), 110.0, true).clicked() {
            self.associations_draft.entries.push(Rule {
                app: self.foreground.clone().unwrap_or_default(),
                profile: self.profile.min(6) as u8,
            });
        }
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            if skin::button(ui, tr("应用"), 110.0, !self.pending).clicked() {
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
            if skin::button(ui, tr("取消修改"), 110.0, !self.pending).clicked() {
                self.associations_draft = self.associations.clone();
            }
            ui.label(if self.association_error.is_some() {
                tr("自动切换已停止")
            } else if self.associations.enabled && !self.connected() {
                tr("等待鼠标连接")
            } else if self.associations.enabled {
                tr("自动切换已启用")
            } else {
                tr("自动切换已关闭")
            });
        });
    }
}
