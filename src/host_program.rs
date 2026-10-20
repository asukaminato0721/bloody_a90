//! Host execution of compiled M instructions. Does not send USB commands.
use crate::{
    amc,
    gestures::Action,
    macros::{self, Repeat},
    storage,
};
use std::{
    fs,
    sync::Arc,
    time::{Duration, Instant},
};

/// HID keyboard page to Linux input codes. Source: Linux hid-input.c.
pub fn keyboard(usage: u16) -> Option<u16> {
    const BASIC: [u16; 128] = [
        0, 0, 0, 0, 30, 48, 46, 32, 18, 33, 34, 35, 23, 36, 37, 38, 50, 49, 24, 25, 16, 19, 31, 20,
        22, 47, 17, 45, 21, 44, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 28, 1, 14, 15, 57, 12, 13, 26, 27,
        43, 43, 39, 40, 41, 51, 52, 53, 58, 59, 60, 61, 62, 63, 64, 65, 66, 67, 68, 87, 88, 99, 70,
        119, 110, 102, 104, 111, 107, 109, 106, 105, 108, 103, 69, 98, 55, 74, 78, 96, 79, 80, 81,
        75, 76, 77, 71, 72, 73, 82, 83, 86, 127, 116, 117, 183, 184, 185, 186, 187, 188, 189, 190,
        191, 192, 193, 194, 134, 138, 130, 132, 128, 129, 131, 137, 133, 135, 136, 113,
    ];
    let code = match usage {
        0..=127 => BASIC[usage as usize],
        128 => 115,
        129 => 114,
        133 => 121,
        135 => 89,
        136 => 93,
        137 => 124,
        138 => 92,
        139 => 94,
        140 => 95,
        144 => 122,
        145 => 123,
        146 => 90,
        147 => 91,
        148 => 85,
        156 | 216 => 111,
        182 => 179,
        183 => 180,
        224..=231 => [29, 42, 56, 125, 97, 54, 100, 126][usage as usize - 224],
        232..=251 => [
            164, 166, 165, 163, 161, 115, 114, 113, 150, 158, 159, 128, 136, 177, 178, 176, 142,
            152, 173, 140,
        ][usage as usize - 232],
        _ => return None,
    };
    (code != 0).then_some(code)
}

pub fn consumer(usage: u16) -> Option<u16> {
    Some(match usage {
        0x30 => 116,
        0x31 => 408,
        0x32 | 0x34 => 142,
        0x40 => 139,
        0x42 => 103,
        0x43 => 108,
        0x44 => 105,
        0x45 => 106,
        0x46 => 1,
        0xb0 => 207,
        0xb1 => 119,
        0xb2 => 167,
        0xb3 => 208,
        0xb4 => 168,
        0xb5 => 163,
        0xb6 => 165,
        0xb7 => 166,
        0xb8 => 161,
        0xcd => 164,
        0xe2 => 113,
        0xe9 => 115,
        0xea => 114,
        0x182 | 0x22a => 156,
        0x183 => 171,
        0x18a => 155,
        0x192 => 140,
        0x193 => 387,
        0x194 => 144,
        0x196 => 150,
        0x21a => 131,
        0x21b => 133,
        0x21c => 137,
        0x21d => 135,
        0x221 => 217,
        0x223 => 172,
        0x224 => 158,
        0x225 => 159,
        0x226 => 128,
        0x227 => 173,
        0x269 => 110,
        0x26a => 111,
        0x279 => 182,
        _ => return None,
    })
}

fn system(usage: u16) -> Option<u16> {
    match usage {
        0x81 => Some(116),
        0x82 => Some(142),
        0x83 => Some(143),
        _ => None,
    }
}

#[derive(Clone, Debug)]
pub struct Prepared {
    pub(crate) press: Arc<[u16]>,
    pub(crate) release: Arc<[u16]>,
    pub repeat: Repeat,
    open: Option<Arc<crate::host_open::Plan>>,
    text: Option<Arc<crate::host_text::Plan>>,
    window: bool,
}

