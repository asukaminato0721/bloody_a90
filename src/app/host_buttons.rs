use super::{App, path_field};
use crate::{skin, worker::Job};
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
            status: "按键主机动作未启用".into(),
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
            2 => Action::new(22, 0, 0, "放大"),
            3 => Action::new(22, 1, 0, "缩小"),
            4 => Action::new(24, 1, 0, "最小化指针下的窗口"),
            5 => Action::new(24, 2, 0, "最大化指针下的窗口"),
            6 => Action::new(2, 0, 0, "16 合一手势"),
            _ => Action::new(2, 1, 0, "鼠标手势"),
        }
        .with_mode(0);
        if self.kind < 6 {
            bloody_a90::host_program::prepare_button(&action, true)?;
        }
        Ok(action)
    }

    pub fn edit(&mut self, ui: &mut egui::Ui, allow_gestures: bool) {
        if !allow_gestures && self.kind >= 6 {
            self.kind = 0;
        }
        let names = [
            "打开文件或程序",
            "输入文字",
            "应用放大",
            "应用缩小",
            "最小化指针下的窗口",
            "最大化指针下的窗口",
            "16 合一手势",
            "鼠标手势",
        ];
        egui::ComboBox::from_id_salt("host-button-kind")
            .selected_text(names[self.kind])
            .show_ui(ui, |ui| {
                for (index, name) in names
                    .into_iter()
                    .take(if allow_gestures { 8 } else { 6 })
                    .enumerate()
                {
                    ui.selectable_value(&mut self.kind, index, name);
                }
            });
        match self.kind {
            0 => {
                path_field(ui, &mut self.target, "文件、程序路径或 URL", 350.0);
                ui.add(
                    egui::TextEdit::multiline(&mut self.arguments)
                        .hint_text("每行一个程序参数，可留空")
                        .desired_rows(3)
                        .desired_width(350.0),
                );
                ui.label("截图、桌面画笔和语音可调用已安装的 Linux 应用。");
            }
            1 => {
                ui.add(
                    egui::TextEdit::multiline(&mut self.text)
                        .hint_text("按下时输入的文字")
                        .desired_rows(3)
                        .desired_width(350.0),
                );
            }
            2 | 3 => {
                ui.label("应用需支持 Ctrl+滚轮缩放。");
            }
            6 | 7 => {
                ui.label("按住此键移动鼠标。设定此按键时，载入手势页的当前动作。修改方向后，在手势页重新应用。");
            }
            _ => {}
        }
        ui.label(
            RichText::new("主机动作需要本程序保持运行。手势按住绘图，其他动作按下执行。")
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
            ui.label(&self.host_buttons.status);
            if skin::button(
                ui,
                if self.host_buttons.running {
                    "停止主机动作"
                } else {
                    "启用已保存的主机动作"
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
