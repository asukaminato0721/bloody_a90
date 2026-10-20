use super::{App, path_field};
use crate::{
    skin,
    worker::{Job, Purpose},
};
use bloody_a90::{
    gesture_runtime::{Settings, Status},
    gestures::{Action, Document, Kind, Trace16, TraceMouse},
    linux_input::BUTTONS,
};
use eframe::egui::{self, RichText};

pub(super) struct Editor {
    pub(super) documents: [Document; 2],
    pub(super) triggers: [Option<u16>; 2],
    pub(super) running: bool,
    pub(super) last_poll: std::time::Instant,
    status: String,
    kind: usize,
    selected: usize,
    paths: [String; 2],
    originals: [Option<(String, Vec<u8>)>; 2],
    loading_path: Option<String>,
    choice: usize,
    usage: u8,
    modifiers: u8,
    consumer: u16,
    mouse: u8,
    macro_path: String,
    launch_target: String,
    launch_arguments: String,
    input_text: String,
    trace: Trace16,
    trace_mouse: TraceMouse,
    points: Vec<egui::Pos2>,
    last: Option<egui::Pos2>,
    result: String,
}

impl Default for Editor {
    fn default() -> Self {
        Self {
            documents: [Document::new(Kind::Sixteen), Document::new(Kind::Mouse)],
            triggers: [None; 2],
            running: false,
            last_poll: std::time::Instant::now(),
            status: "全局手势未启用".into(),
            kind: 0,
            selected: 0,
            paths: ["16in1.gestures".into(), "Mouse.gestures".into()],
            originals: [None, None],
            loading_path: None,
            choice: 0,
            usage: 4,
            modifiers: 0,
            consumer: 205,
            mouse: 1,
            macro_path: String::new(),
            launch_target: String::new(),
            launch_arguments: String::new(),
            input_text: String::new(),
            trace: Trace16::default(),
            trace_mouse: TraceMouse::default(),
            points: Vec::new(),
            last: None,
            result: String::new(),
        }
    }
}

impl Editor {
    pub(super) fn load_state(&mut self, kind: usize, value: &str) {
        if let Ok(document) = Document::parse(value.as_bytes())
            && document.kind == self.documents[kind].kind
        {
            self.documents[kind] = document;
        }
    }

    pub(super) fn state(&self, kind: usize) -> String {
        String::from_utf8(self.documents[kind].to_bytes()).expect("gesture writer emits UTF-8")
    }

    pub(super) fn imported(&mut self, bytes: &[u8]) -> Result<String, String> {
        let path = self.loading_path.take().ok_or("缺少手势文件路径")?;
        let document = Document::parse(bytes)?;
        self.kind = usize::from(document.kind == Kind::Mouse);
        self.originals[self.kind] = Some((path.clone(), bytes.to_vec()));
        self.paths[self.kind] = path;
        self.documents[self.kind] = document;
        self.selected = 0;
        Ok("已载入手势文件。修改后点击“保存”。".into())
    }
}

impl App {
    pub(super) fn gesture_status(&mut self, result: Result<Status, String>, report: bool) {
        match result {
            Ok(status) => {
                self.gestures.running = status.running;
                self.gestures.status = status.message.clone();
                if report {
                    self.report(Ok(status.message));
                }
            }
            Err(error) => {
                self.gestures.running = false;
                self.gestures.status = error.clone();
                self.report(Err(error));
            }
        }
    }
    pub(super) fn gesture_saved(&mut self, path: String, result: Result<Vec<u8>, String>) {
        let result = result.and_then(|bytes| {
            let doc = Document::parse(&bytes)?;
            let kind = usize::from(doc.kind == Kind::Mouse);
            self.gestures.originals[kind] = Some((path.clone(), bytes));
            Ok(format!("已保存手势文件：{path}"))
        });
        self.report(result);
    }