impl Prepared {
    pub fn needs_buttons(&self) -> bool {
        reads_buttons(&self.press) || reads_buttons(&self.release)
    }

    pub fn needs_text(&self) -> bool {
        self.text.is_some()
    }
    pub fn needs_window(&self) -> bool {
        self.window
    }
}

fn reads_buttons(words: &[u16]) -> bool {
    let mut pc = 0;
    while pc < words.len() {
        if words[pc] >> 8 == 0x90 {
            return true;
        }
        pc += storage::width(words[pc]).unwrap_or(1);
    }
    false
}

/// Validate the entire program before any input is emitted.
fn validate(words: &[u16]) -> Result<(), String> {
    storage::validate(words)?;
    let mut pc = 0;
    while pc < words.len() {
        let word = words[pc];
        let low = word & 255;
        let valid = match word {
            0..=0x1fff
            | 0x2600
            | 0x2620
            | 0x262b..=0x262f
            | 0x2701..=0x27ff
            | 0x2801..=0x2be7
            | 0x3a00..=0x7fff => true,
            0x2000..=0x21ff => keyboard(low).is_some(),
            0x2200..=0x23ff => true,
            0x2400..=0x25ff => low & !31 == 0,
            0x3000..=0x37ff => consumer(word & 0x3ff).is_some(),
            0x3800..=0x39ff => system(low).is_some(),
            0x8000..=0x90ff => {
                let op = word >> 8;
                let value = words[pc + 1];
                match op {
                    0x80 => true,
                    0x81..=0x85 => value >> 8 <= 25,
                    0x86 | 0x87 => true,
                    0x88 | 0x8e | 0x8f => value >> 8 <= 25,
                    0x89..=0x8d => value >> 8 <= 25 && value & 255 <= 25,
                    0x90 => value <= 2,
                    _ => false,
                }
            }
            _ => false,
        };
        if !valid {
            return Err(format!(
                "主机手势不支持宏指令 {word:04x}。硬件功能指令请指定给鼠标按键。"
            ));
        }
        pc += storage::width(word).unwrap();
    }
    Ok(())
}

/// Ordinary vendor button notifications supply press and release separately.
/// EXE 0045cd4c, cases 20/21/22/24: mode 0/1 runs on press; mode 3 runs on
/// either phase. Mode 2 does not run these desktop actions.
pub fn prepare_button(action: &Action, pressed: bool) -> Result<Prepared, String> {
    if action.number("Simulator")? == 9
        && action.number("Object")? == 9
        && matches!(action.number("Action")?, 5 | 6)
    {
        // Horizontal wheel actions encode direction in Action. A native
        // physical press or wheel notch produces one scroll step.
        return if pressed {
            prepare(action)
        } else {
            prepare(&Action::default())
        };
    }
    if !matches!(action.number("Simulator")?, 20..=22 | 24) {
        return Err("按键主机动作只支持打开目标、文字、缩放、窗口动作和左右滚轮".into());
    }
    let mode = action.number("Action")?;
    if mode > 3 {
        return Err("按键主机动作的 Action 应为 0 至 3".into());
    }
    if mode == 3 || (pressed && mode <= 1) {
        prepare(&action.clone().with_mode(3))
    } else {
        prepare(&Action::default())
    }
}

