use super::App;
use crate::{skin, worker::Job};
use bloody_a90::{
    button_actions,
    configuration::{Action, BUTTONS, Binding},
    highlanguage,
    storage::{self, NO_OPERATION},
};
use eframe::egui::{self, RichText, vec2};

/// Action types of the KeySet window.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    Mouse,
    Keyboard,
    Media,
    System,
    Shortcut,
    Function,
    Shoot,
    Macro,
    Host,
    Disabled,
}

const CHOICES: [(Choice, &str); 10] = [
    (Choice::Mouse, "鼠标按键"),
    (Choice::Keyboard, "键盘按键"),
    (Choice::Media, "多媒体"),
    (Choice::System, "系统按键"),
    (Choice::Shortcut, "快捷操作"),
    (Choice::Function, "功能"),
    (Choice::Shoot, "射击模式"),
    (Choice::Macro, "宏"),
    (Choice::Host, "主机动作"),
    (Choice::Disabled, "禁用"),
];

const MOUSE: [(&str, u16); 5] = [
    ("左键", 1),
    ("右键", 2),
    ("中键", 4),
    ("后退键（四键）", 8),
    ("前进键（五键）", 16),
];

// USB consumer usages.
const MEDIA: [(&str, u16); 19] = [
    ("播放/暂停", 0xcd),
    ("停止", 0xb7),
    ("上一曲", 0xb6),
    ("下一曲", 0xb5),
    ("静音", 0xe2),
    ("音量加", 0xe9),
    ("音量减", 0xea),
    ("浏览器后退", 0x224),
    ("浏览器前进", 0x225),
    ("停止加载", 0x226),
    ("刷新", 0x227),
    ("搜索", 0x221),
    ("收藏夹", 0x22a),
    ("浏览器主页", 0x223),
    ("打开浏览器", 0x196),
    ("电子邮件", 0x18a),
    ("媒体播放器", 0x193),
    ("计算器", 0x192),
    ("文件管理器", 0x194),
];

const SYSTEM: [(&str, u16); 3] = [("电源", 0x81), ("睡眠", 0x82), ("唤醒", 0x83)];

#[derive(Clone, Copy)]
enum Shortcut {
    Keyboard(u16, u8),
    Clicks(usize),
}

const SHORTCUTS: [(&str, Shortcut); 14] = [
    ("剪切 Ctrl+X", Shortcut::Keyboard(27, 1)),
    ("复制 Ctrl+C", Shortcut::Keyboard(6, 1)),
    ("粘贴 Ctrl+V", Shortcut::Keyboard(25, 1)),
    ("删除 Delete", Shortcut::Keyboard(76, 0)),
    ("查找 Ctrl+F", Shortcut::Keyboard(9, 1)),
    ("全选 Ctrl+A", Shortcut::Keyboard(4, 1)),
    ("重做 Ctrl+Y", Shortcut::Keyboard(28, 1)),
    ("撤销 Ctrl+Z", Shortcut::Keyboard(29, 1)),
    ("新建 Ctrl+N", Shortcut::Keyboard(17, 1)),
    ("打开 Ctrl+O", Shortcut::Keyboard(18, 1)),
    ("保存 Ctrl+S", Shortcut::Keyboard(22, 1)),
    ("打印 Ctrl+P", Shortcut::Keyboard(19, 1)),
    ("左键双击", Shortcut::Clicks(2)),
    ("左键三击", Shortcut::Clicks(3)),
];

// Press and release words, as in the factory table.
const FUNCTIONS: [(&str, u16, u16); 17] = [
    ("滚轮向上", 0x262b, NO_OPERATION),
    ("滚轮向下", 0x262c, NO_OPERATION),
    ("DPI 档位上移", 0x2626, NO_OPERATION),
    ("DPI 档位下移", 0x2624, NO_OPERATION),
    ("DPI 增加", 0x2627, NO_OPERATION),
    ("DPI 减少", 0x2625, NO_OPERATION),
    ("精确模式（按住生效）", 0x2628, 0x2629),
    ("配置上移", 0x2622, NO_OPERATION),
    ("配置下移", 0x2623, NO_OPERATION),
    ("配置循环", 0x262a, NO_OPERATION),
    ("灯光上移", 0x2618, NO_OPERATION),
    ("灯光下移", 0x2619, NO_OPERATION),
    ("灯光循环", 0x261a, NO_OPERATION),
    ("回报率循环", 0x260e, NO_OPERATION),
    ("抬起高度切换", 0x260f, NO_OPERATION),
    ("切换 Y 轴", 0x260b, NO_OPERATION),
    ("锁定右键", 0x260d, NO_OPERATION),
];

