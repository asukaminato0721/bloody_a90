//! Browser bindings. The page talks to the mouse over WebHID (www/hid.js);
//! this module only parses flash images and builds the blocks to save, with
//! the same library code as the desktop tool.
use bloody_a90::{
    amc,
    backup::{Backup, ShootingRoute},
    button_actions,
    configuration::{self, Action, BUTTONS, Binding, Configuration, LIGHT_EFFECTS},
    highlanguage,
    i18n::{self, Language},
    key_menu::{self, CHOICES, Choice, FUNCTIONS, MEDIA, MOUSE, SHORTCUTS, SYSTEM, Shortcut},
    macros::{self, Program, Repeat},
    sensitivity,
    shooting::{self, Cycle, Shot},
    storage::{self, END, NO_OPERATION},
};
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

type Result<T> = std::result::Result<T, JsError>;

fn error(message: impl AsRef<str>) -> JsError {
    JsError::new(message.as_ref())
}

fn to_js<T: Serialize>(value: &T) -> Result<JsValue> {
    serde_wasm_bindgen::to_value(value).map_err(|e| error(e.to_string()))
}

fn from_js<T: for<'a> Deserialize<'a>>(value: JsValue) -> Result<T> {
    serde_wasm_bindgen::from_value(value).map_err(|e| error(e.to_string()))
}

/// Wheel records follow the 24 button records, as in the desktop editor.
const WHEEL_DOWN: usize = BUTTONS;
const RECORDS: usize = BUTTONS + 2;

fn repeat(code: u8) -> Result<Repeat> {
    match code {
        0 => Ok(Repeat::Once),
        1 => Ok(Repeat::Held),
        2 => Ok(Repeat::Toggle),
        _ => Err(error("重复方式应为 0、1 或 2")),
    }
}

fn repeat_code(repeat: Repeat) -> u8 {
    match repeat {
        Repeat::Once => 0,
        Repeat::Held => 1,
        Repeat::Toggle => 2,
    }
}

fn choice(code: &str) -> Result<Choice> {
    Ok(match code {
        "mouse" => Choice::Mouse,
        "keyboard" => Choice::Keyboard,
        "media" => Choice::Media,
        "system" => Choice::System,
        "shortcut" => Choice::Shortcut,
        "function" => Choice::Function,
        "shoot" => Choice::Shoot,
        "macro" => Choice::Macro,
        "disabled" => Choice::Disabled,
        _ => return Err(error(format!("未知动作类型 {code}"))),
    })
}

fn choice_code(choice: Choice) -> &'static str {
    match choice {
        Choice::Mouse => "mouse",
        Choice::Keyboard => "keyboard",
        Choice::Media => "media",
        Choice::System => "system",
        Choice::Shortcut => "shortcut",
        Choice::Function => "function",
        Choice::Shoot => "shoot",
        Choice::Macro => "macro",
        Choice::Host => "host",
        Choice::Disabled => "disabled",
    }
}

#[derive(Serialize)]
struct LanguageEntry {
    code: &'static str,
    name: &'static str,
}

/// The languages of the official program, as the desktop tool lists them.
#[wasm_bindgen]
pub fn languages() -> Result<JsValue> {
    let entries: Vec<_> = Language::ALL
        .iter()
        .map(|language| LanguageEntry {
            code: language.code(),
            name: language.name(),
        })
        .collect();
    to_js(&entries)
}

/// Select the display language by its official folder name.
#[wasm_bindgen(js_name = setLanguage)]
pub fn set_language(code: &str) -> bool {
    match Language::from_code(code) {
        Some(language) => {
            i18n::set_language(language);
            true
        }
        None => false,
    }
}

/// Show Simplified Chinese source text in the current language: an exact
/// caption, a known message template or else line by line.
#[wasm_bindgen]
pub fn tr(text: &str) -> String {
    i18n::tr_text(text)
}

/// Browser pages cannot run host actions, so their menu entry is left out.
#[derive(Serialize)]
struct ChoiceEntry {
    code: &'static str,
    label: &'static str,
}

