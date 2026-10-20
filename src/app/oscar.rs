use super::{App, byte_hex, memo, path_field};
use crate::{
    skin,
    worker::{Job, Purpose},
};
use bloody_a90::{
    amc,
    macros::{self, Repeat},
    storage,
};
use bloody_a90::{
    i18n::{tr, tr_text},
    tf,
};
use eframe::egui::{self, RichText, vec2};
use std::path::Path;

impl App {
    /// MACROEDIT_Instructions: the test copy goes to the fourth key of the
    /// profile running on the mouse.
    fn test_macro(&mut self) {
        let result = (|| {
            let device = self.device.as_ref().ok_or(tr("鼠标未连接"))?;
            let program = self.compiled.as_ref().map_err(Clone::clone)?;
            let mut config = device.backup.configuration()?;
            let profile = usize::from(device.profile.0).min(config.profiles.len() - 1);
            config.profiles[profile].buttons[3] = bloody_a90::configuration::Binding {
                press: bloody_a90::configuration::Action::from_program(program.press.clone()),
                release: bloody_a90::configuration::Action::from_program(program.release.clone()),
            };
            Ok::<_, String>(config)
        })();
        match result {
            Ok(config) => self.start(Job::SaveButtons(Box::new(config))),
            Err(error) => self.report(Err(error)),
        }
    }

    pub(super) fn recompile(&mut self) {
        self.compiled = macros::compile(&self.press, &self.release, self.repeat);
    }

    /// Load an AMC file into the editor. HighLanguage text is translated to
    /// M text, so the editor always holds what the compiler encodes.
    pub(super) fn import_amc(&mut self, bytes: &[u8]) -> Result<String, String> {
        let file = amc::parse(bytes)?;
        let text = |source: &str| {
            if file.high_language {
                bloody_a90::highlanguage::translate(source)
            } else {
                Ok(source.replace("\r\n", "\n"))
            }
        };
        let (press, release) = (text(&file.press)?, text(&file.release)?);
        self.press = press;
        self.release = release;
        self.repeat = file.repeat;
        self.recompile();
        Ok(tf!(
            "已导入 AMC：{}{}",
            file.description,
            if file.high_language {
                tr("（HighLanguage 已转换为 M 文本）")
            } else {
                ""
            }
        ))
    }

    /// Replace the press program and keep the old text for 撤销.
    fn edit_press(&mut self, text: String) {
        if text != self.press {
            let old = std::mem::replace(&mut self.press, text);
            self.oscar_view.undo.push(old);
            self.oscar_view.redo.clear();
            self.recompile();
        }
    }

    fn insert_lines(&mut self, text: &str) {
        let mut press = self.press.clone();
        if !press.is_empty() && !press.ends_with('\n') {
            press.push('\n');
        }
        press.push_str(text);
        self.edit_press(press);
    }

    /// One key stroke with the preset delay, as the 仿真区 keyboard inserts it.
    fn insert_key(&mut self, usage: u16) {
        let delay = self.oscar_view.key_delay;
        self.insert_lines(&format!("KeyDown {usage}\nDelay {delay}\nKeyUp {usage}"));
    }

