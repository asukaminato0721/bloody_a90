use super::{App, path_field};
use crate::{skin, worker::Job};
use bloody_a90::i18n::{tr, tr_text};
use bloody_a90::{configuration::Configuration, gestures::Action, host_buttons::Settings};
use eframe::egui::{self, RichText};

pub(super) struct Editor {
    pub rules: Settings,
    pub applied: Settings,
    pub expected_file: Option<Vec<u8>>,
    pub running: bool,
    pub status: String,
    pub last_poll: std::time::Instant,
    kind: usize,
    target: String,
    arguments: String,
    text: String,
}

impl Default for Editor {
    fn default() -> Self {
        Self {
            rules: Settings::default(),
            applied: Settings::default(),
            expected_file: None,
            running: false,
            status: tr("按键主机动作未启用").into(),
            last_poll: std::time::Instant::now(),
            kind: 0,
            target: String::new(),
            arguments: String::new(),
            text: String::new(),
        }
    }
}

impl Editor {
    pub fn select(&mut self, action: &Action) {
        if action.number("Simulator") == Ok(9) && action.number("Object") == Ok(9) {
            self.kind = if action.number("Action") == Ok(5) {
                9
            } else {
                10
            };
            return;
        }
        self.kind = match (
            action.number("Simulator").unwrap_or(0),
            action.number("Object").unwrap_or(0),
        ) {
            (20, _) => {
                self.target = action.get("MacroName").into();
                self.arguments = action.get("LinuxArguments").into();
                0
            }
            (21, _) => {
                self.text = action.get("HintText").into();
                1
            }
            (22, 0) => 2,
            (22, _) => 3,
            (24, 1) => 4,
            (24, 0) => 8,
            (2, 0) => 6,
            (2, _) => 7,
            _ => 5,
        };
    }

    pub fn load(&mut self, settings: Result<Settings, String>, config: &Configuration) {
        match settings {
            Ok(mut settings) => {
                settings.reconcile(config);
                self.applied = settings.clone();
                self.rules = settings;
            }
            Err(error) => {
                self.status = error;
                self.rules = Settings::default();
                self.rules.resize(config.profiles.len());
                self.applied = self.rules.clone();
            }
        }
    }

    pub fn action(&self) -> Result<Action, String> {
        let action = match self.kind {
            0 => Action::open_target_with_arguments(&self.target, &self.arguments)?,
            1 => Action::input_text(&self.text)?,
            2 => Action::new(22, 0, 0, tr("放大")),
            3 => Action::new(22, 1, 0, tr("缩小")),
            4 => Action::new(24, 1, 0, tr("最小化指针下的窗口")),
            5 => Action::new(24, 2, 0, tr("最大化指针下的窗口")),
            6 => Action::new(2, 0, 0, tr("16 合一手势")),
            7 => Action::new(2, 1, 0, tr("鼠标手势")),
            8 => Action::new(24, 0, 0, tr("全部最小化")),
            9 => Action::new(9, 9, 5, tr("滚轮向左")),
            10 => Action::new(9, 9, 6, tr("滚轮向右")),
            _ => return Err(tr("无效的主机动作").into()),
        };
        let action = if matches!(self.kind, 9 | 10) {
            action
        } else {
            action.with_mode(0)
        };
        if !matches!(self.kind, 6 | 7) {
            bloody_a90::host_program::prepare_button(&action, true)?;
        }
        Ok(action)
    }