    pub(super) fn gestures_page(&mut self, ui: &mut egui::Ui) {
        if let Some(frame) = self.smoke_frames {
            self.gestures.kind = frame as usize % 2;
            self.gestures.choice = match frame % 7 {
                0 => 1,
                1 => 9,
                2 => 10,
                3 => 11,
                4 => 12,
                5 => 13,
                _ => 14,
            };
        }
        let mut job = None;
        let mut report = None;
        ui.label(
            RichText::new(
                "普通触发键需在按键页设为对应鼠标按键。顶键或按配置指定的手势，请在按键页选择“主机动作 → 手势”。",
            )
            .color(skin::GRAY),
        );
        ui.add_enabled_ui(!self.pending, |ui| {
            let editor = &mut self.gestures;
            skin::sheet().show(ui, |ui| {
                ui.strong("全局手势");
                for kind in 0..2 {
                    ui.horizontal(|ui| {
                        let mut enabled = editor.triggers[kind].is_some();
                        if ui.checkbox(&mut enabled, editor.documents[kind].kind.name()).changed() {
                            editor.triggers[kind] = if enabled { Some(BUTTONS[3 + kind]) } else { None };
                        }
                        if let Some(code) = &mut editor.triggers[kind] {
                            egui::ComboBox::from_id_salt(("gesture_trigger", kind))
                                .selected_text(["左键", "右键", "中键", "后退键", "前进键"][BUTTONS.iter().position(|button| button == code).unwrap_or(3)])
                                .show_ui(ui, |ui| {
                                    for (button, label) in BUTTONS.into_iter().zip(["左键", "右键", "中键", "后退键", "前进键"]) {
                                        ui.selectable_value(code, button, label);
                                    }
                                });
                        }
                    });
                }
                ui.horizontal(|ui| {
                    if skin::button(ui, "应用全局设置", 160.0, editor.triggers.iter().any(Option::is_some) || self.host_buttons.applied.assigned_gesture_kinds().into_iter().any(|kind| kind)).clicked() {
                        job = Some(Job::Gestures(Some(Box::new(Settings { documents: editor.documents.clone(), triggers: editor.triggers, wheel: None }))));
                    }
                    if skin::button(ui, "停止全局手势", 160.0, editor.running).clicked() { job = Some(Job::Gestures(None)); }
                    if skin::button(ui, "中断宏", 100.0, editor.running).clicked() { job = Some(Job::Interrupt); }
                });
                ui.label(&editor.status);
                ui.label("按键页指定的手势与普通触发键共用此处的动作。停止全局手势会停止两种触发方式。");
                if let Some(saved) = &self.host_buttons.applied.gestures
                    && skin::button(ui, "载入已保存的按键手势", 230.0, true).clicked()
                {
                    editor.documents = saved.documents.clone();
                }
                ui.label("同时按下右键和中键可停止全局手势。普通触发键未移动时保留原点击。");
                ui.label("编辑后需重新应用。手势距离使用鼠标原始位移。关闭本工具会解除输入接管。");
                ui.label("按住重复宏：即时手势中按住触发键时循环，释放识别时执行一遍。开关宏再次匹配同一手势时停止。");
            });
            skin::sheet().show(ui, |ui| {
                ui.horizontal(|ui| {
                    for (index, kind) in [Kind::Sixteen, Kind::Mouse].into_iter().enumerate() {
                        if ui
                            .selectable_value(&mut editor.kind, index, kind.name())
                            .clicked()
                        {
                            editor.selected = 0;
                        }
                    }
                });
                let kind = editor.kind;
                ui.horizontal(|ui| {
                    path_field(ui, &mut editor.paths[kind], "手势文件路径", 440.0);
                    let path = editor.paths[kind].trim().to_owned();
                    if skin::button(ui, "载入", 90.0, !path.is_empty()).clicked() {
                        editor.loading_path = Some(path.clone());
                        job = Some(Job::Read(path.clone(), Purpose::Gestures));
                    }
                    if skin::button(ui, "保存", 90.0, !path.is_empty()).clicked() {
                        let expected = editor.originals[kind]
                            .as_ref()
                            .filter(|(original_path, _)| original_path == &path)
                            .map(|(_, bytes)| bytes.clone());
                        job = Some(Job::SaveGesture(
                            path,
                            editor.documents[kind].to_bytes(),
                            expected,
                        ));
                    }
                });
                ui.label("更换路径可另存为新文件。载入会替换该套手势的编辑内容。");
                let document = &mut editor.documents[kind];
                match document.kind {
                    Kind::Sixteen => {
                        if ui.checkbox(&mut document.realtime, "按距离即时响应").changed()
                            && document.realtime && editor.selected >= 8 { editor.selected = 0; }
                        ui.horizontal(|ui| {
                            ui.label("即时响应距离：");
                            ui.add_enabled(
                                document.realtime,
                                egui::DragValue::new(&mut document.distance).range(1..=u32::MAX),
                            );
                        });
                        if document.realtime { ui.label("即时响应使用前八个单方向。折线动作保持原值。"); }
                    }
                    Kind::Mouse => {
                        ui.checkbox(&mut document.tip, "显示浮动手势提示板");
                    }
                }
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(190.0);
                        for (index, shape) in document.kind.shapes().iter().enumerate() {
                            ui.add_enabled_ui(document.kind != Kind::Sixteen || !document.realtime || index < 8, |ui| {
                                ui.selectable_value(&mut editor.selected, index, shape.name);
                            });
                        }
                    });
                    ui.vertical(|ui| {
                        ui.set_min_width(420.0);
                        let current = &document.actions[editor.selected];
                        ui.strong(format!("当前动作：{}", current.label()));
                        ui.monospace(format!(
                            "Simulator={}  Object={}  Action={}",
                            current.get("Simulator"),
                            current.get("Object"),
                            current.get("Action")
                        ));
                        if !current.get("MacroPath").is_empty() {
                            ui.label(current.get("MacroPath"));
                        }
                        if current.number("Simulator") == Ok(20) { ui.label(current.get("MacroName")); }
                        if !current.get("LinuxArguments").is_empty() {
                            ui.label("程序参数：");
                            ui.monospace(current.get("LinuxArguments").chars().take(1024).collect::<String>());
                        }
                        if current.number("Simulator") == Ok(21) {
                            ui.label(current.get("HintText").chars().take(256).collect::<String>());
                        }
                        egui::ComboBox::from_id_salt("gesture_action")
                            .selected_text(
                                [
                                    "未设置",
                                    "键盘",
                                    "媒体键",
                                    "鼠标按键",
                                    "滚轮向上",
                                    "滚轮向下",
                                    "AMC 宏",
                                    "滚轮向左",
                                    "滚轮向右",
                                    "打开文件或程序",
                                    "输入文字",
                                    "最小化指针下的窗口",
                                    "最大化指针下的窗口",
                                    "应用放大（Ctrl+滚轮）",
                                    "应用缩小（Ctrl+滚轮）",
                                    "全部最小化（KDE Wayland）",
                                ][editor.choice],
                            )
                            .show_ui(ui, |ui| {
                                for (index, label) in [
                                    "未设置",
                                    "键盘",
                                    "媒体键",
                                    "鼠标按键",
                                    "滚轮向上",
                                    "滚轮向下",
                                    "AMC 宏",
                                    "滚轮向左",
                                    "滚轮向右",
                                    "打开文件或程序",
                                    "输入文字",
                                    "最小化指针下的窗口",
                                    "最大化指针下的窗口",
                                    "应用放大（Ctrl+滚轮）",
                                    "应用缩小（Ctrl+滚轮）",
                                    "全部最小化（KDE Wayland）",
                                ]
                                .iter()
                                .enumerate()
                                {
                                    ui.selectable_value(&mut editor.choice, index, *label);
                                }
                            });
                        match editor.choice {
                            1 => {
                                ui.horizontal(|ui| {
                                    egui::ComboBox::from_id_salt("gesture_keyboard_key")
                                        .selected_text(bloody_a90::highlanguage::key_name(editor.usage as u16).unwrap_or("其他按键"))
                                        .show_ui(ui, |ui| {
                                            ui.selectable_value(&mut editor.usage, 0, "仅修饰键");
                                            for (usage, label) in bloody_a90::highlanguage::keys() {
                                                ui.selectable_value(&mut editor.usage, usage as u8, label);
                                            }
                                        });
                                    ui.label("USB HID 键码：");
                                    ui.add(egui::DragValue::new(&mut editor.usage).range(0..=255));
                                });
                                for (start, labels) in [
                                    (0, ["左 Ctrl", "左 Shift", "左 Alt", "左 Super"]),
                                    (4, ["右 Ctrl", "右 Shift", "右 Alt", "右 Super"]),
                                ] {
                                    ui.horizontal(|ui| {
                                        for (bit, label) in labels.into_iter().enumerate() {
                                            let mask = 1 << (start + bit);
                                            let mut value = editor.modifiers & mask != 0;
                                            if ui.checkbox(&mut value, label).changed() {
                                                editor.modifiers = (editor.modifiers & !mask)
                                                    | if value { mask } else { 0 };
                                            }
                                        }
                                    });
                                }
                                ui.label("例如：A=4，V=25，X=27，Enter=40，Esc=41。");
                            }
                            2 => {
                                ui.horizontal(|ui| {
                                    ui.label("Consumer HID 键码：");
                                    ui.add(
                                        egui::DragValue::new(&mut editor.consumer).range(0..=1023),
                                    );
                                });
                                ui.label("播放/暂停=205，静音=226，音量加=233，音量减=234。");
                            }
                            3 => {
                                ui.horizontal(|ui| {
                                    ui.label("鼠标按键：");
                                    ui.add(egui::DragValue::new(&mut editor.mouse).range(1..=5));
                                });
                            }
                            6 => {
                                path_field(ui, &mut editor.macro_path, "AMC 文件路径", 400.0);
                            }
                            9 => {
                                path_field(ui, &mut editor.launch_target, "文件、程序路径或 URL", 400.0);
                                ui.label("可执行文件直接启动。其他文件和 URL 使用默认应用。目标按完整路径处理。");
                                ui.add(egui::TextEdit::multiline(&mut editor.launch_arguments)
                                    .hint_text("程序参数，可留空。每行一个参数。")
                                    .desired_width(400.0).desired_rows(3));
                                ui.label("截图、桌面画笔和语音功能可调用已安装的 Linux 工具。");
                                ui.label("参数中的空格和引号按原文传递。带参数时，目标必须是可执行程序。");
                            }
                            10 => {
                                ui.add(egui::TextEdit::multiline(&mut editor.input_text)
                                    .hint_text("要输入的文字").desired_width(400.0).desired_rows(3));
                                ui.label("支持 KWin EIS，或提供虚拟键盘接口的 Wayland 桌面。换行发送 Enter。");
                            }
                            13 | 14 => {
                                ui.label("应用需支持 Ctrl+滚轮缩放。使用其他绑定的应用可设置键盘动作或 AMC 宏。");
                            }
                            _ => {}
                        }
                        if skin::button(ui, "替换当前方向的动作", 220.0, true).clicked() {
                            let action = match editor.choice {
                                0 => Ok(Action::default()),
                                1 => Ok(Action::keyboard(
                                    editor.usage,
                                    editor.modifiers,
                                    &format!("键盘 {} + {}", editor.modifiers, editor.usage),
                                )),
                                2 => Ok(Action::new(
                                    7,
                                    editor.consumer as u32,
                                    3,
                                    &format!("媒体键 {}", editor.consumer),
                                )),
                                3 => Ok(Action::new(
                                    9,
                                    editor.mouse as u32,
                                    3,
                                    &format!("鼠标按键 {}", editor.mouse),
                                )),
                                4 => Ok(Action::new(9, 9, 3, "滚轮向上")),
                                5 => Ok(Action::new(9, 9, 4, "滚轮向下")),
                                7 => Ok(Action::new(9, 9, 5, "滚轮向左")),
                                8 => Ok(Action::new(9, 9, 6, "滚轮向右")),
                                9 => Action::open_target_with_arguments(&editor.launch_target, &editor.launch_arguments),
                                10 => Action::input_text(&editor.input_text),
                                11 => Ok(Action::new(24, 1, 3, "最小化指针下的窗口")),
                                12 => Ok(Action::new(24, 2, 3, "最大化指针下的窗口")),
                                13 => Ok(Action::new(22, 0, 3, "放大")),
                                14 => Ok(Action::new(22, 1, 3, "缩小")),
                                15 => Ok(Action::new(24, 0, 3, "全部最小化")),
                                _ => Action::macro_file(&editor.macro_path),
                            };
                            match action {
                                Ok(action) => {
                                    document.actions[editor.selected] = action;
                                }
                                Err(error) => report = Some(Err(error)),
                            }
                        }
                        ui.label("仅点击“替换”才改变动作。载入的其他动作保持原值。");
                    });
                });
            });
            {
                skin::sheet().show(ui, |ui| {
                    let kind = editor.documents[editor.kind].kind;
                    ui.strong(format!("{}：释放时识别预览", kind.name()));
                    ui.label("按住左键绘图。松开后显示匹配动作。此区域不执行动作。");
                    let (response, painter) = ui.allocate_painter(
                        egui::vec2(ui.available_width(), 220.0),
                        egui::Sense::drag(),
                    );
                    painter.rect_filled(response.rect, 0.0, skin::FIELD);
                    if response.drag_started() {
                        editor.trace = Trace16::default();
                        editor.trace_mouse = TraceMouse::default();
                        editor.points.clear();
                        editor.last = ui.input(|input| input.pointer.press_origin());
                        if let Some(point) = editor.last {
                            editor.points.push(point);
                        }
                        editor.result.clear();
                    }
                    if response.dragged()
                        && let Some(point) = response.interact_pointer_pos()
                        && let Some(last) = editor.last
                    {
                        let delta = point - last;
                        let step = [delta.x.round() as i32, delta.y.round() as i32];
                        editor.trace.feed(step);
                        editor.trace_mouse.feed(step);
                        editor.last = Some(last + egui::vec2(step[0] as f32, step[1] as f32));
                        if editor.points.len() < 8192 {
                            editor.points.push(point);
                        }
                    }
                    for pair in editor.points.windows(2) {
                        painter.line_segment(
                            [pair[0], pair[1]],
                            egui::Stroke::new(2.0, skin::CAPTION_RED),
                        );
                    }
                    if response.drag_stopped() {
                        let index = if editor.kind == 0 {
                            editor.trace.index()
                        } else {
                            editor.trace_mouse.index()
                        };
                        editor.result = if let Some(index) = index {
                            format!(
                                "{}：{}",
                                kind.shapes()[index].name,
                                editor.documents[editor.kind].actions[index].label()
                            )
                        } else {
                            "没有匹配的手势".into()
                        };
                    }
                    ui.label(format!(
                        "方向序列：{}  {}",
                        if editor.kind == 0 {
                            editor.trace.path()
                        } else {
                            editor.trace_mouse.path()
                        },
                        editor.result
                    ));
                });
            }
        });
        if let Some(job) = job {
            self.start(job);
        }
        if let Some(result) = report {
            self.report(result);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imported_gesture_kind_and_saved_reply_preserve_other_drafts() {
        let mut app = App::from_context(&egui::Context::default());
        app.gestures.documents[0].actions[2] = Action::keyboard(27, 1, "剪切");
        let sixteen = app.gestures.documents[0].clone();
        let mut mouse = Document::new(Kind::Mouse);
        mouse.actions[4] = Action::new(7, 205, 3, "播放/暂停");
        app.gestures.loading_path = Some("/tmp/Mouse.gestures".into());
        app.gestures.imported(&mouse.to_bytes()).unwrap();
        assert_eq!(app.gestures.documents, [sixteen.clone(), mouse.clone()]);
        assert_eq!(app.gestures.kind, 1);
        // A successful save acknowledges the saved bytes, without replacing
        // an editor draft that may have changed before the reply arrives.
        app.gestures.documents[1].actions[0] = Action::keyboard(4, 0, "A");
        let changed = app.gestures.documents[1].clone();
        app.gesture_saved("/tmp/saved.gestures".into(), Ok(mouse.to_bytes()));
        assert_eq!(app.gestures.documents, [sixteen.clone(), changed.clone()]);
        let mut restored = Editor::default();
        for kind in 0..2 {
            let stored = super::super::escape(&app.gestures.state(kind));
            restored.load_state(kind, &super::super::unescape(&stored));
        }
        assert_eq!(restored.documents, [sixteen, changed]);
        let before = restored.documents.clone();
        restored.load_state(0, &String::from_utf8(mouse.to_bytes()).unwrap());
        assert_eq!(restored.documents, before);
    }
}