    /// OscarDragMacroEditor: ① 指令区, ② 仿真区 and ③ 编程区, with
    /// 功能注解, 中断宏 and 测试 at the bottom.
    pub(super) fn oscar(&mut self, ui: &mut egui::Ui) {
        let heading = skin::step;
        // In 奥斯卡III the library takes ① and ②, so the editor counts on.
        let numbers = if self.page == super::Page::Library {
            ["③", "④", "⑤"]
        } else {
            ["①", "②", "③"]
        };
        let full = ui.available_width();
        let left = 140.0;
        let right = (full * 0.32).clamp(360.0, 450.0);
        let middle = full - left - right - 32.0;
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 16.0;
            ui.vertical(|ui| {
                ui.set_width(left + 16.0 + middle);
                ui.horizontal(|ui| {
                    ui.set_width(left + 16.0 + middle);
                    heading(ui, numbers[0], tr("指令区"));
                    ui.add_space(left - 90.0);
                    heading(ui, numbers[1], tr("仿真区"));
                    ui.add_space(24.0);
                    ui.label(
                        RichText::new(tr("重复方式"))
                            .font(skin::regular(16.0))
                            .color(skin::GRAY),
                    );
                    for (mode, title, hint) in [
                        (Repeat::Once, tr("单次"), tr("不重复执行")),
                        (Repeat::Held, tr("按住"), tr("按下：执行\n弹起：终止执行")),
                        (Repeat::Toggle, tr("开关"), tr("按下：执行\n再按：终止执行")),
                    ] {
                        if skin::tool(ui, title, self.repeat == mode)
                            .on_hover_text(hint)
                            .clicked()
                            && self.repeat != mode
                        {
                            self.repeat = mode;
                            self.recompile();
                        }
                    }
                });
                ui.add_space(8.0);
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = 16.0;
                    egui::Frame::new()
                        .fill(egui::Color32::from_rgb(29, 30, 32))
                        .inner_margin(egui::Margin::symmetric(22, 30))
                        .show(ui, |ui| {
                            ui.set_width(left - 44.0);
                            ui.set_min_height(430.0);
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = 36.0;
                                for (index, (caption, color)) in CATEGORIES.iter().enumerate() {
                                    if category(
                                        ui,
                                        caption,
                                        *color,
                                        self.oscar_view.category == index,
                                    )
                                    .clicked()
                                    {
                                        self.oscar_view.category = index;
                                    }
                                }
                            });
                        });
                    egui::Frame::new()
                        .fill(egui::Color32::from_rgb(29, 30, 32))
                        .inner_margin(egui::Margin::symmetric(16, 24))
                        .show(ui, |ui| {
                            ui.set_width(middle - 32.0);
                            ui.set_min_height(406.0);
                            ui.vertical(|ui| self.emulation(ui));
                        });
                });
            });
            ui.vertical(|ui| {
                ui.set_width(right);
                heading(ui, numbers[2], tr("编程区"));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(tr("文件"))
                            .font(skin::regular(18.0))
                            .color(skin::GRAY),
                    );
                    path_field(
                        ui,
                        &mut self.export_path,
                        tr("AMC 或二进制文件路径"),
                        right - 60.0,
                    );
                });
                self.program_area(ui, right);
            });
        });
        self.oscar_details(ui);
        let connected = self.connected() && !self.pending;
        let clicked = skin::footer(
            ui,
            self.footer_area,
            &[
                (tr("功能注解"), true),
                (tr("中断宏"), connected),
                (tr("测试"), connected),
            ],
        );
        if clicked[0] {
            self.oscar_view.notes = !self.oscar_view.notes;
        }
        if clicked[1] {
            self.start(Job::Interrupt);
        }
        if clicked[2] {
            self.test_macro();
        }
        if self.oscar_view.notes {
            let mut open = true;
            egui::Window::new(tr("功能注解"))
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .show(ui.ctx(), |ui| {
                    ui.monospace("KeyDown 4\nDelay 100\nKeyUp 4\nMoveR 20 -5\nMoveTo 400 300\nvar VarA = 0\nVarA = VarA + 1\nif VarA != 3 goto 5\ngowhile 1 3\nifkey 4 1 9");
                    ui.label(tr("跳转使用源代码行号。gowhile X N 把第 X 行至此行重复 N 次；\nifkey K S L 在按键 K 按住（S=1）或松开（S=0）时跳到第 L 行。"));
                    ui.label(tr("HighLanguage 格式的 AMC 文件导入时会转换为 M 文本。\n详细语法见 docs/research/TEXT.md。"));
                    ui.label(tr("测试：写入当前配置的后侧键（第四键）。\n宏无法停止时，同时按下 Caps Lock、Scroll Lock、Num Lock 中断。"));
                });
            self.oscar_view.notes = open;
        }
    }

    /// ② 仿真区 of the chosen 指令区 category.
    fn emulation(&mut self, ui: &mut egui::Ui) {
        let mut insert: Option<String> = None;
        match self.oscar_view.category {
            0 => {
                if let Some(usage) = keyboard(ui) {
                    self.insert_key(usage);
                }
                ui.add_space(24.0);
                skin::check(ui, &mut self.oscar_view.physical, tr("实体键盘输入"))
                    .on_hover_text(tr("开启后，在本窗口按下的键盘按键会插入编程区。"));
                ui.horizontal(|ui| {
                    ui.add(egui::DragValue::new(&mut self.oscar_view.key_delay).range(0..=999_999));
                    ui.label(tr("：预设延迟时间(毫秒)"));
                });
                if self.oscar_view.physical {
                    let pressed: Vec<u16> = ui.input(|input| {
                        input
                            .events
                            .iter()
                            .filter_map(|event| match event {
                                egui::Event::Key {
                                    key,
                                    pressed: true,
                                    repeat: false,
                                    ..
                                } => usage_of(*key),
                                _ => None,
                            })
                            .collect()
                    });
                    for usage in pressed {
                        self.insert_key(usage);
                    }
                }
            }
            1 => {
                ui.label(RichText::new(tr("鼠标按键")).color(skin::GRAY));
                egui::Grid::new("oscar-mouse")
                    .spacing(vec2(8.0, 8.0))
                    .show(ui, |ui| {
                        for (caption, name) in [
                            (tr("左键"), "Left"),
                            (tr("右键"), "Right"),
                            (tr("中键"), "Middle"),
                            (tr("四键"), "Button4"),
                            (tr("五键"), "Button5"),
                        ] {
                            ui.label(caption);
                            for (action, text) in [
                                (tr("按下"), format!("{name}Down")),
                                (tr("弹起"), format!("{name}Up")),
                                (
                                    tr("单击"),
                                    format!(
                                        "{name}Down\nDelay {}\n{name}Up",
                                        self.oscar_view.key_delay
                                    ),
                                ),
                            ] {
                                if skin::tool(ui, action, false).clicked() {
                                    insert = Some(text);
                                }
                            }
                            ui.end_row();
                        }
                    });
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(tr("滚轮")).color(skin::GRAY));
                    if skin::tool(ui, tr("前滚"), false).clicked() {
                        insert = Some("WheelUp".into());
                    }
                    if skin::tool(ui, tr("后滚"), false).clicked() {
                        insert = Some("WheelDown".into());
                    }
                });
            }
            2 => {
                ui.label(RichText::new(tr("鼠标相对移动坐标(X,Y)")).color(skin::GRAY));
                ui.horizontal(|ui| {
                    for value in &mut self.motion {
                        ui.add(egui::DragValue::new(value).range(-32768..=32767));
                    }
                    if skin::tool(ui, tr("插入"), false).clicked() {
                        insert = Some(format!("MoveR {} {}", self.motion[0], self.motion[1]));
                    }
                });
                ui.add_space(8.0);
                ui.label(RichText::new(tr("鼠标绝对坐标(X,Y)")).color(skin::GRAY));
                ui.horizontal(|ui| {
                    for value in &mut self.oscar_view.position {
                        ui.add(egui::DragValue::new(value).range(0..=65535));
                    }
                    if skin::tool(ui, tr("插入"), false).clicked() {
                        insert = Some(format!(
                            "MoveTo {} {}",
                            self.oscar_view.position[0], self.oscar_view.position[1]
                        ));
                    }
                });
            }
            3 => {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(tr("延时")).color(skin::GRAY));
                    ui.add(
                        egui::DragValue::new(&mut self.delay)
                            .range(0..=999_999)
                            .suffix(" ms"),
                    );
                    if skin::tool(ui, tr("插入"), false).clicked() {
                        insert = Some(format!("Delay {}", self.delay));
                    }
                });
                ui.horizontal(|ui| {
                    ui.label(RichText::new(tr("跳跃前往第")).color(skin::GRAY));
                    ui.add(egui::DragValue::new(&mut self.goto).range(1..=16384));
                    ui.label(RichText::new(tr("行")).color(skin::GRAY));
                    if skin::tool(ui, tr("插入"), false).clicked() {
                        insert = Some(format!("Goto {}", self.goto));
                    }
                });
                ui.horizontal(|ui| {
                    ui.label(RichText::new(tr("从第")).color(skin::GRAY));
                    ui.add(egui::DragValue::new(&mut self.goto).range(1..=16384));
                    ui.label(RichText::new(tr("行起循环")).color(skin::GRAY));
                    ui.add(egui::DragValue::new(&mut self.oscar_view.count).range(1..=65535));
                    ui.label(RichText::new(tr("次")).color(skin::GRAY));
                    if skin::tool(ui, tr("插入"), false).clicked() {
                        insert = Some(format!("gowhile {} {}", self.goto, self.oscar_view.count));
                    }
                });
                ui.horizontal(|ui| {
                    ui.label(RichText::new(tr("按键")).color(skin::GRAY));
                    ui.add(egui::DragValue::new(&mut self.oscar_view.key).range(1..=255));
                    ui.label(RichText::new(tr("按住时前往第")).color(skin::GRAY));
                    ui.add(egui::DragValue::new(&mut self.goto).range(1..=16384));
                    ui.label(RichText::new(tr("行")).color(skin::GRAY));
                    if skin::tool(ui, tr("插入"), false).clicked() {
                        insert = Some(format!("ifkey {} 1 {}", self.oscar_view.key, self.goto));
                    }
                });
            }
            _ => {
                ui.label(RichText::new(tr("变量 VarA ~ VarZ")).color(skin::GRAY));
                for (caption, text) in [
                    (tr("定义变量 VarA = 0"), "var VarA = 0"),
                    (tr("VarA 加 1"), "VarA = VarA + 1"),
                    (tr("VarA 减 1"), "VarA = VarA - 1"),
                    (tr("VarA 不等于 3 时前往第 1 行"), "if VarA != 3 goto 1"),
                ] {
                    if skin::tool(ui, caption, false).clicked() {
                        insert = Some(text.into());
                    }
                }
            }
        }
        if let Some(text) = insert {
            self.insert_lines(&text);
        }
    }

    /// ③ 编程区: toolbar, press program and release program.
    fn program_area(&mut self, ui: &mut egui::Ui, width: f32) {
        ui.add_space(8.0);
        let (bar, _) = ui.allocate_exact_size(vec2(width, 50.0), egui::Sense::hover());
        ui.painter()
            .rect_filled(bar, 0, egui::Color32::from_rgb(56, 57, 61));
        let path = self.export_path.trim().to_owned();
        let has_path = !self.pending && !path.is_empty();
        let tools: [(&str, &str, bool); 5] = [
            ("💾", tr("导出 AMC 到文件路径"), has_path),
            ("📂", tr("从文件路径导入 AMC"), has_path),
            ("↶", tr("撤销"), !self.oscar_view.undo.is_empty()),
            ("↷", tr("重做"), !self.oscar_view.redo.is_empty()),
            ("🗑", tr("清空编程区"), !self.press.is_empty()),
        ];
        let mut clicked = None;
        for (index, (glyph, hint, enabled)) in tools.iter().enumerate() {
            let rect = egui::Rect::from_center_size(
                bar.left_center() + vec2(32.0 + index as f32 * 64.0, 0.0),
                vec2(44.0, 40.0),
            );
            let response = ui
                .interact(
                    rect,
                    ui.scope_id().with(("oscar-tool", index)),
                    egui::Sense::click(),
                )
                .on_hover_text(*hint);
            let color = if !enabled {
                egui::Color32::from_rgb(90, 90, 92)
            } else if response.hovered() {
                skin::CAPTION_RED
            } else {
                egui::Color32::from_rgb(200, 200, 202)
            };
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                *glyph,
                skin::regular(26.0),
                color,
            );
            if response.clicked() && *enabled {
                clicked = Some(index);
            }
        }
        match clicked {
            Some(0) => {
                let description = Path::new(&path)
                    .file_stem()
                    .map(|stem| stem.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let file = amc::Macro {
                    description,
                    high_language: false,
                    repeat: self.repeat,
                    press: self.press.clone(),
                    release: self.release.clone(),
                };
                self.start(Job::Export(path, file.to_bytes()));
            }
            Some(1) => self.start(Job::Read(path, Purpose::Amc)),
            Some(2) => {
                if let Some(text) = self.oscar_view.undo.pop() {
                    let current = std::mem::replace(&mut self.press, text);
                    self.oscar_view.redo.push(current);
                    self.recompile();
                }
            }
            Some(3) => {
                if let Some(text) = self.oscar_view.redo.pop() {
                    let current = std::mem::replace(&mut self.press, text);
                    self.oscar_view.undo.push(current);
                    self.recompile();
                }
            }
            Some(4) => self.edit_press(String::new()),
            _ => {}
        }
        let before = self.press.clone();
        if memo(ui, "oscar-press", &mut self.press, 15, true) {
            self.oscar_view.undo.push(before);
            self.oscar_view.redo.clear();
            self.recompile();
        }
        ui.label(
            RichText::new(if self.repeat == Repeat::Once {
                tr("弹起程序")
            } else {
                tr("弹起程序由重复方式决定")
            })
            .color(skin::GRAY),
        );
        if memo(
            ui,
            "oscar-release",
            &mut self.release,
            3,
            self.repeat == Repeat::Once,
        ) {
            self.recompile();
        }
        match &self.compiled {
            Ok(program) => {
                ui.label(
                    RichText::new(tf!(
                        "编译通过 · 按下 {} 字 · 弹起 {} 字 · 在“按键”页选择“宏”即可指定给按键",
                        program.press.len(),
                        program.release.len()
                    ))
                    .font(skin::regular(13.0))
                    .color(skin::GRAY),
                );
            }
            Err(error) => {
                ui.colored_label(skin::ERROR, tr_text(error));
            }
        }
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(tr(
                    "⚠ 测试宏无法停止时，同时按下 Caps Lock、Scroll Lock、Num Lock 中断。",
                ))
                .font(skin::regular(13.0))
                .color(skin::LIGHT_GRAY),
            );
        });
    }

    /// Encoded words and raw exports, below the three areas.
    fn oscar_details(&mut self, ui: &mut egui::Ui) {
        let Ok(program) = &self.compiled else {
            return;
        };
        let mut export = None;
        ui.collapsing(tr("编码与导出"), |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                if skin::button(ui, tr("复制按下字节"), 150.0, true).clicked() {
                    ui.ctx().copy_text(byte_hex(&program.press));
                }
                if skin::button(ui, tr("复制弹起字节"), 150.0, true).clicked() {
                    ui.ctx().copy_text(byte_hex(&program.release));
                }
                let enabled = !self.pending && !self.export_path.trim().is_empty();
                if skin::button(ui, tr("导出按下程序"), 150.0, enabled)
                    .on_disabled_hover_text(tr("先填写文件路径"))
                    .clicked()
                {
                    export = Some(macros::bytes(&program.press));
                }
                if skin::button(ui, tr("导出弹起程序"), 150.0, enabled)
                    .on_disabled_hover_text(tr("先填写文件路径"))
                    .clicked()
                {
                    export = Some(macros::bytes(&program.release));
                }
            });
            for (title, words) in [(tr("按下"), &program.press), (tr("弹起"), &program.release)]
            {
                ui.label(title);
                for instruction in storage::list(words).instructions {
                    ui.monospace(format!(
                        "{:04}  {:<15}{}",
                        instruction.offset,
                        macros::hex(&instruction.words),
                        tr_text(&instruction.text)
                    ));
                }
            }
        });
        if let Some(bytes) = export {
            self.start(Job::Export(self.export_path.trim().to_owned(), bytes));
        }
    }
}