    pub fn edit(&mut self, ui: &mut egui::Ui, allow_gestures: bool) {
        if !allow_gestures && matches!(self.kind, 6 | 7) {
            self.kind = 0;
        }
        let names = [
            tr("打开文件或程序"),
            tr("输入文字"),
            tr("应用放大"),
            tr("应用缩小"),
            tr("最小化指针下的窗口"),
            tr("最大化指针下的窗口"),
            tr("16 合一手势"),
            tr("鼠标手势"),
            tr("全部最小化"),
            tr("滚轮向左"),
            tr("滚轮向右"),
        ];
        egui::ComboBox::from_id_salt("host-button-kind")
            .selected_text(tr_text(names[self.kind]))
            .show_ui(ui, |ui| {
                for (index, name) in names
                    .into_iter()
                    .enumerate()
                    .filter(|(index, _)| allow_gestures || !matches!(index, 6 | 7))
                {
                    ui.selectable_value(&mut self.kind, index, tr_text(name));
                }
            });
        match self.kind {
            0 => {
                path_field(ui, &mut self.target, tr("文件、程序路径或 URL"), 350.0);
                ui.add(
                    egui::TextEdit::multiline(&mut self.arguments)
                        .hint_text(tr("每行一个程序参数，可留空"))
                        .desired_rows(3)
                        .desired_width(350.0),
                );
                ui.label(tr("截图、桌面画笔和语音可调用已安装的 Linux 应用。"));
            }
            1 => {
                ui.add(
                    egui::TextEdit::multiline(&mut self.text)
                        .hint_text(tr("按下时输入的文字"))
                        .desired_rows(3)
                        .desired_width(350.0),
                );
            }
            2 | 3 => {
                ui.label(tr("应用需支持 Ctrl+滚轮缩放。"));
            }
            6 | 7 => {
                ui.label(tr("按住此键移动鼠标。设定此按键时，载入手势页的当前动作。修改方向后，在手势页重新应用。"));
            }
            8 => {
                ui.label(tr(
                    "最小化当前桌面和活动中的所有可最小化窗口。需要 KDE Wayland。",
                ));
            }
            9 | 10 => {
                ui.label(tr("每次按下或每格滚动执行一格水平滚动。"));
            }
            _ => {}
        }
        ui.label(
            RichText::new(tr(
                "主机动作需要本程序保持运行。手势按住绘图，其他动作按下执行。",
            ))
            .color(skin::GRAY),
        );
    }
}

impl App {
    pub(super) fn host_button_status(
        &mut self,
        result: Result<bloody_a90::gesture_runtime::Status, String>,
    ) {
        match result {
            Ok(status) => {
                self.host_buttons.running = status.running;
                self.host_buttons.status = status.message;
            }
            Err(error) => {
                self.host_buttons.running = false;
                self.host_buttons.status = error.clone();
                self.report(Err(error));
            }
        }
    }

    pub(super) fn host_button_controls(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.label(tr_text(&self.host_buttons.status));
            if skin::button(
                ui,
                if self.host_buttons.running {
                    tr("停止主机动作")
                } else {
                    tr("启用已保存的主机动作")
                },
                190.0,
                !self.pending && self.connected() && !self.host_buttons.applied.is_empty(),
            )
            .clicked()
            {
                let enabled = !self.host_buttons.running;
                self.start(Job::HostButtonsEnabled(
                    enabled,
                    self.host_buttons.expected_file.clone(),
                ));
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_minimize_remains_available_for_wheel_actions_without_gesture_choices() {
        let context = egui::Context::default();
        let mut editor = Editor::default();
        let action = Action::new(24, 0, 0, "全部最小化");
        editor.select(&action);
        let mut output = context.run_ui(Default::default(), |ui| editor.edit(ui, false));
        output.textures_delta.clear();
        assert_eq!(editor.action().unwrap(), action);
        for mode in [5, 6] {
            let action = Action::new(
                9,
                9,
                mode,
                if mode == 5 {
                    "滚轮向左"
                } else {
                    "滚轮向右"
                },
            );
            editor.select(&action);
            let mut output = context.run_ui(Default::default(), |ui| editor.edit(ui, false));
            output.textures_delta.clear();
            assert_eq!(editor.action().unwrap(), action);
        }
        editor.select(&Action::new(2, 0, 0, "手势"));
        let mut output = context.run_ui(Default::default(), |ui| editor.edit(ui, false));
        output.textures_delta.clear();
        assert_ne!(editor.kind, 6);
    }
}
