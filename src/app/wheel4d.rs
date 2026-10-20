use super::App;
use crate::{
    skin,
    worker::{Job, WheelUpdate},
};
use bloody_a90::{
    gesture_runtime::Status,
    wheel4d::{Application, Settings},
};
use eframe::egui;

#[derive(Default)]
pub(super) struct Editor {
    pub loaded: bool,
    pub applied: Settings,
    draft: Settings,
    expected: Option<Vec<u8>>,
    pub running: bool,
    runtime_enabled: bool,
    message: String,
}

impl App {
    pub(super) fn wheel_runtime_status(&mut self, status: &Status, enabled: bool) {
        self.wheel4d.runtime_enabled = enabled;
        self.wheel_status(status);
    }

    pub(super) fn wheel_reply(&mut self, result: Result<WheelUpdate, String>) {
        self.wheel4d.loaded = true;
        match result {
            Ok(update) => {
                self.wheel4d.applied = update.settings.clone();
                self.wheel4d.draft = update.settings;
                self.wheel4d.expected = update.bytes;
                self.wheel4d.runtime_enabled = update.runtime_enabled;
                let failed = update.runtime_enabled && !update.status.running;
                self.wheel_status(&update.status);
                if failed {
                    self.report(Err(update.status.message));
                } else {
                    self.report(Ok("已读取或应用滚轮设置".into()));
                }
            }
            Err(error) => self.report(Err(error)),
        }
    }

    pub(super) fn wheel_status(&mut self, status: &Status) {
        self.wheel4d.running = self.wheel4d.runtime_enabled && status.running;
        self.wheel4d.message = if self.wheel4d.runtime_enabled {
            if status.running {
                "4-Way Wheel 正在运行".into()
            } else {
                status.message.clone()
            }
        } else {
            "4-Way Wheel 已关闭".into()
        };
    }

    pub(super) fn evolutionary_wheel(&mut self, ui: &mut egui::Ui) {
        if !self.wheel4d.loaded && !self.pending {
            self.start(Job::WheelRead);
        }
        let mut job = None;
        skin::sheet().show(ui, |ui| {
            ui.strong("4-Way Wheel · 区域滚动");
            if !self.wheel4d.loaded {
                ui.label("正在读取滚轮设置…");
                return;
            }
            let editor = &mut self.wheel4d;
            ui.add_enabled_ui(!self.pending, |ui| {
                ui.horizontal(|ui| {
                    ui.label("按鼠标核心启用区域滚动：");
                    for index in 0..4 {
                        ui.checkbox(
                            &mut editor.draft.cores[index],
                            format!("核心 {}", index + 1),
                        );
                    }
                });
                if let Some(device) = &self.device {
                    ui.label(format!("鼠标当前核心：{}", device.identity.core));
                }
                ui.add(
                    egui::Slider::new(&mut editor.draft.divider, 100..=900)
                        .custom_formatter(|v, _| format!("{:.1}%", v / 10.0))
                        .text("左侧区域宽度"),
                );
                ui.checkbox(
                    &mut editor.draft.reverse_regions,
                    "左侧横向滚动，右侧纵向滚动",
                );
                ui.checkbox(&mut editor.draft.direction_icon, "显示横向滚动方向图标");
                ui.checkbox(&mut editor.draft.divider_icon, "显示可拖动分隔图标");
                ui.label("拖动后，程序保存该应用的分界。点击“重新读取”可载入更新后的设置。");
                ui.label("默认左侧纵向滚动，右侧横向滚动。区域按指针所在窗口计算。");
                egui::CollapsingHeader::new("每应用分界")
                    .default_open(self.smoke_frames.is_some())
                    .show(ui, |ui| {
                        ui.label("使用 KDE 应用标识。未匹配的应用使用上方默认分界。");
                        let mut remove = None;
                        egui::Grid::new("wheel-application-dividers")
                            .striped(true)
                            .show(ui, |ui| {
                                ui.label("应用标识");
                                ui.label("左侧区域宽度");
                                ui.end_row();
                                for (index, rule) in
                                    editor.draft.applications.iter_mut().enumerate()
                                {
                                    ui.add(
                                        egui::TextEdit::singleline(&mut rule.app)
                                            .desired_width(300.0)
                                            .hint_text("例如 org.kde.kwrite"),
                                    );
                                    ui.add(
                                        egui::Slider::new(&mut rule.divider, 100..=900)
                                            .custom_formatter(|v, _| format!("{:.1}%", v / 10.0)),
                                    );
                                    if ui.button("删除").clicked() {
                                        remove = Some(index);
                                    }
                                    ui.end_row();
                                }
                            });
                        if let Some(index) = remove {
                            editor.draft.applications.remove(index);
                        }
                        if skin::button(
                            ui,
                            "添加应用分界",
                            160.0,
                            editor.draft.applications.len() < 128,
                        )
                        .clicked()
                        {
                            editor.draft.applications.push(Application {
                                app: String::new(),
                                divider: editor.draft.divider,
                            });
                        }
                    });
                ui.label("适用于 KDE Wayland。桌面和面板保留纵向滚动。关闭本工具会停止区域滚动。");
                ui.label("同时按下右键和中键可停止输入接管。再次点击“应用区域滚动”可恢复。");
                ui.horizontal(|ui| {
                    if skin::button(ui, "应用区域滚动", 170.0, true).clicked() {
                        job = Some(Job::WheelWrite(
                            editor.draft.clone(),
                            editor.expected.clone(),
                        ));
                    }
                    if skin::button(ui, "取消修改", 120.0, editor.draft != editor.applied).clicked()
                    {
                        editor.draft = editor.applied.clone();
                    }
                    if skin::button(ui, "恢复默认", 120.0, true).clicked() {
                        editor.draft = Settings::default();
                    }
                    if skin::button(ui, "重新读取", 120.0, true).clicked() {
                        job = Some(Job::WheelRead);
                    }
                });
            });
            ui.label(&editor.message);
        });
        if let Some(job) = job {
            self.start(job);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wheel_read_does_not_claim_a_saved_setting_is_running() {
        let context = egui::Context::default();
        let mut app = App::from_context(&context);
        let settings = Settings {
            cores: [false, true, false, false],
            ..Default::default()
        };
        app.wheel_reply(Ok(WheelUpdate {
            bytes: Some(settings.to_bytes()),
            settings: settings.clone(),
            status: Status {
                running: true,
                message: "手势运行中".into(),
            },
            runtime_enabled: false,
        }));
        assert!(app.wheel4d.draft.enabled_for(2));
        assert!(!app.wheel4d.running);
        app.wheel4d.runtime_enabled = true;
        app.wheel_status(&Status {
            running: true,
            message: String::new(),
        });
        assert!(app.wheel4d.running);
        app.wheel_runtime_status(
            &Status {
                running: true,
                message: "手势运行中".into(),
            },
            false,
        );
        assert!(!app.wheel4d.running);
        app.wheel_runtime_status(
            &Status {
                running: true,
                message: String::new(),
            },
            true,
        );
        app.wheel_status(&Status {
            running: false,
            message: "输入接管已停止".into(),
        });
        assert!(!app.wheel4d.running);
        assert_eq!(app.wheel4d.draft, settings);
        app.smoke_frames = Some(0);
        app.wheel4d.draft.applications.push(Application {
            app: "org.kde.kwrite".into(),
            divider: 500,
        });
        let mut output = context.run_ui(Default::default(), |ui| app.evolutionary_wheel(ui));
        output.textures_delta.clear();
    }
}