/// State of the Oscar editor that is not saved between runs.
pub(super) struct View {
    category: usize,
    physical: bool,
    key_delay: u32,
    position: [u32; 2],
    count: u32,
    key: u32,
    notes: bool,
    undo: Vec<String>,
    redo: Vec<String>,
}

impl Default for View {
    fn default() -> Self {
        Self {
            category: 0,
            physical: false,
            key_delay: 64,
            position: [400, 300],
            count: 3,
            key: 4,
            notes: false,
            undo: Vec::new(),
            redo: Vec::new(),
        }
    }
}

/// 指令区 categories and their color marks.
const CATEGORIES: [(&str, egui::Color32); 5] = [
    ("键盘", egui::Color32::from_rgb(28, 160, 110)),
    ("鼠标", egui::Color32::from_rgb(150, 40, 200)),
    ("动作", egui::Color32::from_rgb(60, 70, 220)),
    ("控制", egui::Color32::from_rgb(240, 140, 20)),
    ("运算", egui::Color32::from_rgb(40, 180, 60)),
];

fn category(
    ui: &mut egui::Ui,
    caption: &str,
    color: egui::Color32,
    selected: bool,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(98.0, 24.0), egui::Sense::click());
    let painter = ui.painter();
    let fill = if selected {
        color
    } else if response.hovered() {
        egui::Color32::from_rgb(78, 79, 84)
    } else {
        egui::Color32::from_rgb(60, 61, 66)
    };
    painter.rect_filled(rect, 3, fill);
    painter.rect_filled(
        egui::Rect::from_min_size(rect.min, vec2(5.0, 24.0)),
        1,
        color,
    );
    painter.text(
        rect.left_center() + vec2(12.0, 0.0),
        egui::Align2::LEFT_CENTER,
        tr_text(caption),
        skin::regular(16.0),
        skin::TEXT,
    );
    response
}