#[wasm_bindgen]
pub fn choices() -> Result<JsValue> {
    let entries: Vec<_> = CHOICES
        .iter()
        .filter(|(choice, _)| *choice != Choice::Host)
        .map(|(choice, label)| ChoiceEntry {
            code: choice_code(*choice),
            label,
        })
        .collect();
    to_js(&entries)
}

fn functions(record: usize) -> impl Iterator<Item = &'static (&'static str, u16, u16)> {
    // Wheel steps have no release edge, so hold-only functions are left out.
    FUNCTIONS
        .iter()
        .filter(move |(_, _, release)| record < BUTTONS || *release == NO_OPERATION)
}

fn options(choice: Choice, record: usize) -> Vec<String> {
    match choice {
        Choice::Mouse => MOUSE.iter().map(|(name, _)| name.to_string()).collect(),
        Choice::Keyboard => highlanguage::keys()
            .map(|(_, name)| name.to_owned())
            .chain(std::iter::once("无主键（仅修饰键）".into()))
            .collect(),
        Choice::Media => MEDIA.iter().map(|(name, _)| name.to_string()).collect(),
        Choice::System => SYSTEM.iter().map(|(name, _)| name.to_string()).collect(),
        Choice::Shortcut => SHORTCUTS.iter().map(|(name, _)| name.to_string()).collect(),
        Choice::Function => functions(record).map(|(name, ..)| name.to_string()).collect(),
        Choice::Shoot => (1..=8).map(|index| format!("射击模式 {index}")).collect(),
        Choice::Macro | Choice::Host | Choice::Disabled => Vec::new(),
    }
}

/// Menu entries of one action type. Function entries depend on the record.
#[wasm_bindgen(js_name = choiceOptions)]
pub fn choice_options(code: &str, record: usize) -> Result<JsValue> {
    to_js(&options(choice(code)?, record))
}

/// The entry chosen in the key editor, as in App::binding.
struct Selection<'a> {
    choice: Choice,
    value: usize,
    modifiers: u8,
    program: Option<&'a Program>,
    /// Source text for a wheel macro, which runs both sections once.
    wheel_source: Option<(&'a str, &'a str)>,
}

impl Selection<'_> {
    fn binding(&self, record: usize) -> std::result::Result<Binding, String> {
        let direct = |press, release| Binding {
            press: Action::Direct(press),
            release: Action::Direct(release),
        };
        let entry = |count: usize| {
            (self.value < count)
                .then_some(self.value)
                .ok_or_else(|| "无效选项".to_string())
        };
        Ok(match self.choice {
            Choice::Mouse => {
                let mask = MOUSE[entry(MOUSE.len())?].1;
                direct(0x2500 | mask, 0x2400 | mask)
            }
            Choice::Keyboard => {
                let usage = if self.value == highlanguage::keys().count() {
                    0
                } else {
                    highlanguage::keys().nth(self.value).ok_or("无效按键")?.0
                };
                button_actions::keyboard(usage, self.modifiers)?
            }
            Choice::Media => {
                let usage = MEDIA[entry(MEDIA.len())?].1;
                direct(0x3400 | usage, 0x3000 | usage)
            }
            Choice::System => {
                let usage = SYSTEM[entry(SYSTEM.len())?].1;
                direct(0x3900 | usage, 0x3800 | usage)
            }
            Choice::Shortcut => match SHORTCUTS[entry(SHORTCUTS.len())?].1 {
                Shortcut::Keyboard(usage, modifiers) => button_actions::keyboard(usage, modifiers)?,
                Shortcut::Clicks(count) => button_actions::clicks(count)?,
            },
            Choice::Function => {
                let (_, press, release) = functions(record).nth(self.value).ok_or("无效功能")?;
                Binding {
                    press: Action::from_program(vec![*press, END]),
                    release: Action::from_program(vec![*release, END]),
                }
            }
            Choice::Shoot => direct(0x2610 + entry(8)? as u16, NO_OPERATION),
            Choice::Macro => {
                let program = self.program.ok_or("宏程序无法编译")?;
                Binding {
                    press: Action::from_program(program.press.clone()),
                    release: Action::from_program(program.release.clone()),
                }
            }
            Choice::Host | Choice::Disabled => direct(NO_OPERATION, NO_OPERATION),
        })
    }

    fn wheel(&self, record: usize) -> std::result::Result<Action, String> {
        if self.choice == Choice::Macro {
            let (press, release) = self.wheel_source.ok_or("宏程序无法编译")?;
            return macros::compile_wheel(press, release).map(Action::from_program);
        }
        let keyboard_delay = self.choice == Choice::Keyboard
            || (self.choice == Choice::Shortcut
                && matches!(
                    SHORTCUTS.get(self.value).map(|s| s.1),
                    Some(Shortcut::Keyboard(..))
                ));
        button_actions::wheel(self.binding(record)?, keyboard_delay)
    }
}