pub fn prepare(action: &Action) -> Result<Prepared, String> {
    let (sim, object, mode) = (
        action.number("Simulator")?,
        action.number("Object")?,
        action.number("Action")?,
    );
    let once = |press: Vec<u16>| Prepared {
        press: press.into(),
        release: vec![storage::END].into(),
        repeat: Repeat::Once,
        open: None,
        text: None,
        window: false,
    };
    if sim == 0 {
        return Ok(once(vec![storage::END]));
    }
    if sim == 1 {
        let path = action.get("MacroPath");
        let metadata = fs::metadata(path).map_err(|e| format!("无法读取手势宏 {path}：{e}"))?;
        if metadata.len() > 1024 * 1024 {
            return Err("手势 AMC 宏超过 1 MiB".into());
        }
        let file = amc::parse(&fs::read(path).map_err(|e| e.to_string())?)?;
        // The host scheduler implements repeat, rather than the device branch.
        let normalized = if file.high_language {
            crate::highlanguage::translate(&file.press)?
        } else {
            file.press.clone()
        };
        let released = if file.high_language {
            crate::highlanguage::translate(&file.release)?
        } else {
            file.release.clone()
        };
        let program = macros::compile(&normalized, &released, Repeat::Once)?;
        validate(&program.press)?;
        validate(&program.release)?;
        return Ok(Prepared {
            press: program.press.into(),
            release: program.release.into(),
            repeat: file.repeat,
            open: None,
            text: None,
            window: false,
        });
    }
    if sim == 20 {
        if mode <= 2 {
            return Ok(once(vec![storage::END]));
        }
        if mode != 3 {
            return Err("打开目标动作需要 Action=0 至 3".into());
        }
        // 0045db32 reads record +18. 00546eb0 names it sMacroName.
        let plan = crate::host_open::Plan::prepare_with_arguments(
            action.get("MacroName"),
            action.get("LinuxArguments"),
        )?;
        return Ok(Prepared {
            press: vec![0xff20, storage::END].into(),
            release: vec![storage::END].into(),
            repeat: Repeat::Once,
            open: Some(Arc::new(plan)),
            text: None,
            window: false,
        });
    }
    if sim == 21 {
        if mode <= 2 {
            return Ok(once(vec![storage::END]));
        }
        if mode != 3 {
            return Err("文字输入动作需要 Action=0 至 3".into());
        }
        let plan = crate::host_text::Plan::prepare(action.get("HintText"))?;
        if plan.is_empty() {
            return Ok(once(vec![storage::END]));
        }
        return Ok(Prepared {
            press: vec![0xff21, storage::END].into(),
            release: vec![storage::END].into(),
            repeat: Repeat::Once,
            open: None,
            text: Some(Arc::new(plan)),
            window: false,
        });
    }
    if sim == 22 {
        if mode > 3 {
            return Err("缩放动作需要 Action=0 至 3".into());
        }
        if mode != 3 {
            return Ok(once(vec![storage::END]));
        }
        // GoZoomEvent chooses an application handler. 100017a0 uses Ctrl,
        // Sleep(1), one +/-120 wheel notch, then Ctrl up. The EXE passes count=1.
        return Ok(once(vec![
            0x2301,
            0x2801,
            if object == 0 { 0x262b } else { 0x262c },
            0x2201,
            storage::END,
        ]));
    }
    if sim == 24 {
        if mode > 3 {
            return Err("窗口动作需要 Action=0 至 3".into());
        }
        if mode != 3 {
            return Ok(once(vec![storage::END]));
        }
        // Object 0 is compiled to Win+M by 0062f6c8 on Windows. Linux uses
        // a desktop operation. The native host dispatcher handles only 1/2.
        // 004602df posts SC_MINIMIZE; 00460273 posts SC_MAXIMIZE.
        let mut prepared = once(vec![
            match object {
                0 => 0xff33,
                1 => 0xff31,
                _ => 0xff32,
            },
            storage::END,
        ]);
        prepared.window = true;
        return Ok(prepared);
    }
    let button = matches!(sim, 6..=8) || (sim == 9 && (1..=5).contains(&object));
    if !button && !(sim == 9 && object == 9) {
        return Err(format!("主机手势暂不支持 Simulator={sim}，Object={object}"));
    }
    if button && mode == 1 {
        // Both native result handlers (00464e61, 004651b7) pass phase 2.
        // A press-only action therefore has no event at gesture completion.
        return Ok(once(vec![storage::END]));
    }
    if (button && mode > 3) || (!button && !(sim == 9 && object == 9 && (3..=6).contains(&mode))) {
        return Err("按键动作需要 Action=0 至 3，滚轮需要 Action=3 至 6".into());
    }
    let mut words = Vec::new();
    match sim {
        6 if object / 1000 <= 255 && object % 1000 <= 255 => {
            let modifiers = (object / 1000) as u16;
            let usage = (object % 1000) as u16;
            if modifiers != 0 && mode == 3 {
                words.push(0x2300 | modifiers);
            }
            if usage != 0 {
                if (224..=231).contains(&usage) {
                    if mode == 3 {
                        words.extend([0x2300 | 1 << (usage - 224), 0x2810]);
                    }
                    words.push(0x2200 | 1 << (usage - 224));
                } else {
                    if mode == 3 {
                        words.extend([0x2100 | usage, 0x2810]);
                    }
                    words.push(0x2000 | usage);
                }
            } else if mode == 3 {
                words.push(0x2810);
            }
            if modifiers != 0 {
                words.push(0x2200 | modifiers);
            }
        }
        7 if object <= 1023 => {
            if mode == 3 {
                words.extend([0x3400 | object as u16, 0x2810]);
            }
            words.push(0x3000 | object as u16);
        }
        8 if object <= 255 => {
            if mode == 3 {
                words.extend([0x3900 | object as u16, 0x2810]);
            }
            words.push(0x3800 | object as u16);
        }
        9 if (1..=5).contains(&object) => {
            let mask = 1 << (object - 1);
            if mode == 3 {
                words.extend([0x2500 | mask, 0x2810]);
            }
            words.push(0x2400 | mask);
        }
        9 if object == 9 && mode <= 4 => words.push(if mode == 3 { 0x262b } else { 0x262c }),
        // Horizontal wheels are host outputs, represented by reserved local
        // instructions and handled before the ordinary firmware interpreter.
        9 if object == 9 && (mode == 5 || mode == 6) => {
            return Ok(once(vec![
                if mode == 5 { 0xff05 } else { 0xff06 },
                storage::END,
            ]));
        }
        _ => return Err(format!("主机手势暂不支持 Simulator={sim}，Object={object}")),
    }
    words.push(storage::END);
    validate(&words)?;
    Ok(once(words))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Output {
    Open(Arc<crate::host_open::Plan>),
    Text(Arc<crate::host_text::Plan>),
    Window(crate::host_window::Operation),
    Key(u16, bool),
    Button(u16, bool),
    Move(i32, i32),
    Wheel(i32, i32),
}

pub struct Vm {
    program: Prepared,
    pc: usize,
    release: bool,
    variables: [u8; 26],
    due: Instant,
    finished: bool,
    trigger_held: bool,
}

fn signed(value: u32, bits: u32) -> i32 {
    (value << (32 - bits)) as i32 >> (32 - bits)
}

impl Vm {
    pub fn new(program: Prepared, now: Instant, held: bool) -> Self {
        Self {
            program,
            pc: 0,
            release: false,
            variables: [0; 26],
            due: now,
            finished: false,
            trigger_held: held,
        }
    }

    pub fn stop(&mut self, now: Instant) {
        self.release = true;
        self.pc = 0;
        self.due = now;
    }

    pub fn released(&mut self, now: Instant) {
        self.trigger_held = false;
        if self.program.repeat == Repeat::Held {
            self.stop(now);
        }
    }

    pub fn finished(&self) -> bool {
        self.finished
    }
    pub fn repeat(&self) -> Repeat {
        self.program.repeat
    }
    pub fn ready(&self, now: Instant) -> bool {
        !self.finished && now >= self.due
    }
    pub fn needs_buttons(&self) -> bool {
        let words = if self.release {
            &self.program.release
        } else {
            &self.program.press
        };
        reads_buttons(words)
    }

    /// Bound each tick so input forwarding and cancellation remain responsive.
    pub fn tick(&mut self, now: Instant, buttons: [u8; 3]) -> Result<Vec<Output>, String> {
        let mut output = Vec::new();
        for _ in 0..256 {
            if self.finished || now < self.due {
                break;
            }
            let words = if self.release {
                &self.program.release
            } else {
                &self.program.press
            };
            let word = *words.get(self.pc).ok_or("主机宏指令地址越界")?;
            let low = word & 255;
            let size = if word >= 0xff00 {
                1
            } else {
                storage::width(word).ok_or("主机宏指令无效")?
            };
            let at = self.pc;
            self.pc += size;
            let operand = words.get(at + 1).copied().unwrap_or(0);
            match word {
                0..=0x1fff => self.pc = (at as i64 + i64::from(signed(word as u32, 13))) as usize,
                0x2000..=0x21ff => output.push(Output::Key(
                    keyboard(low).ok_or("主机键码无效")?,
                    word & 0x100 != 0,
                )),
                0x2200..=0x23ff => {
                    for bit in 0..8 {
                        if low & (1 << bit) != 0 {
                            output
                                .push(Output::Key(keyboard(224 + bit).unwrap(), word & 0x100 != 0));
                        }
                    }
                }
                0x2400..=0x25ff => {
                    for bit in 0..5 {
                        if low & (1 << bit) != 0 {
                            output.push(Output::Button(
                                crate::linux_input::BUTTONS[bit],
                                word & 0x100 != 0,
                            ));
                        }
                    }
                }
                0x2600 => output.push(Output::Move(
                    operand as i16 as i32,
                    words[at + 2] as i16 as i32,
                )),
                0x2620 => output.push(Output::Move(-32767, -32767)),
                0x262b => output.push(Output::Wheel(0, 1)),
                0x262c => output.push(Output::Wheel(0, -1)),
                0x262d => self.finished = true,
                0x262e => {}
                0x262f => {
                    if self.release {
                        self.finished = true;
                    } else if self.program.repeat == Repeat::Toggle
                        || (self.program.repeat == Repeat::Held && self.trigger_held)
                    {
                        self.pc = 0;
                        self.due = now + Duration::from_millis(1);
                    } else {
                        self.stop(now);
                    }
                }
                0x2701..=0x27ff => self.due = now + Duration::from_millis(u64::from(low) * 1024),
                0x2801..=0x2be7 => self.due = now + Duration::from_millis(u64::from(word - 0x2800)),
                0x3000..=0x37ff => output.push(Output::Key(
                    consumer(word & 0x3ff).ok_or("主机媒体键无效")?,
                    word & 0x400 != 0,
                )),
                0x3800..=0x39ff => output.push(Output::Key(
                    system(low).ok_or("主机系统键无效")?,
                    word & 0x100 != 0,
                )),
                0x3a00..=0x3aff => {
                    self.pc = (at as i64
                        + i64::from(signed(((low as u32) << 16) | operand as u32, 24)))
                        as usize
                }
                0x3b00..=0x3bff => output.push(Output::Move(
                    signed(((word as u32 >> 4 & 15) << 8) | (operand as u32 >> 8), 12),
                    signed(((word as u32 & 15) << 8) | (operand as u32 & 255), 12),
                )),
                0x4000..=0x7fff => output.push(Output::Move(
                    signed((word as u32 >> 7) & 127, 7),
                    signed(word as u32 & 127, 7),
                )),
                0x8000..=0x90ff => {
                    let op = word >> 8;
                    let destination = low as usize;
                    let register = (operand >> 8) as usize;
                    match op {
                        0x80 => self.variables[destination] = operand as u8,
                        0x88 => self.variables[destination] = self.variables[register],
                        0x90 => self.variables[destination] = buttons[operand as usize],
                        0x86 | 0x87 | 0x8e | 0x8f => {
                            let right = if op >= 0x8e {
                                self.variables[register]
                            } else {
                                register as u8
                            };
                            if (self.variables[destination] == right) == (op & 1 == 0) {
                                self.pc = (at as i64
                                    + i64::from(signed(
                                        ((operand as u32 & 255) << 16) | words[at + 2] as u32,
                                        24,
                                    ))) as usize;
                            }
                        }
                        _ => {
                            let left = self.variables[register];
                            let right = if op >= 0x89 {
                                self.variables[(operand & 255) as usize]
                            } else {
                                operand as u8
                            };
                            self.variables[destination] = match op & 7 {
                                1 => left.wrapping_add(right),
                                2 => left.wrapping_sub(right),
                                3 => left & right,
                                4 => left | right,
                                5 => left ^ right,
                                _ => return Err("主机变量指令无效".into()),
                            };
                        }
                    }
                }
                0xff05 => output.push(Output::Wheel(-1, 0)),
                0xff06 => output.push(Output::Wheel(1, 0)),
                0xff20 => output.push(Output::Open(
                    self.program.open.clone().ok_or("缺少打开目标")?,
                )),
                0xff21 => output.push(Output::Text(
                    self.program.text.clone().ok_or("缺少输入文字")?,
                )),
                0xff31 => output.push(Output::Window(crate::host_window::Operation::Minimize)),
                0xff32 => output.push(Output::Window(crate::host_window::Operation::Maximize)),
                0xff33 => output.push(Output::Window(crate::host_window::Operation::MinimizeAll)),
                _ => return Err(format!("主机宏指令未实现：{word:04x}")),
            }
        }
        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn native_horizontal_wheels_scroll_once_on_press_and_skip_release() {
        use super::*;
        for (mode, horizontal) in [(5, -1), (6, 1)] {
            for pressed in [false, true] {
                let now = Instant::now();
                let action = Action::new(9, 9, mode, "左右滚轮");
                let mut vm = Vm::new(prepare_button(&action, pressed).unwrap(), now, false);
                assert_eq!(
                    vm.tick(now, [0; 3]).unwrap(),
                    if pressed {
                        vec![Output::Wheel(horizontal, 0)]
                    } else {
                        Vec::new()
                    }
                );
                assert!(vm.finished());
            }
        }
        assert!(prepare_button(&Action::new(9, 1, 5, "错误对象"), true).is_err());
        assert!(prepare_button(&Action::new(9, 9, 7, "错误方向"), true).is_err());
    }
    #[test]
    fn player_and_file_manager_consumer_keys_keep_linux_press_and_release_codes() {
        use super::*;
        for (usage, code) in [(0x193, 387), (0x194, 144)] {
            let action = Action::new(7, usage, 3, "启动应用");
            let now = Instant::now();
            let mut vm = Vm::new(prepare(&action).unwrap(), now, false);
            assert_eq!(vm.tick(now, [0; 3]).unwrap(), [Output::Key(code, true)]);
            assert_eq!(
                vm.tick(now + Duration::from_millis(16), [0; 3]).unwrap(),
                [Output::Key(code, false)]
            );
            assert!(vm.finished());
        }
    }
    #[test]
    fn native_buttons_use_press_phases_for_desktop_actions() {
        use super::*;
        for action in [
            Action::open_target("/usr/bin/true").unwrap(),
            Action::input_text("中😀").unwrap(),
            Action::new(22, 0, 0, "放大"),
            Action::new(24, 1, 0, "最小化"),
        ] {
            for mode in 0..=3 {
                for pressed in [false, true] {
                    let plan = prepare_button(&action.clone().with_mode(mode), pressed).unwrap();
                    let now = Instant::now();
                    let events = Vm::new(plan, now, false).tick(now, [0; 3]).unwrap();
                    assert_eq!(!events.is_empty(), mode == 3 || (pressed && mode <= 1));
                }
            }
        }
        assert!(prepare_button(&Action::new(20, 0, 4, "错误阶段"), true).is_err());
        assert!(prepare_button(&Action::keyboard(4, 0, "硬件按键"), true).is_err());
    }
    use super::*;
    #[test]
    fn zoom_direction_delay_and_result_phase() {
        let now = Instant::now();
        for object in [0, 1, 2, u32::MAX] {
            for mode in 0..=3 {
                let mut vm = Vm::new(
                    prepare(&Action::new(22, object, mode, "缩放")).unwrap(),
                    now,
                    false,
                );
                if mode != 3 {
                    assert!(vm.tick(now, [0; 3]).unwrap().is_empty());
                    assert!(vm.finished());
                    continue;
                }
                assert_eq!(vm.tick(now, [0; 3]).unwrap(), [Output::Key(29, true)]);
                assert!(!vm.ready(now + Duration::from_micros(999)));
                assert_eq!(
                    vm.tick(now + Duration::from_millis(1), [0; 3]).unwrap(),
                    [
                        Output::Wheel(0, if object == 0 { 1 } else { -1 }),
                        Output::Key(29, false)
                    ]
                );
                assert!(vm.finished());
            }
        }
        assert!(prepare(&Action::new(22, 0, 4, "错误阶段")).is_err());
    }
    #[test]
    fn window_objects_keep_pointer_actions_and_translate_all_minimize_for_linux() {
        use crate::host_window::Operation;
        let now = Instant::now();
        for object in [0, 1, 2, 9, u32::MAX] {
            for mode in 0..=3 {
                let prepared = prepare(&Action::new(24, object, mode, "窗口动作")).unwrap();
                let executes = mode == 3;
                assert_eq!(prepared.needs_window(), executes);
                let mut vm = Vm::new(prepared, now, false);
                let expected = if executes {
                    vec![Output::Window(if object == 0 {
                        Operation::MinimizeAll
                    } else if object == 1 {
                        Operation::Minimize
                    } else {
                        Operation::Maximize
                    })]
                } else {
                    Vec::new()
                };
                assert_eq!(
                    vm.tick(now, [0; 3]).unwrap(),
                    expected,
                    "object={object}, mode={mode}"
                );
                assert!(vm.finished());
            }
        }
        assert!(prepare(&Action::new(24, 1, 4, "错误阶段")).is_err());
    }
    #[test]
    fn text_reads_hint_and_uses_release_result_phase() {
        let action = Action::input_text("中😀\r\nA").unwrap();
        let prepared = prepare(&action).unwrap();
        assert!(prepared.needs_text());
        let now = Instant::now();
        let mut vm = Vm::new(prepared, now, false);
        assert_eq!(
            vm.tick(now, [0; 3]).unwrap(),
            [Output::Text(Arc::new(
                crate::host_text::Plan::prepare("中😀\r\nA").unwrap()
            ))]
        );
        assert!(vm.finished());
        for mode in 0..=2 {
            let prepared = prepare(&Action::new(21, 0, mode, "输入文字")).unwrap();
            assert!(!prepared.needs_text());
            assert!(
                Vm::new(prepared, now, false)
                    .tick(now, [0; 3])
                    .unwrap()
                    .is_empty()
            );
        }
        assert!(prepare(&Action::new(21, 0, 4, "无效阶段")).is_err());
        let mut document = crate::gestures::Document::new(crate::gestures::Kind::Mouse);
        document.actions[0] = action;
        assert_eq!(
            crate::gestures::Document::parse(&document.to_bytes()).unwrap(),
            document
        );
    }

    #[test]
    fn shortcut_modifiers_and_timed_release() {
        let now = Instant::now();
        let mut vm = Vm::new(
            prepare(&Action::keyboard(27, 1, "剪切")).unwrap(),
            now,
            false,
        );
        assert_eq!(
            vm.tick(now, [0; 3]).unwrap(),
            [Output::Key(29, true), Output::Key(45, true)]
        );
        assert!(
            vm.tick(now + Duration::from_millis(15), [0; 3])
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            vm.tick(now + Duration::from_millis(16), [0; 3]).unwrap(),
            [Output::Key(45, false), Output::Key(29, false)]
        );
        assert!(vm.finished());
    }

    #[test]
    fn gesture_results_use_release_phase_for_non_tap_actions() {
        let now = Instant::now();
        for mode in [0, 1, 2] {
            let mut vm = Vm::new(
                prepare(&Action::new(6, 1027, mode, "剪切")).unwrap(),
                now,
                false,
            );
            let result = vm.tick(now, [0; 3]).unwrap();
            if mode == 1 {
                assert!(result.is_empty());
            } else {
                assert_eq!(result, [Output::Key(45, false), Output::Key(29, false)]);
            }
            assert!(vm.finished());
        }
        for (action, output) in [
            (Action::new(7, 205, 2, "播放释放"), Output::Key(164, false)),
            (Action::new(8, 0x82, 0, "睡眠释放"), Output::Key(142, false)),
            (
                Action::new(9, 2, 2, "右键释放"),
                Output::Button(0x111, false),
            ),
        ] {
            let mut vm = Vm::new(prepare(&action).unwrap(), now, false);
            assert_eq!(vm.tick(now, [0; 3]).unwrap(), [output]);
            assert!(vm.finished());
        }
        assert!(prepare(&Action::new(6, 4, 6, "错误事件")).is_err());
    }

    #[test]
    fn open_target_uses_the_native_macro_name_field() {
        let action = Action::open_target("/bin/true").unwrap();
        assert_eq!(action.get("MacroName"), "/bin/true");
        assert!(action.get("MacroPath").is_empty());
        let now = Instant::now();
        let mut vm = Vm::new(prepare(&action).unwrap(), now, false);
        assert_eq!(
            vm.tick(now, [0; 3]).unwrap(),
            [Output::Open(Arc::new(crate::host_open::Plan::Program(
                std::path::PathBuf::from("/bin/true")
            )))]
        );
        assert!(vm.finished());
        for mode in 0..=2 {
            let mut vm = Vm::new(
                prepare(&Action::new(20, 0, mode, "未触发")).unwrap(),
                now,
                false,
            );
            assert!(vm.tick(now, [0; 3]).unwrap().is_empty());
            assert!(vm.finished());
        }
        assert!(Action::open_target("").is_err());
    }

    #[test]
    fn button_preflight_covers_release_program_and_skips_numeric_operands() {
        let program = Prepared {
            press: vec![0x2600, 0x9000, 0, storage::END].into(),
            release: vec![0x9007, 1, storage::END].into(),
            repeat: Repeat::Once,
            open: None,
            text: None,
            window: false,
        };
        assert!(program.needs_buttons());
        let now = Instant::now();
        let mut vm = Vm::new(program, now, false);
        assert!(!vm.needs_buttons());
        vm.stop(now);
        assert!(vm.needs_buttons());
    }

    #[test]
    fn variables_branches_movement_release_and_repeat_cancel() {
        let program = macros::compile("var VarA = 254\nvar VarA = VarA + 3\nif VarA != 1 goto 6\nmover -2048 32767\nKeyDown 4", "KeyUp 4", Repeat::Once).unwrap();
        validate(&program.press).unwrap();
        let prepared = Prepared {
            press: program.press.into(),
            release: program.release.into(),
            repeat: Repeat::Once,
            open: None,
            text: None,
            window: false,
        };
        let now = Instant::now();
        let mut vm = Vm::new(prepared.clone(), now, false);
        assert_eq!(
            vm.tick(now, [0; 3]).unwrap(),
            [
                Output::Move(-2048, 32767),
                Output::Key(30, true),
                Output::Key(30, false)
            ]
        );
        assert!(vm.finished());
        let mut vm = Vm::new(
            Prepared {
                repeat: Repeat::Held,
                ..prepared
            },
            now,
            true,
        );
        assert_eq!(vm.tick(now, [0; 3]).unwrap().len(), 2);
        assert!(!vm.finished());
        vm.released(now);
        assert_eq!(vm.tick(now, [0; 3]).unwrap(), [Output::Key(30, false)]);
        assert!(vm.finished());
        assert!(prepare(&Action::new(7, 1023, 3, "未知")).is_err());
        assert!(validate(&[0x2626, storage::END]).is_err());
    }
}