// On-screen keyboard rows: caption, HID usage and width in key units.
type Row = &'static [(&'static str, u16, f32)];
const MAIN: [Row; 6] = [
    &[
        ("ESC", 41, 1.0),
        ("", 0, 1.0),
        ("F1", 58, 1.0),
        ("F2", 59, 1.0),
        ("F3", 60, 1.0),
        ("F4", 61, 1.0),
        ("", 0, 0.3),
        ("F5", 62, 1.0),
        ("F6", 63, 1.0),
        ("F7", 64, 1.0),
        ("F8", 65, 1.0),
        ("", 0, 0.3),
        ("F9", 66, 1.0),
        ("F10", 67, 1.0),
        ("F11", 68, 1.0),
        ("F12", 69, 1.0),
    ],
    &[],
    &[
        ("~ `", 53, 1.0),
        ("1", 30, 1.0),
        ("2", 31, 1.0),
        ("3", 32, 1.0),
        ("4", 33, 1.0),
        ("5", 34, 1.0),
        ("6", 35, 1.0),
        ("7", 36, 1.0),
        ("8", 37, 1.0),
        ("9", 38, 1.0),
        ("0", 39, 1.0),
        ("_ -", 45, 1.0),
        ("+ =", 46, 1.0),
        ("back", 42, 1.75),
    ],
    &[
        ("Tab", 43, 1.3),
        ("Q", 20, 1.0),
        ("W", 26, 1.0),
        ("E", 8, 1.0),
        ("R", 21, 1.0),
        ("T", 23, 1.0),
        ("Y", 28, 1.0),
        ("U", 24, 1.0),
        ("I", 12, 1.0),
        ("O", 18, 1.0),
        ("P", 19, 1.0),
        ("{ [", 47, 1.0),
        ("} ]", 48, 1.0),
        ("| \\", 49, 1.45),
    ],
    &[
        ("Caps", 57, 1.6),
        ("A", 4, 1.0),
        ("S", 22, 1.0),
        ("D", 7, 1.0),
        ("F", 9, 1.0),
        ("G", 10, 1.0),
        ("H", 11, 1.0),
        ("J", 13, 1.0),
        ("K", 14, 1.0),
        ("L", 15, 1.0),
        (": ;", 51, 1.0),
        ("\" '", 52, 1.0),
        ("Enter", 40, 2.15),
    ],
    &[
        ("Shift", 225, 2.0),
        ("Z", 29, 1.0),
        ("X", 27, 1.0),
        ("C", 6, 1.0),
        ("V", 25, 1.0),
        ("B", 5, 1.0),
        ("N", 17, 1.0),
        ("M", 16, 1.0),
        ("< ,", 54, 1.0),
        ("> .", 55, 1.0),
        ("? /", 56, 1.0),
        ("RShift", 229, 2.75),
    ],
];
const BOTTOM: Row = &[
    ("Ctrl", 224, 1.2),
    ("LW", 227, 1.1),
    ("Alt", 226, 1.1),
    ("Space", 44, 6.4),
    ("RAlt", 230, 1.1),
    ("RW", 231, 1.1),
    ("MENU", 101, 1.2),
    ("RCtrl", 228, 1.2),
];
const EDIT: [&[(&str, u16)]; 5] = [
    &[("Print", 70), ("Scroll", 71), ("Pause", 72)],
    &[("Insert", 73), ("Home", 74), ("Up", 75)],
    &[("Delete", 76), ("End", 77), ("Down", 78)],
    &[("", 0), ("^", 82), ("", 0)],
    &[("<", 80), ("v", 81), (">", 79)],
];
const PAD: [&[(&str, u16)]; 5] = [
    &[("LOCK", 83), ("/", 84), ("*", 85), ("-", 86)],
    &[("7", 95), ("8", 96), ("9", 97), ("+", 87)],
    &[("4", 92), ("5", 93), ("6", 94), ("", 0)],
    &[("1", 89), ("2", 90), ("3", 91), ("Enter", 88)],
    &[("0", 98), ("", 0), ("Del", 99), ("", 0)],
];