fn describe(action: &Action) -> String {
    match action {
        Action::Direct(word) => storage::describe(*word),
        Action::Macro(words) => format!("宏（{} 字）", words.len()),
    }
}

/// One instruction of a compiled program, as the desktop listings show it.
#[derive(Serialize)]
struct Line {
    offset: usize,
    hex: String,
    text: String,
}

fn lines(words: &[u16]) -> Vec<Line> {
    storage::list(words)
        .instructions
        .into_iter()
        .map(|instruction| Line {
            offset: instruction.offset,
            hex: macros::hex(&instruction.words),
            text: instruction.text,
        })
        .collect()
}

#[derive(Serialize)]
struct RecordView {
    record: usize,
    name: String,
    physical: bool,
    choice: &'static str,
    value: usize,
    modifiers: u8,
    caption: String,
    press: String,
    release: String,
}

#[derive(Serialize)]
struct Info {
    model: u16,
    firmware: u8,
    core: u8,
    summary: String,
    shooting_route: &'static str,
    shooting_note: &'static str,
}

#[derive(Serialize)]
struct SensitivityView {
    presets: [[u32; 2]; 5],
    count: usize,
    selected: usize,
    rate_hz: Option<u32>,
    lift: Option<u8>,
    checksum_valid: Option<bool>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ShotInput {
    interval_ms: u32,
    x: i32,
    y: i32,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ShootingInput {
    /// a, b, c, d or m.
    kind: String,
    shots: Vec<ShotInput>,
    loop_start: usize,
    recover: bool,
    special: bool,
}

#[derive(Serialize)]
struct ProgramView {
    press: Vec<u16>,
    release: Vec<u16>,
    press_lines: Vec<Line>,
    release_lines: Vec<Line>,
}

fn program_view(program: &Program) -> ProgramView {
    ProgramView {
        press: program.press.clone(),
        release: program.release.clone(),
        press_lines: lines(&program.press),
        release_lines: lines(&program.release),
    }
}

/// One flash image and the key-table draft edited from it.
#[wasm_bindgen]
pub struct Editor {
    backup: Backup,
    config: std::result::Result<Configuration, String>,
    /// Oscar II editor text, used by the 宏 action type.
    press: String,
    release: String,
    repeat: Repeat,
    compiled: std::result::Result<Program, String>,
}

#[wasm_bindgen]
impl Editor {
    /// A complete 32768-byte flash image, read from the mouse or a file.
    #[wasm_bindgen(constructor)]
    pub fn new(bytes: &[u8]) -> Result<Editor> {
        let backup = Backup::parse(bytes).map_err(error)?;
        let press = "KeyDown 4\nDelay 25\nKeyUp 4".to_string();
        Ok(Editor {
            config: backup.configuration(),
            compiled: macros::compile(&press, "", Repeat::Once),
            backup,
            press,
            release: String::new(),
            repeat: Repeat::Once,
        })
    }

    /// Replace the image after a save. `keep_keys` keeps the key draft, for
    /// saves that did not touch the key table.
    pub fn reload(&mut self, bytes: &[u8], keep_keys: bool) -> Result<()> {
        self.backup = Backup::parse(bytes).map_err(error)?;
        if !keep_keys || self.config.is_err() {
            self.config = self.backup.configuration();
        }
        Ok(())
    }

    pub fn info(&self) -> Result<JsValue> {
        let model = self.backup.model();
        let route = self.backup.shooting_route();
        to_js(&Info {
            model,
            firmware: self.backup.layout().firmware,
            core: (model & 3) as u8 + 1,
            summary: self.backup.summary(),
            shooting_route: match route {
                ShootingRoute::Absent => "absent",
                ShootingRoute::Present => "present",
                ShootingRoute::Unknown => "unknown",
            },
            shooting_note: route.description(),
        })
    }

    #[wasm_bindgen(js_name = toBytes)]
    pub fn to_bytes(&self) -> Vec<u8> {
        self.backup.to_bytes()
    }

    // ---- Sensitivity page ----

    pub fn sensitivity(&self) -> Result<JsValue> {
        let data = self.backup.sensitivity().map_err(error)?;
        let template = self.backup.sensitivity_template().map_err(error)?;
        let mut presets = [[1000; 2], [1200; 2], [1600; 2], [2000; 2], [6200; 2]];
        for (preset, record) in presets.iter_mut().zip(&data.records) {
            if let Some(axes) = sensitivity::decode(*record) {
                *preset = axes;
            }
        }
        let count = data.count.min(sensitivity::PRESETS);
        to_js(&SensitivityView {
            presets,
            count,
            selected: data.selected.min(count - 1),
            rate_hz: [125, 250, 500, 1000]
                .into_iter()
                .find(|hz| sensitivity::rate_code(*hz) == Ok(data.rate_code)),
            lift: sensitivity::lift_height(&template).map_err(error)?,
            checksum_valid: data.checksum_valid,
        })
    }

    /// The block for a DPI save: active presets [x, y] and the selected one.
    #[wasm_bindgen(js_name = sensitivityBlock)]
    pub fn sensitivity_block(&self, presets: JsValue, selected: usize) -> Result<Vec<u16>> {
        let presets: Vec<[u32; 2]> = from_js(presets)?;
        let template = self.backup.sensitivity_template().map_err(error)?;
        sensitivity::block(&template, &presets, selected, None).map_err(error)
    }

    #[wasm_bindgen(js_name = rateBlock)]
    pub fn rate_block(&self, hz: u32) -> Result<Vec<u16>> {
        let template = self.backup.sensitivity_template().map_err(error)?;
        sensitivity::with_rate(&template, hz).map_err(error)
    }

    /// None keeps the sensor's preset height.
    #[wasm_bindgen(js_name = liftBlock)]
    pub fn lift_block(&self, level: Option<u8>) -> Result<Vec<u16>> {
        let template = self.backup.sensitivity_template().map_err(error)?;
        sensitivity::with_lift_height(&template, level).map_err(error)
    }

    #[wasm_bindgen(js_name = sensitivityTemplate)]
    pub fn sensitivity_template(&self) -> Result<Vec<u16>> {
        self.backup.sensitivity_template().map_err(error)
    }

    // ---- Key table draft ----

    fn config(&self) -> Result<&Configuration> {
        self.config.as_ref().map_err(|e| error(format!("按键表无法转换：{e}")))
    }

    fn config_mut(&mut self) -> Result<&mut Configuration> {
        self.config
            .as_mut()
            .map_err(|e| error(format!("按键表无法转换：{e}")))
    }

    /// Discard draft changes. `factory` loads the factory table instead.
    #[wasm_bindgen(js_name = resetKeys)]
    pub fn reset_keys(&mut self, factory: bool) {
        self.config = if factory {
            self.backup.factory_configuration()
        } else {
            self.backup.configuration()
        };
    }

    #[wasm_bindgen(js_name = profileCount)]
    pub fn profile_count(&self) -> Result<usize> {
        Ok(self.config()?.profiles.len())
    }

    /// The selected profile saved in the table header.
    #[wasm_bindgen(js_name = savedProfile)]
    pub fn saved_profile(&self) -> Result<usize> {
        Ok(self.config()?.selected)
    }

    fn selection_of(&self, profile: usize, record: usize) -> Result<(Selection<'static>, Binding)> {
        let config = self.config()?;
        let profile = config.profiles.get(profile).ok_or(error("配置不存在"))?;
        let wheel = record >= BUTTONS;
        let actual = if wheel {
            Binding {
                press: profile.wheel[record - BUTTONS].clone(),
                release: Action::Direct(NO_OPERATION),
            }
        } else {
            profile.buttons[record].clone()
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
            let selection = Selection {
                choice: Choice::Keyboard,
                value: index,
                modifiers,
                program: None,
                wheel_source: None,
            };
            return Ok((selection, actual));
        }
        for choice in [
            Choice::Mouse,
            Choice::Media,
            Choice::System,
            Choice::Shortcut,
            Choice::Function,
            Choice::Shoot,
            Choice::Disabled,
        ] {
            for value in 0..options(choice, record).len().max(1) {
                let selection = Selection {
                    choice,
                    value,
                    modifiers: 0,
                    program: None,
                    wheel_source: None,
                };
                let matches = if wheel {
                    selection.wheel(record).is_ok_and(|a| a == actual.press)
                } else {
                    selection.binding(record).is_ok_and(|b| b == actual)
                };
                if matches {
                    return Ok((selection, actual));
                }
            }
        }
        let selection = Selection {
            choice: Choice::Macro,
            value: 0,
            modifiers: 0,
            program: None,
            wheel_source: None,
        };
        Ok((selection, actual))
    }

    fn record_view(&self, profile: usize, record: usize) -> Result<RecordView> {
        if record >= RECORDS {
            return Err(error("记录不存在"));
        }
        let (selection, actual) = self.selection_of(profile, record)?;
        let caption = match selection.choice {
            Choice::Mouse => format!("鼠标{}", MOUSE[selection.value].0),
            Choice::Keyboard => {
                let mut parts: Vec<String> = ["Ctrl", "Shift", "Alt", "Super"]
                    .into_iter()
                    .enumerate()
                    .filter(|(index, _)| selection.modifiers & (0x11 << index) != 0)
                    .map(|(_, name)| name.to_string())
                    .collect();
                if let Some((_, name)) = highlanguage::keys().nth(selection.value) {
                    parts.push(name.to_string());
                }
                format!("键盘：{}", parts.join("+"))
            }
            Choice::Disabled => "禁用".into(),
            Choice::Macro => describe(&actual.press),
            choice => options(choice, record)
                .get(selection.value)
                .cloned()
                .unwrap_or_default(),
        };
        Ok(RecordView {
            record,
            name: key_menu::record_name(record),
            physical: key_menu::PHYSICAL.contains(&record) || record >= BUTTONS,
            choice: choice_code(selection.choice),
            value: selection.value,
            modifiers: selection.modifiers,
            caption,
            press: describe(&actual.press),
            release: describe(&actual.release),
        })
    }

    /// All 24 button records and the two wheel directions of one profile.
    pub fn records(&self, profile: usize) -> Result<JsValue> {
        let views = (0..RECORDS)
            .map(|record| self.record_view(profile, record))
            .collect::<Result<Vec<_>>>()?;
        to_js(&views)
    }

    /// Assign a menu entry. The 宏 type uses the Oscar II editor program.
    pub fn assign(
        &mut self,
        profile: usize,
        record: usize,
        code: &str,
        value: usize,
        modifiers: u8,
    ) -> Result<()> {
        if record >= RECORDS {
            return Err(error("记录不存在"));
        }
        let program = self.compiled.as_ref().ok();
        let selection = Selection {
            choice: choice(code)?,
            value,
            modifiers,
            program,
            wheel_source: program.map(|_| (self.press.as_str(), self.release.as_str())),
        };
        let binding = if record < BUTTONS {
            selection.binding(record)
        } else {
            selection.wheel(record).map(|press| Binding {
                press,
                release: Action::Direct(NO_OPERATION),
            })
        }
        .map_err(error)?;
        let config = self.config_mut()?;
        let profile = config.profiles.get_mut(profile).ok_or(error("配置不存在"))?;
        if record < BUTTONS {
            profile.buttons[record] = binding;
        } else {
            profile.wheel[record - BUTTONS] = binding.press;
        }
        Ok(())
    }

    /// Restore the normal scroll step of one wheel direction.
    #[wasm_bindgen(js_name = restoreWheel)]
    pub fn restore_wheel(&mut self, profile: usize, record: usize) -> Result<()> {
        if !(BUTTONS..RECORDS).contains(&record) {
            return Err(error("记录不是滚轮方向"));
        }
        let config = self.config_mut()?;
        let profile = config.profiles.get_mut(profile).ok_or(error("配置不存在"))?;
        profile.wheel[record - BUTTONS] =
            Action::Direct(if record == WHEEL_DOWN { 0x262c } else { 0x262b });
        Ok(())
    }

    /// The main vector to save at 2600.
    #[wasm_bindgen(js_name = buttonsBlock)]
    pub fn buttons_block(&self) -> Result<Vec<u16>> {
        configuration::build(self.config()?).map_err(error)
    }

    /// Hardware button response time 0..16 ms, kept in the key table.
    pub fn debounce(&self) -> Result<Option<u8>> {
        Ok(self.config()?.debounce_ms())
    }

    #[wasm_bindgen(js_name = setDebounce)]
    pub fn set_debounce(&mut self, ms: u8) -> Result<()> {
        self.config_mut()?.set_debounce_ms(ms).map_err(error)
    }

    /// The effect shared by all profiles, or undefined when they differ.
    #[wasm_bindgen(js_name = lightEffect)]
    pub fn light_effect(&self) -> Result<Option<u8>> {
        Ok(self.config()?.light_effect())
    }

    #[wasm_bindgen(js_name = setLightEffect)]
    pub fn set_light_effect(&mut self, effect: u8) -> Result<()> {
        self.config_mut()?.set_light_effect(effect).map_err(error)
    }

    /// Profile colours as #rrggbb, from the metadata words.
    #[wasm_bindgen(js_name = profileColors)]
    pub fn profile_colors(&self) -> Result<Vec<String>> {
        Ok(self
            .config()?
            .profiles
            .iter()
            .map(|p| format!("#{:02x}{:04x}", p.metadata[0] & 0xff, p.metadata[1]))
            .collect())
    }

    // ---- Oscar II editor ----

    /// Set the editor text and compile it. Returns the compiled program.
    #[wasm_bindgen(js_name = setMacro)]
    pub fn set_macro(&mut self, press: &str, release: &str, repeat_code: u8) -> Result<JsValue> {
        self.press = press.to_owned();
        self.release = release.to_owned();
        self.repeat = repeat(repeat_code)?;
        self.compiled = macros::compile(press, release, self.repeat);
        match &self.compiled {
            Ok(program) => to_js(&program_view(program)),
            Err(e) => Err(error(e)),
        }
    }

    /// Write the editor program to the fourth key (后侧键) of one profile,
    /// as the official 测试 button does. The caller saves the block.
    #[wasm_bindgen(js_name = testMacroBlock)]
    pub fn test_macro_block(&self, profile: usize) -> Result<Vec<u16>> {
        let program = self.compiled.as_ref().map_err(error)?;
        let mut config = self.backup.configuration().map_err(error)?;
        let profile = profile.min(config.profiles.len() - 1);
        config.profiles[profile].buttons[3] = Binding {
            press: Action::from_program(program.press.clone()),
            release: Action::from_program(program.release.clone()),
        };
        configuration::build(&config).map_err(error)
    }

    // ---- Shooting (左键编程 / 狙击键) ----

    fn shooting_program(&self, input: &ShootingInput) -> std::result::Result<Program, String> {
        let shots: Vec<Shot> = input
            .shots
            .iter()
            .map(|s| Shot {
                interval_ms: s.interval_ms,
                x: s.x,
                y: s.y,
            })
            .collect();
        match input.kind.as_str() {
            "m" => self.compiled.as_ref().map_err(Clone::clone).map(|p| Program {
                press: p.press.clone(),
                release: p.release.clone(),
            }),
            "a" => shooting::burst(&shots, input.recover, input.special).map(|press| Program {
                press,
                release: vec![END],
            }),
            kind => {
                let cycle = match kind {
                    "b" => Cycle::B,
                    "c" => Cycle::C,
                    "d" => Cycle::D,
                    _ => return Err("未知射击类型".into()),
                };
                let program = shooting::cycle(
                    cycle,
                    &shots,
                    input.loop_start.saturating_sub(1),
                    input.recover,
                    input.special,
                )?;
                Ok(Program {
                    press: program.press,
                    release: program.release,
                })
            }
        }
    }

    /// Compile a shooting slot program for preview.
    #[wasm_bindgen(js_name = shootingPreview)]
    pub fn shooting_preview(&self, input: JsValue) -> Result<JsValue> {
        let input: ShootingInput = from_js(input)?;
        let program = self.shooting_program(&input).map_err(error)?;
        to_js(&program_view(&program))
    }

    /// Store a slot program (1..7) in the draft and optionally point a
    /// trigger record at it, in one profile or all of them.
    #[wasm_bindgen(js_name = setShooting)]
    pub fn set_shooting(
        &mut self,
        slot: usize,
        input: JsValue,
        trigger: Option<usize>,
        profile: Option<usize>,
    ) -> Result<()> {
        let input: ShootingInput = from_js(input)?;
        let program = self.shooting_program(&input).map_err(error)?;
        let vertical = (input.kind == "d")
            .then(|| input.shots.first())
            .flatten()
            .map(|s| Shot {
                interval_ms: s.interval_ms,
                x: s.x,
                y: s.y,
            });
        let mut config = self.config()?.clone();
        config
            .set_shooting(
                slot,
                Binding {
                    press: Action::from_program(program.press),
                    release: Action::from_program(program.release),
                },
            )
            .map_err(error)?;
        config
            .set_shooting_parameters(slot, vertical)
            .map_err(error)?;
        if let Some(trigger) = trigger {
            if trigger >= BUTTONS {
                return Err(error("触发键必须是按键记录"));
            }
            for (index, p) in config.profiles.iter_mut().enumerate() {
                if profile.is_none_or(|only| only == index) {
                    p.buttons[trigger] = Binding {
                        press: Action::Direct(0x2610 + slot as u16),
                        release: Action::Direct(NO_OPERATION),
                    };
                }
            }
        }
        configuration::build(&config).map_err(error)?;
        self.config = Ok(config);
        Ok(())
    }

    /// 指定切换按键: point a button record at slot 1..7, in one profile or
    /// all of them. The slot program is stored by `setShooting`.
    #[wasm_bindgen(js_name = assignShootingTrigger)]
    pub fn assign_shooting_trigger(
        &mut self,
        slot: usize,
        trigger: usize,
        profile: Option<usize>,
    ) -> Result<()> {
        if !(1..=7).contains(&slot) || trigger >= BUTTONS {
            return Err(error("射击槽位应为 1 至 7，触发键必须是按键记录"));
        }
        for (index, p) in self.config_mut()?.profiles.iter_mut().enumerate() {
            if profile.is_none_or(|only| only == index) {
                p.buttons[trigger] = Binding {
                    press: Action::Direct(0x2610 + slot as u16),
                    release: Action::Direct(NO_OPERATION),
                };
            }
        }
        Ok(())
    }
}

/// X scale, Y scale and sensor code of one preset, for 查看编码参数.
#[wasm_bindgen(js_name = encodeDpi)]
pub fn encode_dpi(x: u32, y: u32) -> Result<Vec<u16>> {
    let record = sensitivity::encode(x, y).map_err(error)?;
    Ok(vec![record.x_scale, record.y_scale, record.sensor_code])
}

#[wasm_bindgen(js_name = lightEffects)]
pub fn light_effects() -> Vec<String> {
    LIGHT_EFFECTS.iter().map(|name| name.to_string()).collect()
}

#[derive(Serialize)]
struct AmcView {
    description: String,
    high_language: bool,
    repeat: u8,
    press: String,
    release: String,
}

/// Load an official AMC file. HighLanguage text becomes M text, so the
/// editor always holds what the compiler encodes.
#[wasm_bindgen(js_name = parseAmc)]
pub fn parse_amc(bytes: &[u8]) -> Result<JsValue> {
    let file = amc::parse(bytes).map_err(error)?;
    let text = |source: &str| {
        if file.high_language {
            highlanguage::translate(source)
        } else {
            Ok(source.replace("\r\n", "\n"))
        }
    };
    to_js(&AmcView {
        press: text(&file.press).map_err(error)?,
        release: text(&file.release).map_err(error)?,
        description: file.description,
        high_language: file.high_language,
        repeat: repeat_code(file.repeat),
    })
}

#[wasm_bindgen(js_name = exportAmc)]
pub fn export_amc(description: &str, press: &str, release: &str, repeat_code: u8) -> Result<Vec<u8>> {
    Ok(amc::Macro {
        description: description.to_owned(),
        high_language: false,
        repeat: repeat(repeat_code)?,
        press: press.to_owned(),
        release: release.to_owned(),
    }
    .to_bytes())
}

/// Command 0d payload for a sensitivity block's selected preset and rate.
#[wasm_bindgen(js_name = runtimePayload)]
pub fn runtime_payload(template: &[u16]) -> Result<Vec<u8>> {
    sensitivity::Runtime::from_template(template)
        .and_then(|runtime| runtime.payload())
        .map(|payload| payload.to_vec())
        .map_err(error)
}

/// Validate eight runtime bytes read from RAM; return them unchanged.
#[wasm_bindgen(js_name = checkRuntimePayload)]
pub fn check_runtime_payload(payload: &[u8]) -> Result<Vec<u8>> {
    let bytes: [u8; 8] = payload.try_into().map_err(|_| error("运行参数应为 8 字节"))?;
    sensitivity::Runtime::from_payload(bytes).map_err(error)?;
    Ok(bytes.to_vec())
}

/// Lift level of a sensitivity block, undefined for the preset height.
#[wasm_bindgen(js_name = liftHeight)]
pub fn lift_height(template: &[u16]) -> Result<Option<u8>> {
    sensitivity::lift_height(template).map_err(error)
}

/// Checksums over words 4.. of a saved block, as the firmware checks them.
#[wasm_bindgen]
pub fn checksums(words: &[u16]) -> Vec<u16> {
    let (a, b) = storage::checksums(words);
    vec![a, b]
}

/// Debounce byte of a key-table header word 6, if it holds 0..16 ms.
#[wasm_bindgen(js_name = debounceOfHeader)]
pub fn debounce_of_header(word6: u16) -> Option<u8> {
    let value = word6 as u8;
    (1..=17).contains(&value).then(|| value - 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_round_trips_the_menu() {
        for record in [3, BUTTONS, BUTTONS + 1] {
            for (choice, _) in CHOICES {
                if matches!(choice, Choice::Host | Choice::Macro) {
                    continue;
                }
                for value in 0..options(choice, record).len().max(1) {
                    let selection = Selection {
                        choice,
                        value,
                        modifiers: if choice == Choice::Keyboard { 1 } else { 0 },
                        program: None,
                        wheel_source: None,
                    };
                    if record < BUTTONS {
                        assert!(selection.binding(record).is_ok());
                    } else {
                        assert!(selection.wheel(record).is_ok());
                    }
                }
            }
        }
    }
}
