//! Hardware assignments from EXE 0062f6c8. Buttons have two phases. Each
//! wheel direction has one complete program, including any required releases.
use crate::{
    configuration::{Action, Binding},
    storage::{self, END, NO_OPERATION},
};

fn program(mut words: Vec<u16>) -> Result<Action, String> {
    words.push(END);
    storage::validate(&words)?;
    Ok(Action::from_program(words))
}

/// USB keyboard usage plus left/right modifier bits e0..e7. A modifier
/// selected as the main key joins the mask instead of producing two holds.
pub fn keyboard(usage: u16, mut modifiers: u8) -> Result<Binding, String> {
    if usage > 255 || (usage == 0 && modifiers == 0) {
        return Err("需要有效键盘按键或修饰键".into());
    }
    let modifier_key = (0xe0..=0xe7).contains(&usage);
    if modifier_key {
        modifiers |= 1 << (usage - 0xe0);
    }
    let mut press = Vec::new();
    let mut release = Vec::new();
    for bit in 0..8 {
        if modifiers & (1 << bit) != 0 {
            press.push(0x2300 | (1 << bit));
        }
    }
    if usage != 0 && !modifier_key {
        press.push(0x2100 | usage);
        release.push(0x2000 | usage);
    }
    for bit in (0..8).rev() {
        if modifiers & (1 << bit) != 0 {
            release.push(0x2200 | (1 << bit));
        }
    }
    Ok(Binding {
        press: program(press)?,
        release: program(release)?,
    })
}

fn body(action: &Action) -> Result<Vec<u16>, String> {
    match action {
        Action::Direct(NO_OPERATION) => Ok(Vec::new()),
        Action::Direct(word) if storage::is_direct(*word) => Ok(vec![*word]),
        Action::Direct(_) => Err("按键记录包含无效的直接动作".into()),
        Action::Macro(words) => {
            storage::validate(words)?;
            Ok(words[..words.len() - 1].to_vec())
        }
    }
}

/// EXE wheel mode 1 appends keyboard releases after 16 ms. Mouse,
/// Consumer and System assignments append releases without that delay.
pub fn wheel(binding: Binding, keyboard_delay: bool) -> Result<Action, String> {
    let mut press = body(&binding.press)?;
    let release = body(&binding.release)?;
    if release.is_empty() {
        return Ok(binding.press);
    }
    if keyboard_delay && !press.is_empty() {
        press.push(0x2810);
    }
    press.extend(release);
    program(press)
}

/// Recognize only a complete canonical keyboard assignment. Other macros
/// remain macros, including an isolated key press without a matching release.
pub fn keyboard_value(binding: &Binding) -> Option<(u16, u8)> {
    let mut usage = 0;
    let mut modifiers = 0;
    for word in body(&binding.press).ok()? {
        match word {
            0x2301..=0x2380 if (word & 255).count_ones() == 1 => {
                modifiers |= word as u8;
            }
            0x2101..=0x21ff if usage == 0 => usage = word & 255,
            _ => return None,
        }
    }
    (keyboard(usage, modifiers).ok()? == *binding).then_some((usage, modifiers))
}

pub fn wheel_keyboard_value(action: &Action) -> Option<(u16, u8)> {
    let words = body(action).ok()?;
    let delay = words.iter().position(|word| *word == 0x2810)?;
    let binding = Binding {
        press: program(words[..delay].to_vec()).ok()?,
        release: program(words[delay + 1..].to_vec()).ok()?,
    };
    let value = keyboard_value(&binding)?;
    (wheel(binding, true).ok()? == *action).then_some(value)
}