fn key_cap(ui: &mut egui::Ui, rect: egui::Rect, caption: &str, id: u16) -> bool {
    let response = ui.interact(
        rect,
        ui.scope_id().with(("oscar-key", id)),
        egui::Sense::click(),
    );
    let painter = ui.painter();
    let fill = if response.is_pointer_button_down_on() {
        skin::RED
    } else if response.hovered() {
        egui::Color32::from_rgb(70, 71, 76)
    } else {
        egui::Color32::from_rgb(20, 20, 22)
    };
    painter.rect_filled(rect, 2, fill);
    painter.rect_stroke(
        rect,
        2,
        egui::Stroke::new(1.0, egui::Color32::from_rgb(170, 170, 172)),
        egui::StrokeKind::Inside,
    );
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        tr_text(caption),
        skin::regular(12.0),
        skin::TEXT,
    );
    response.clicked()
}

/// The 仿真区 keyboard. Returns the HID usage of a clicked key.
fn keyboard(ui: &mut egui::Ui) -> Option<u16> {
    let unit = 30.0;
    let height = 26.0;
    let (rect, _) =
        ui.allocate_exact_size(vec2(unit * 24.0, height * 6.0 + 6.0), egui::Sense::hover());
    let mut chosen = None;
    let y =
        |row: usize| rect.top() + row as f32 * (height + 4.0) - if row > 1 { 26.0 } else { 0.0 };
    for (row, keys) in MAIN.iter().enumerate() {
        let mut x = rect.left();
        for (caption, usage, width) in keys.iter() {
            let w = unit * width;
            if *usage != 0 {
                let r = egui::Rect::from_min_size(egui::pos2(x, y(row)), vec2(w - 2.0, height));
                if key_cap(ui, r, caption, *usage) {
                    chosen = Some(*usage);
                }
            }
            x += w;
        }
    }
    let mut x = rect.left();
    for (caption, usage, width) in BOTTOM {
        let w = unit * width;
        let r = egui::Rect::from_min_size(egui::pos2(x, y(6)), vec2(w - 2.0, height));
        if key_cap(ui, r, caption, *usage) {
            chosen = Some(*usage);
        }
        x += w;
    }
    let edit_left = rect.left() + unit * 15.2;
    for (row, keys) in EDIT.iter().enumerate() {
        for (column, (caption, usage)) in keys.iter().enumerate() {
            if *usage == 0 {
                continue;
            }
            let r = egui::Rect::from_min_size(
                egui::pos2(edit_left + column as f32 * (unit + 10.0), y(row + 2)),
                vec2(unit + 8.0, height),
            );
            if key_cap(ui, r, caption, *usage) {
                chosen = Some(*usage);
            }
        }
    }
    let pad_left = edit_left + 3.0 * (unit + 10.0) + 12.0;
    for (row, keys) in PAD.iter().enumerate() {
        for (column, (caption, usage)) in keys.iter().enumerate() {
            if *usage == 0 {
                continue;
            }
            let tall = matches!(*usage, 87 | 88);
            let wide = *usage == 98;
            let r = egui::Rect::from_min_size(
                egui::pos2(pad_left + column as f32 * unit, y(row + 2)),
                vec2(
                    if wide { unit * 2.0 - 2.0 } else { unit - 2.0 },
                    if tall { height * 2.0 + 4.0 } else { height },
                ),
            );
            if key_cap(ui, r, caption, *usage) {
                chosen = Some(*usage);
            }
        }
    }
    chosen
}