fn describe(action: &Action) -> String {
    match action {
        Action::Direct(word) => storage::describe(*word),
        Action::Macro(words) => format!("宏（{} 字）", words.len()),
    }
}

/// Record N belongs to button N of the firmware state bits. The A90 names
/// were found with the button query on the mouse.
fn record_name(index: usize) -> String {
    match index {
        0 => "左键".into(),
        1 => "右键".into(),
        2 => "中键".into(),
        3 => "后侧键".into(),
        4 => "前侧键".into(),
        8 => "“1”键".into(),
        9 => "“C”键".into(),
        10 => "“3”键".into(),
        24 => "滚轮向下".into(),
        25 => "滚轮向上".into(),
        _ => format!("记录 {}", index + 1),
    }
}

/// Records with a physical button on the A90.
const PHYSICAL: [usize; 8] = [0, 1, 2, 3, 4, 8, 9, 10];

impl App {
    fn key_functions(&self) -> impl Iterator<Item = &'static (&'static str, u16, u16)> + '_ {
        FUNCTIONS
            .iter()
            .filter(|(_, _, release)| self.record < BUTTONS || *release == NO_OPERATION)
    }

    pub(super) fn host_key_selected(&mut self) {
        if let Some(action) = self
            .host_buttons
            .rules
            .profiles
            .get(self.profile)
            .and_then(|slots| slots[self.record].clone())
        {
            self.choice = Choice::Host;
            self.host_buttons.select(&action);
        } else {
            self.select_hardware();
        }
    }

    fn select_hardware(&mut self) {
        let Some(profile) = self
            .keys
            .as_ref()
            .and_then(|config| config.profiles.get(self.profile))
        else {
            return;
        };
        let wheel = self.record >= BUTTONS;
        let actual = if wheel {
            Binding {
                press: profile.wheel[self.record - BUTTONS].clone(),
                release: Action::Direct(NO_OPERATION),
            }
        } else {
            profile.buttons[self.record].clone()
        };
        let keyboard = if wheel {
            button_actions::wheel_keyboard_value(&actual.press)
        } else {
            button_actions::keyboard_value(&actual)
        };
        if let Some((usage, modifiers)) = keyboard
            && let Some(index) = highlanguage::keys()
                .position(|(key, _)| key == usage)
                .or_else(|| (usage == 0).then(|| highlanguage::keys().count()))
        {
            self.choice = Choice::Keyboard;
            self.choice_value = index;
            self.key_modifiers = modifiers;
            return;
        }
        self.key_modifiers = 0;
        for choice in [
            Choice::Mouse,
            Choice::Media,
            Choice::System,
            Choice::Shortcut,
            Choice::Function,
            Choice::Shoot,
            Choice::Disabled,
        ] {
            self.choice = choice;
            for index in 0..self.options().len().max(1) {
                self.choice_value = index;
                let matches = if wheel {
                    self.wheel_action().is_ok_and(|value| value == actual.press)
                } else {
                    self.binding().is_ok_and(|value| value == actual)
                };
                if matches {
                    return;
                }
            }
        }
        self.choice = Choice::Macro;
        self.choice_value = 0;
    }

    fn options(&self) -> Vec<String> {
        match self.choice {
            Choice::Mouse => MOUSE.iter().map(|(name, _)| name.to_string()).collect(),
            Choice::Keyboard => highlanguage::keys()
                .map(|(_, name)| name.to_owned())
                .chain(std::iter::once("无主键（仅修饰键）".into()))
                .collect(),
            Choice::Media => MEDIA.iter().map(|(name, _)| name.to_string()).collect(),
            Choice::System => SYSTEM.iter().map(|(name, _)| name.to_string()).collect(),
            Choice::Shortcut => SHORTCUTS.iter().map(|(name, _)| name.to_string()).collect(),
            Choice::Function => self
                .key_functions()
                .map(|(name, ..)| name.to_string())
                .collect(),
            Choice::Shoot => (1..=8)
                .map(|index| format!("shoot index {index}"))
                .collect(),
            Choice::Macro | Choice::Host | Choice::Disabled => Vec::new(),
        }
    }

    fn binding(&self) -> Result<Binding, String> {
        let direct = |press, release| Binding {
            press: Action::Direct(press),
            release: Action::Direct(release),
        };
        let value = self.choice_value;
        Ok(match self.choice {
            Choice::Mouse => {
                let mask = MOUSE[value].1;
                direct(0x2500 | mask, 0x2400 | mask)
            }
            Choice::Keyboard => {
                let usage = if value == highlanguage::keys().count() {
                    0
                } else {
                    highlanguage::keys().nth(value).ok_or("无效按键")?.0
                };
                button_actions::keyboard(usage, self.key_modifiers)?
            }
            Choice::Media => {
                let usage = MEDIA[value].1;
                direct(0x3400 | usage, 0x3000 | usage)
            }
            Choice::System => {
                let usage = SYSTEM[value].1;
                direct(0x3900 | usage, 0x3800 | usage)
            }
            Choice::Shortcut => match SHORTCUTS[value].1 {
                Shortcut::Keyboard(usage, modifiers) => button_actions::keyboard(usage, modifiers)?,
                Shortcut::Clicks(count) => button_actions::clicks(count)?,
            },
            Choice::Function => {
                let (_, press, release) = self.key_functions().nth(value).ok_or("无效功能")?;
                Binding {
                    press: Action::from_program(vec![*press, storage::END]),
                    release: Action::from_program(vec![*release, storage::END]),
                }
            }
            Choice::Shoot => direct(0x2610 + value as u16, NO_OPERATION),
            Choice::Macro => {
                let program = self
                    .compiled
                    .as_ref()
                    .map_err(|e| format!("奥斯卡宏编译失败：{e}"))?;
                Binding {
                    press: Action::from_program(program.press.clone()),
                    release: Action::from_program(program.release.clone()),
                }
            }
            Choice::Host | Choice::Disabled => direct(NO_OPERATION, NO_OPERATION),
        })
    }

    fn wheel_action(&self) -> Result<Action, String> {
        if self.choice == Choice::Macro {
            return bloody_a90::macros::compile_wheel(&self.press, &self.release)
                .map(Action::from_program);
        }
        let keyboard_delay = self.choice == Choice::Keyboard
            || (self.choice == Choice::Shortcut
                && matches!(SHORTCUTS[self.choice_value].1, Shortcut::Keyboard(..)));
        button_actions::wheel(self.binding()?, keyboard_delay)
    }

    pub(super) fn keys_page(&mut self, ui: &mut egui::Ui) {
        let selected = (self.profile, self.record);
        if self.watching
            && let Some(button) = self.held.first()
        {
            self.record = (*button as usize - 1).min(BUTTONS - 1);
        }
        let count = self.keys.as_ref().map_or(0, |config| config.profiles.len());
        ui.label(RichText::new("选择配置 :").font(skin::regular(16.0)));
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            for index in 0..count.max(5) {
                if skin::profile_tab(
                    ui,
                    &format!("模式{}", index + 1),
                    self.profile == index,
                    index < count,
                )
                .clicked()
                {
                    self.profile = index;
                }
            }
        });
        if selected != (self.profile, self.record) {
            self.host_key_selected();
        }
        let Some(config) = &self.keys else {
            skin::sheet().show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new("按键设置").font(skin::bold(17.0)));
                ui.label(
                    RichText::new("请连接鼠标，或在 其他设置 → 已有备份 中读取文件。")
                        .color(skin::GRAY),
                );
            });
            return;
        };
        let profile = &config.profiles[self.profile.min(count - 1)];
        let rows: Vec<[String; 2]> = profile
            .buttons
            .iter()
            .enumerate()
            .map(|(slot, binding)| {
                if let Some(action) = self
                    .host_buttons
                    .rules
                    .profiles
                    .get(self.profile)
                    .and_then(|slots| slots[slot].as_ref())
                    .filter(|_| *binding == Binding::default())
                {
                    [format!("主机：{}", action.label()), "空操作".into()]
                } else {
                    [describe(&binding.press), describe(&binding.release)]
                }
            })
            .collect();
        let wheel: [String; 2] = std::array::from_fn(|index| {
            self.host_buttons
                .rules
                .profiles
                .get(self.profile)
                .and_then(|slots| slots[24 + index].as_ref())
                .filter(|_| profile.wheel[index] == Action::Direct(NO_OPERATION))
                .map_or_else(
                    || describe(&profile.wheel[index]),
                    |action| format!("主机：{}", action.label()),
                )
        });
        let mut select = None;
        let mut select_wheel = None;
        skin::sheet().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 24.0;
                ui.vertical(|ui| {
                    ui.set_width(520.0);
                    ui.label(RichText::new("按键设置").font(skin::bold(17.0)));
                    let mut row = |ui: &mut egui::Ui, index: usize| {
                        let held = self.held.contains(&(index as u8 + 1));
                        let name = if held {
                            format!("● {}", record_name(index))
                        } else {
                            record_name(index)
                        };
                        if ui.selectable_label(self.record == index, name).clicked() {
                            select = Some(index);
                        }
                        ui.label(&rows[index][0]);
                        ui.label(&rows[index][1]);
                        ui.end_row();
                    };
                    egui::Grid::new("key-records")
                        .striped(true)
                        .min_col_width(80.0)
                        .show(ui, |ui| {
                            for title in ["按键", "按下", "弹起"] {
                                ui.label(RichText::new(title).color(skin::GRAY));
                            }
                            ui.end_row();
                            for index in PHYSICAL {
                                row(ui, index);
                            }
                            for (index, label) in wheel.iter().enumerate() {
                                if ui
                                    .selectable_label(
                                        self.record == 24 + index,
                                        record_name(24 + index),
                                    )
                                    .clicked()
                                {
                                    select_wheel = Some(24 + index);
                                }
                                ui.label(label);
                                ui.label("—");
                                ui.end_row();
                            }
                        });
                    ui.collapsing("其他记录（A90 没有对应的物理按键）", |ui| {
                        egui::Grid::new("other-records")
                            .striped(true)
                            .min_col_width(80.0)
                            .show(ui, |ui| {
                                for index in (0..BUTTONS).filter(|i| !PHYSICAL.contains(i)) {
                                    row(ui, index);
                                }
                            });
                    });
                });
                ui.vertical(|ui| self.key_editor(ui));
            });
        });
        if let Some(index) = select.or(select_wheel) {
            self.record = index;
            self.host_key_selected();
        }
        ui.add_space(14.0);
        let connected = self.connected() && !self.pending;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let apply = skin::button(ui, "应用", 140.0, connected && self.keys_dirty)
                    .on_disabled_hover_text("需要连接鼠标，并先修改按键");
                if apply.clicked()
                    && let Some(config) = &self.keys
                {
                    self.start(Job::SaveHostButtons(
                        Box::new(config.clone()),
                        Box::new(self.host_buttons.rules.clone()),
                        self.host_buttons.expected_file.clone(),
                    ));
                }
                if skin::button(ui, "取消", 140.0, self.keys_dirty).clicked() {
                    self.host_buttons.rules = self.host_buttons.applied.clone();
                    let backup = self
                        .device
                        .as_ref()
                        .map(|device| device.backup.clone())
                        .or_else(|| self.source.clone());
                    if let Some(backup) = backup {
                        self.load_source(backup);
                    }
                }
                let factory = self
                    .source
                    .as_ref()
                    .map(|backup| backup.factory_configuration());
                if skin::button(ui, "恢复默认", 140.0, factory.is_some())
                    .on_hover_text("载入出厂按键表，点击“应用”后写入")
                    .clicked()
                {
                    match factory {
                        Some(Ok(config)) => {
                            self.host_buttons.rules = bloody_a90::host_buttons::Settings::default();
                            self.host_buttons.rules.resize(config.profiles.len());
                            self.keys = Some(config);
                            self.keys_dirty = true;
                        }
                        Some(Err(error)) => self.report(Err(error)),
                        None => {}
                    }
                }
            });
        });
        self.host_button_controls(ui);
    }

    fn key_editor(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new(record_name(self.record)).font(skin::bold(17.0)));
        if let Some(binding) = self
            .keys
            .as_ref()
            .and_then(|config| config.profiles.get(self.profile))
            .and_then(|profile| profile.buttons.get(self.record))
        {
            ui.label(
                RichText::new(format!(
                    "当前：{} / {}",
                    describe(&binding.press),
                    describe(&binding.release)
                ))
                .color(skin::GRAY),
            );
        }
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
            for (choice, caption) in CHOICES {
                if skin::tool(ui, caption, self.choice == choice).clicked() && self.choice != choice
                {
                    self.choice = choice;
                    self.choice_value = 0;
                }
            }
        });
        let options = self.options();
        if !options.is_empty() {
            self.choice_value = self.choice_value.min(options.len() - 1);
            egui::ComboBox::from_id_salt("key-choice")
                .selected_text(&options[self.choice_value])
                .width(260.0)
                .show_ui(ui, |ui| {
                    for (index, option) in options.iter().enumerate() {
                        ui.selectable_value(&mut self.choice_value, index, option);
                    }
                });
        }
        if self.choice == Choice::Keyboard {
            for (start, labels) in [
                (0, ["左 Ctrl", "左 Shift", "左 Alt", "左 Super"]),
                (4, ["右 Ctrl", "右 Shift", "右 Alt", "右 Super"]),
            ] {
                ui.horizontal_wrapped(|ui| {
                    for (index, label) in labels.into_iter().enumerate() {
                        let mask = 1 << (start + index);
                        let mut enabled = self.key_modifiers & mask != 0;
                        if ui.checkbox(&mut enabled, label).changed() {
                            self.key_modifiers =
                                (self.key_modifiers & !mask) | if enabled { mask } else { 0 };
                        }
                    }
                });
            }
        }
        if matches!(self.choice, Choice::Media | Choice::System) {
            ui.label("动作由桌面处理。实际效果取决于桌面快捷键和默认应用设置。");
        }
        if self.choice == Choice::Macro {
            ui.label(
                RichText::new(if self.record >= BUTTONS {
                    match self.wheel_action() {
                        Ok(Action::Macro(words)) => {
                            format!("使用奥斯卡II中的单次滚轮程序：{} 字", words.len())
                        }
                        Ok(_) => "使用奥斯卡II中的直接滚轮动作".into(),
                        Err(error) => format!("滚轮程序无法编译：{error}"),
                    }
                } else {
                    match &self.compiled {
                        Ok(program) => format!(
                            "使用奥斯卡II中的程序：按下 {} 字，弹起 {} 字",
                            program.press.len(),
                            program.release.len()
                        ),
                        Err(error) => format!("奥斯卡II中的程序无法编译：{error}"),
                    }
                })
                .color(skin::GRAY),
            );
            if self.record >= BUTTONS {
                ui.label("每格滚动执行一次按下程序，再执行释放程序。滚轮不使用按住或开关重复。");
            }
        }
        if self.choice == Choice::Host {
            self.host_buttons.edit(ui, self.record < BUTTONS);
        }
        if self.choice == Choice::Shoot {
            ui.label(
                RichText::new("射击模式索引由固件解释，效果取决于射击设置。")
                    .font(skin::regular(13.0))
                    .color(skin::GRAY),
            );
        }
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            if skin::button(ui, "设定此按键", 130.0, self.keys.is_some()).clicked() {
                let result = (if self.record < BUTTONS {
                    self.binding()
                } else {
                    self.wheel_action().map(|press| Binding {
                        press,
                        release: Action::Direct(NO_OPERATION),
                    })
                })
                .and_then(|binding| {
                    let host = if self.choice == Choice::Host {
                        Some(self.host_buttons.action()?)
                    } else {
                        None
                    };
                    Ok((binding, host))
                });
                match result {
                    Ok((binding, host)) => {
                        if host
                            .as_ref()
                            .is_some_and(|action| action.number("Simulator") == Ok(2))
                        {
                            if self.record >= BUTTONS {
                                self.report(Err("手势不能指定给滚轮".into()));
                                return;
                            }
                            self.host_buttons.rules.gestures =
                                Some(Box::new(bloody_a90::host_buttons::Gestures {
                                    enabled: true,
                                    documents: self.gestures.documents.clone(),
                                }));
                        }
                        self.host_buttons
                            .rules
                            .resize(self.keys.as_ref().map_or(0, |c| c.profiles.len()));
                        if let Some(profile) = self
                            .keys
                            .as_mut()
                            .and_then(|config| config.profiles.get_mut(self.profile))
                        {
                            if self.record < BUTTONS {
                                profile.buttons[self.record] = binding;
                            } else {
                                profile.wheel[self.record - BUTTONS] = binding.press;
                            }
                            self.host_buttons.rules.profiles[self.profile][self.record] = host;
                            self.keys_dirty = true;
                        }
                    }
                    Err(error) => self.report(Err(error)),
                }
            }
            let caption = if self.watching {
                "停止查询"
            } else {
                "按键查询"
            };
            if skin::button(ui, caption, 130.0, self.connected())
                .on_hover_text("按住鼠标上的按键，自动选中对应记录")
                .clicked()
            {
                self.watching = !self.watching;
                self.held.clear();
            }
        });
        if self.record >= BUTTONS
            && skin::button(ui, "恢复此方向滚动", 160.0, self.keys.is_some()).clicked()
            && let Some(profile) = self
                .keys
                .as_mut()
                .and_then(|config| config.profiles.get_mut(self.profile))
        {
            profile.wheel[self.record - BUTTONS] =
                Action::Direct(if self.record == 24 { 0x262c } else { 0x262b });
            self.host_buttons
                .rules
                .resize(self.keys.as_ref().unwrap().profiles.len());
            self.host_buttons.rules.profiles[self.profile][self.record] = None;
            self.keys_dirty = true;
        }
        if self.watching {
            ui.label(
                RichText::new(if self.held.is_empty() {
                    "请按下鼠标按键".to_owned()
                } else {
                    format!("按住：{:?}", self.held)
                })
                .color(skin::CAPTION_RED),
            );
        }
        ui.add_space(6.0);
        ui.label(
            RichText::new("修改只影响当前模式。点击“应用”后写入鼠标并读回核对。")
                .font(skin::regular(13.0))
                .color(skin::GRAY),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extra_key_menus_encode_and_reload_combinations_without_changing_macro_drafts() {
        let context = egui::Context::default();
        let mut app = App::from_context(&context);
        let mut profile = bloody_a90::configuration::Profile {
            buttons: Default::default(),
            wheel: [Action::Direct(0x262c), Action::Direct(0x262b)],
            gestures: None,
            metadata: [0x1ff, 0],
        };
        profile.buttons[3] = button_actions::keyboard(27, 0x85).unwrap();
        profile.wheel[1] =
            button_actions::wheel(button_actions::keyboard(6, 1).unwrap(), true).unwrap();
        app.keys = Some(bloody_a90::configuration::Configuration {
            profiles: vec![profile],
            selected: 0,
            auxiliary: std::array::from_fn(|_| Action::Direct(NO_OPERATION)),
            parameters: [50; 6],
            flags: 0x10,
            header_word6: 1,
        });
        app.record = 3;
        app.host_key_selected();
        assert!(app.choice == Choice::Keyboard);
        assert_eq!(app.key_modifiers, 0x85);
        assert_eq!(
            app.binding().unwrap(),
            app.keys.as_ref().unwrap().profiles[0].buttons[3]
        );
        app.record = 25;
        app.host_key_selected();
        assert!(app.choice == Choice::Keyboard);
        assert_eq!(app.key_modifiers, 1);
        assert_eq!(
            app.wheel_action().unwrap(),
            Action::Macro(vec![0x2301, 0x2106, 0x2810, 0x2006, 0x2201, storage::END])
        );
        let draft = (app.press.clone(), app.release.clone(), app.repeat);
        for choice in [
            Choice::Keyboard,
            Choice::Media,
            Choice::System,
            Choice::Shortcut,
            Choice::Function,
        ] {
            for record in [3, 24, 25] {
                app.record = record;
                app.choice = choice;
                app.choice_value = 0;
                let mut output = context.run_ui(Default::default(), |ui| app.keys_page(ui));
                output.textures_delta.clear();
            }
        }
        assert_eq!((app.press.clone(), app.release.clone(), app.repeat), draft);
        for (index, (_, usage)) in SYSTEM.iter().enumerate() {
            app.choice = Choice::System;
            app.choice_value = index;
            assert_eq!(
                app.binding().unwrap(),
                Binding {
                    press: Action::Direct(0x3900 | usage),
                    release: Action::Direct(0x3800 | usage),
                }
            );
            assert_eq!(
                app.wheel_action().unwrap(),
                Action::Macro(vec![0x3900 | usage, 0x3800 | usage, storage::END])
            );
        }
        app.choice = Choice::Shortcut;
        app.choice_value = 12;
        assert_eq!(
            app.wheel_action().unwrap(),
            button_actions::clicks(2).unwrap().press
        );
        app.record = 3;
        app.keys.as_mut().unwrap().profiles[0].buttons[3] = app.binding().unwrap();
        app.host_key_selected();
        assert!(app.choice == Choice::Shortcut);
        assert_eq!(app.choice_value, 12);
        for (index, (_, usage)) in MEDIA.iter().enumerate() {
            assert!(bloody_a90::host_program::consumer(*usage).is_some());
            app.choice = Choice::Media;
            app.choice_value = index;
            assert_eq!(app.binding().unwrap().press, Action::Direct(0x3400 | usage));
        }
        app.choice = Choice::Function;
        app.choice_value = app
            .options()
            .iter()
            .position(|label| label == "锁定右键")
            .unwrap();
        assert_eq!(
            app.binding().unwrap().press,
            Action::Macro(vec![0x260d, storage::END])
        );
    }

    #[test]
    fn wheel_hardware_assignments_include_release_edges_and_keep_button_behavior() {
        let context = egui::Context::default();
        let mut app = App::from_context(&context);
        app.record = 24;
        app.choice = Choice::Mouse;
        app.choice_value = 0;
        assert_eq!(
            app.wheel_action().unwrap(),
            Action::Macro(vec![0x2501, 0x2401, storage::END])
        );
        app.choice = Choice::Keyboard;
        app.choice_value = highlanguage::keys()
            .position(|(usage, _)| usage == 4)
            .unwrap();
        assert_eq!(
            app.wheel_action().unwrap(),
            Action::Macro(vec![0x2104, 0x2810, 0x2004, storage::END])
        );
        app.choice_value = highlanguage::keys()
            .position(|(usage, _)| usage == 0xe0)
            .unwrap();
        assert_eq!(
            app.wheel_action().unwrap(),
            Action::Macro(vec![0x2301, 0x2810, 0x2201, storage::END])
        );
        app.choice = Choice::Media;
        app.choice_value = 0;
        assert_eq!(
            app.wheel_action().unwrap(),
            Action::Macro(vec![0x34cd, 0x30cd, storage::END])
        );
        app.choice = Choice::Function;
        assert!(!app.options().iter().any(|label| label.contains("按住")));
        app.choice_value = app
            .options()
            .iter()
            .position(|label| label == "配置上移")
            .unwrap();
        assert_eq!(app.wheel_action().unwrap(), Action::Direct(0x2622));
        app.choice = Choice::Macro;
        app.press = "KeyDown 4\ndelay 16".into();
        app.release = "KeyUp 4".into();
        app.repeat = bloody_a90::macros::Repeat::Held;
        app.recompile();
        assert_eq!(
            app.wheel_action().unwrap(),
            Action::Macro(vec![0x2104, 0x2810, 0x2004, storage::END])
        );
        app.choice = Choice::Disabled;
        assert_eq!(app.wheel_action().unwrap(), Action::Direct(NO_OPERATION));
        app.record = 8;
        app.choice = Choice::Function;
        assert!(app.options().iter().any(|label| label.contains("按住")));
        app.choice = Choice::Mouse;
        app.choice_value = 0;
        assert_eq!(app.binding().unwrap().press, Action::Direct(0x2501));
        assert_eq!(app.binding().unwrap().release, Action::Direct(0x2401));
    }
}
