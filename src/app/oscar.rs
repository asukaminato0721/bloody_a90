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
use eframe::egui::{self, RichText, vec2};
use std::path::Path;

impl App {
    fn insert(&mut self, text: &str) {
        if !self.press.is_empty() && !self.press.ends_with('\n') {
            self.press.push('\n');
        }
        self.press.push_str(text);
        self.recompile();
    }

    /// MACROEDIT_Instructions: the test copy goes to the fourth key of the
    /// profile running on the mouse.
    fn test_macro(&mut self) {
        let result = (|| {
            let device = self.device.as_ref().ok_or("鼠标未连接")?;
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
        Ok(format!(
            "已导入 AMC：{}{}",
            file.description,
            if file.high_language {
                "（HighLanguage 已转换为 M 文本）"
            } else {
                ""
            }
        ))
    }

    pub(super) fn oscar(&mut self, ui: &mut egui::Ui) {
        let mut changed = false;
        ui.horizontal(|ui| {
            ui.label(RichText::new("文件").color(skin::GRAY));
            path_field(ui, &mut self.export_path, "AMC 或二进制文件路径", 360.0);
            let path = self.export_path.trim().to_owned();
            let enabled = !self.pending && !path.is_empty();
            if skin::tool(ui, "导入 AMC", false).clicked() && enabled {
                self.start(Job::Read(path.clone(), Purpose::Amc));
            }
            if skin::tool(ui, "导出 AMC", false).clicked() && enabled {
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
        });
        ui.add_space(4.0);
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 16.0;
            ui.vertical(|ui| {
                ui.set_width(250.0);
                self.oscar_tools(ui);
            });
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    ui.label(RichText::new("重复方式").color(skin::GRAY));
                    for (mode, title, hint) in [
                        (Repeat::Once, "单次", "不重复执行"),
                        (Repeat::Held, "按住", "按下：执行\n弹起：终止执行"),
                        (Repeat::Toggle, "开关", "按下：执行\n再按：终止执行"),
                    ] {
                        if skin::tool(ui, title, self.repeat == mode)
                            .on_hover_text(hint)
                            .clicked()
                            && self.repeat != mode
                        {
                            self.repeat = mode;
                            changed = true;
                        }
                    }
                });
                changed |= memo(ui, "oscar-press", &mut self.press, 16, true);
                ui.label(
                    RichText::new(if self.repeat == Repeat::Once {
                        "弹起程序"
                    } else {
                        "弹起程序由重复方式决定"
                    })
                    .color(skin::GRAY),
                );
                changed |= memo(
                    ui,
                    "oscar-release",
                    &mut self.release,
                    4,
                    self.repeat == Repeat::Once,
                );
            });
        });
        if changed {
            self.recompile();
        }
        ui.add_space(8.0);
        let mut test = false;
        let mut interrupt = false;
        let export = match &self.compiled {
            Ok(program) => {
                ui.label(format!(
                    "编译通过 · 按下 {} 字 · 弹起 {} 字 · 在“按键”页选择“宏”即可指定给按键",
                    program.press.len(),
                    program.release.len()
                ));
                let mut export = None;
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 10.0;
                    if skin::button(ui, "复制按下字节", 150.0, true).clicked() {
                        ui.ctx().copy_text(byte_hex(&program.press));
                    }
                    if skin::button(ui, "复制弹起字节", 150.0, true).clicked() {
                        ui.ctx().copy_text(byte_hex(&program.release));
                    }
                    let enabled = !self.pending && !self.export_path.trim().is_empty();
                    if skin::button(ui, "导出按下程序", 150.0, enabled)
                        .on_disabled_hover_text("先填写文件路径")
                        .clicked()
                    {
                        export = Some(macros::bytes(&program.press));
                    }
                    if skin::button(ui, "导出弹起程序", 150.0, enabled)
                        .on_disabled_hover_text("先填写文件路径")
                        .clicked()
                    {
                        export = Some(macros::bytes(&program.release));
                    }
                    if skin::button(ui, "测试", 90.0, self.connected() && !self.pending)
                        .on_hover_text(
                            "写入后侧键（第四键）测试。恢复后侧键请到“按键”页重新设定。\n\
                             宏无法停止时，同时按下 Caps Lock、Scroll Lock、Num Lock 中断。",
                        )
                        .clicked()
                    {
                        test = true;
                    }
                    if skin::button(ui, "中断宏", 110.0, self.connected() && !self.pending)
                        .on_hover_text("相当于同时打开 Caps Lock、Num Lock、Scroll Lock")
                        .clicked()
                    {
                        interrupt = true;
                    }
                });
                ui.collapsing("查看指令", |ui| {
                    for (title, words) in [("按下", &program.press), ("弹起", &program.release)]
                    {
                        ui.label(title);
                        for instruction in storage::list(words).instructions {
                            ui.monospace(format!(
                                "{:04}  {:<15}{}",
                                instruction.offset,
                                macros::hex(&instruction.words),
                                instruction.text
                            ));
                        }
                    }
                });
                export
            }
            Err(error) => {
                ui.colored_label(skin::ERROR, error);
                None
            }
        };
        if let Some(bytes) = export {
            self.start(Job::Export(self.export_path.trim().to_owned(), bytes));
        }
        if test {
            self.test_macro();
        }
        if interrupt {
            self.start(Job::Interrupt);
        }
        ui.collapsing("语法示例与限制", |ui| {
            ui.monospace("KeyDownUp 4\ndelay 100\nmover 20 -5\nmoveto 400 300\nvar VarA = 0\nVarA = VarA + 1\nif VarA != 3 goto 5\ngowhile 1 3\nifkey 4 1 9");
            ui.label("跳转使用源代码行号。gowhile X N 把第 X 行至此行重复 N 次；ifkey K S L 在按键 K 按住（S=1）或松开（S=0）时跳到第 L 行。");
            ui.label("HighLanguage 格式的 AMC 文件导入时会转换为上述 M 文本。详细语法见 docs/research/TEXT.md。");
        });
    }

    // Left column of the Oscar editor: program, time, cursor and mouse groups.
    fn oscar_tools(&mut self, ui: &mut egui::Ui) {
        let group = |ui: &mut egui::Ui, title: &str, add: &mut dyn FnMut(&mut egui::Ui)| {
            skin::sheet()
                .inner_margin(egui::Margin::same(10))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new(title).font(skin::bold(15.0)));
                    add(ui);
                });
        };
        let mut insert = None;
        group(ui, "程序", &mut |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("跳跃前往第").color(skin::GRAY));
                ui.add(egui::DragValue::new(&mut self.goto).range(1..=16384));
                ui.label(RichText::new("行").color(skin::GRAY));
                if skin::tool(ui, "插入", false).clicked() {
                    insert = Some(format!("Goto {}", self.goto));
                }
            });
        });
        group(ui, "时间控制", &mut |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("时间").color(skin::GRAY));
                ui.add(
                    egui::DragValue::new(&mut self.delay)
                        .range(0..=999_999)
                        .suffix(" ms"),
                );
                if skin::tool(ui, "插入", false).clicked() {
                    insert = Some(format!("Delay {}", self.delay));
                }
            });
        });
        group(ui, "光标控制", &mut |ui| {
            ui.label(RichText::new("鼠标相对移动坐标(X,Y)").color(skin::GRAY));
            ui.horizontal(|ui| {
                for value in &mut self.motion {
                    ui.add(egui::DragValue::new(value).range(-32768..=32767));
                }
                if skin::tool(ui, "插入", false).clicked() {
                    insert = Some(format!("MoveR {} {}", self.motion[0], self.motion[1]));
                }
            });
        });
        group(ui, "鼠标控制", &mut |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
                for (caption, text) in [
                    ("前滚", "WheelUp"),
                    ("后滚", "WheelDown"),
                    ("左键", "LeftDown\nLeftUp"),
                    ("右键", "RightDown\nRightUp"),
                    ("中键", "MiddleDown\nMiddleUp"),
                    ("四键", "Button4Down\nButton4Up"),
                    ("五键", "Button5Down\nButton5Up"),
                ] {
                    if skin::tool(ui, caption, false).clicked() {
                        insert = Some(text.to_owned());
                    }
                }
            });
        });
        if let Some(text) = insert {
            self.insert(&text);
        }
    }
}