/// HID usage of a key typed on the physical keyboard.
fn usage_of(key: egui::Key) -> Option<u16> {
    use egui::Key;
    let letters = [
        Key::A,
        Key::B,
        Key::C,
        Key::D,
        Key::E,
        Key::F,
        Key::G,
        Key::H,
        Key::I,
        Key::J,
        Key::K,
        Key::L,
        Key::M,
        Key::N,
        Key::O,
        Key::P,
        Key::Q,
        Key::R,
        Key::S,
        Key::T,
        Key::U,
        Key::V,
        Key::W,
        Key::X,
        Key::Y,
        Key::Z,
    ];
    if let Some(index) = letters.iter().position(|k| *k == key) {
        return Some(4 + index as u16);
    }
    let digits = [
        Key::Num1,
        Key::Num2,
        Key::Num3,
        Key::Num4,
        Key::Num5,
        Key::Num6,
        Key::Num7,
        Key::Num8,
        Key::Num9,
        Key::Num0,
    ];
    if let Some(index) = digits.iter().position(|k| *k == key) {
        return Some(30 + index as u16);
    }
    let functions = [
        Key::F1,
        Key::F2,
        Key::F3,
        Key::F4,
        Key::F5,
        Key::F6,
        Key::F7,
        Key::F8,
        Key::F9,
        Key::F10,
        Key::F11,
        Key::F12,
    ];
    if let Some(index) = functions.iter().position(|k| *k == key) {
        return Some(58 + index as u16);
    }
    Some(match key {
        Key::Enter => 40,
        Key::Escape => 41,
        Key::Backspace => 42,
        Key::Tab => 43,
        Key::Space => 44,
        Key::Minus => 45,
        Key::Equals => 46,
        Key::OpenBracket => 47,
        Key::CloseBracket => 48,
        Key::Backslash => 49,
        Key::Semicolon => 51,
        Key::Quote => 52,
        Key::Backtick => 53,
        Key::Comma => 54,
        Key::Period => 55,
        Key::Slash => 56,
        Key::Insert => 73,
        Key::Home => 74,
        Key::PageUp => 75,
        Key::Delete => 76,
        Key::End => 77,
        Key::PageDown => 78,
        Key::ArrowRight => 79,
        Key::ArrowLeft => 80,
        Key::ArrowDown => 81,
        Key::ArrowUp => 82,
        _ => return None,
    })
}