/// Simulator 23, Object 0/1: complete left-button double/triple clicks.
/// Each hold and each interval lasts 64 ms in the original converter.
pub fn clicks(count: usize) -> Result<Binding, String> {
    if !(2..=3).contains(&count) {
        return Err("连击预设需要 2 或 3 次点击".into());
    }
    let mut words = Vec::new();
    for index in 0..count {
        if index != 0 {
            words.push(0x2840);
        }
        words.extend([0x2501, 0x2840, 0x2401]);
    }
    Ok(Binding {
        press: program(words)?,
        release: Action::Direct(NO_OPERATION),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires /dev/uinput; grabs only its own generated outputs, without accessing the A90"]
    fn wheel_keyboard_combination_emits_and_releases_all_uinput_keys() {
        use crate::{
            gesture_runtime::Devices,
            host_program::{self, Vm},
            linux_input as input,
        };
        use std::time::{Duration, Instant};
        let words = match wheel(keyboard(27, 0x85).unwrap(), true).unwrap() {
            Action::Macro(words) => words,
            _ => panic!("expected a combined wheel macro"),
        };
        let mut plan = host_program::prepare(&crate::gestures::Action::default()).unwrap();
        plan.press = words.into();
        let mut devices = Devices::for_host_actions(&[plan.clone()]).unwrap();
        let [(_pointer, _), (mut keyboard, keyboard_sys)] = devices.fixture_sources().unwrap();
        keyboard.grab().unwrap();
        let mut vm = Vm::new(plan, Instant::now(), false);
        let deadline = Instant::now() + Duration::from_secs(2);
        while !vm.finished() {
            assert!(Instant::now() < deadline, "wheel macro did not finish");
            for output in vm.tick(Instant::now(), [0; 3]).unwrap() {
                devices.output(output).unwrap();
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        devices.clear_generated().unwrap();
        let events = keyboard.read().unwrap();
        let keys: Vec<_> = events
            .iter()
            .filter(|event| event.kind == input::KEY)
            .map(|event| (event.code, event.value))
            .collect();
        assert_eq!(
            keys,
            [
                (29, 1),
                (56, 1),
                (126, 1),
                (45, 1),
                (45, 0),
                (126, 0),
                (56, 0),
                (29, 0)
            ]
        );
        let key_down = events
            .iter()
            .find(|event| event.kind == input::KEY && event.code == 45 && event.value == 1)
            .unwrap();
        let key_up = events
            .iter()
            .find(|event| event.kind == input::KEY && event.code == 45 && event.value == 0)
            .unwrap();
        assert!(key_up.timestamp_micros() - key_down.timestamp_micros() >= 16000);
        assert!(keyboard.held().unwrap().is_empty());
        drop(devices);
        assert!(
            !std::path::Path::new("/sys/devices/virtual/input")
                .join(keyboard_sys)
                .exists()
        );
    }

    #[test]
    fn keyboard_combinations_follow_original_modifier_order_and_wheel_delay() {
        let binding = keyboard(27, 0x85).unwrap();
        assert_eq!(
            binding.press,
            Action::Macro(vec![0x2301, 0x2304, 0x2380, 0x211b, END])
        );
        assert_eq!(
            binding.release,
            Action::Macro(vec![0x201b, 0x2280, 0x2204, 0x2201, END])
        );
        assert_eq!(
            wheel(binding, true).unwrap(),
            Action::Macro(vec![
                0x2301, 0x2304, 0x2380, 0x211b, 0x2810, 0x201b, 0x2280, 0x2204, 0x2201, END,
            ])
        );
        let ctrl_x = keyboard(27, 1).unwrap();
        assert_eq!(keyboard_value(&ctrl_x), Some((27, 1)));
        assert_eq!(
            wheel_keyboard_value(&wheel(ctrl_x.clone(), true).unwrap()),
            Some((27, 1))
        );
        let mut incomplete = ctrl_x;
        incomplete.release = Action::Direct(NO_OPERATION);
        assert_eq!(keyboard_value(&incomplete), None);
        assert_eq!(keyboard(4, 0).unwrap().press, Action::Direct(0x2104));
        assert_eq!(
            keyboard(0xe0, 1).unwrap(),
            Binding {
                press: Action::Direct(0x2301),
                release: Action::Direct(0x2201),
            }
        );
        assert_eq!(
            wheel(keyboard(0, 3).unwrap(), true).unwrap(),
            Action::Macro(vec![0x2301, 0x2302, 0x2810, 0x2202, 0x2201, END])
        );
        assert!(keyboard(0, 0).is_err());
        assert!(keyboard(256, 0).is_err());
    }

    #[test]
    fn click_presets_have_exact_native_timing_and_no_release_macro() {
        let double = clicks(2).unwrap();
        assert_eq!(
            double.press,
            Action::Macro(vec![
                0x2501, 0x2840, 0x2401, 0x2840, 0x2501, 0x2840, 0x2401, END,
            ])
        );
        assert_eq!(double.release, Action::Direct(NO_OPERATION));
        assert_eq!(wheel(double.clone(), false).unwrap(), double.press);
        assert_eq!(
            clicks(3).unwrap().press,
            Action::Macro(vec![
                0x2501, 0x2840, 0x2401, 0x2840, 0x2501, 0x2840, 0x2401, 0x2840, 0x2501, 0x2840,
                0x2401, END,
            ])
        );
        assert!(clicks(1).is_err());
        assert!(clicks(4).is_err());
    }
}
