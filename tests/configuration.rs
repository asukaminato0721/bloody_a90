use bloody_a90::{
    backup::Backup,
    configuration::{self, Action, Binding, Configuration, Profile},
    storage::{END, NO_OPERATION, Termination},
};

const FOUR_CLICKS: [u16; 16] = [
    0x2501, 0x2819, 0x2401, 0x2855, 0x2501, 0x2819, 0x2401, 0x2855, 0x2501, 0x2819, 0x2401, 0x2855,
    0x2501, 0x2819, 0x2401, END,
];
const HELD: [u16; 5] = [0x2104, 0x2819, 0x2004, 0x1ffd, END];
// END in an operand must remain data.
const MOTION: [u16; 4] = [0x2600, END, 1, END];

fn direct(word: u16) -> Action {
    Action::Direct(word)
}

fn fixture() -> Configuration {
    let mut buttons: [Binding; 24] = Default::default();
    for (index, binding) in buttons[..5].iter_mut().enumerate() {
        binding.press = direct(0x2500 | 1 << index);
        binding.release = direct(0x2400 | 1 << index);
    }
    buttons[0].press = Action::Macro(FOUR_CLICKS.to_vec());
    buttons[0].release = direct(NO_OPERATION);
    buttons[2].press = Action::Macro(FOUR_CLICKS[4..].to_vec());
    buttons[2].release = direct(NO_OPERATION);
    let first = Profile {
        buttons,
        wheel: [direct(0x262c), direct(0x262b)],
        gestures: None,
        metadata: [0x01ff, 0xff00],
    };
    let second = second_profile(first.clone());
    let mut first = first;
    first.buttons[23].press = direct(0x2613);
    let mut auxiliary: [Action; 16] = std::array::from_fn(|_| direct(NO_OPERATION));
    auxiliary[1] = Action::Macro(FOUR_CLICKS[8..].to_vec());
    Configuration {
        profiles: vec![first, second],
        selected: 1,
        auxiliary,
        parameters: [50, 50, 50, 50, 50, 0x0101],
        flags: 0x10,
        header_word6: 1,
        lighting: Vec::new(),
    }
}

fn second_profile(mut second: Profile) -> Profile {
    second.buttons[1] = Binding {
        press: direct(0x2104),
        release: direct(0x2004),
    };
    second.buttons[3] = Binding {
        press: Action::Macro(HELD.to_vec()),
        release: direct(0x262d),
    };
    second.buttons[23].press = direct(0x2616);
    let mut gestures: Box<[Binding; 162]> = Box::new(std::array::from_fn(|_| Binding::default()));
    gestures[161].press = Action::Macro(MOTION.to_vec());
    second.gestures = Some(gestures);
    second
}

// Explicit expected layout from the prior Python check, with literal references.
fn golden() -> Vec<u16> {
    let mut words = vec![
        0xa4a4, 0xffff, 0xa1d0, 0xa785, 0x0101, 0x10, 1, 163, 169, 819, 63, 41, 0xffff, 0, 0, 0,
    ];
    words.extend(FOUR_CLICKS);
    words.extend(HELD);
    words.extend(MOTION);
    words.extend([NO_OPERATION, 0xc018]);
    words.extend([NO_OPERATION; 14]);
    words.extend([50, 50, 50, 50, 50, 0x0101]);
    words.extend([0x01ff, 0xff00, 0x01ff, 0xff00]);
    for (second, last) in [(false, 0x2613), (true, 0x2616)] {
        words.extend([0xc010, NO_OPERATION]);
        words.extend(if second {
            [0x2104, 0x2004]
        } else {
            [0x2502, 0x2402]
        });
        words.extend([0xc014, NO_OPERATION]);
        words.extend(if second {
            [0xc020, 0x262d]
        } else {
            [0x2508, 0x2408]
        });
        words.extend([0x2510, 0x2410]);
        words.extend([NO_OPERATION; 36]);
        words.extend([last, NO_OPERATION]);
    }
    words.extend([67, 115, 0x262c, 0x262b, 0x262c, 0x262b, 165, 167]);
    words.extend([NO_OPERATION; 324 + 322]);
    words.extend([0xc025, NO_OPERATION, 171, 495]);
    words
}

#[test]
fn golden_main_vector() {
    let words = configuration::build(&fixture()).unwrap();
    assert_eq!(words.len(), 821);
    assert_eq!(words, golden());
}

#[test]
fn backup_parser_resolves_new_allocations() {
    let mut image = vec![0xff; 32768];
    image[0x1804 * 2..0x1806 * 2].copy_from_slice(&[0xfd, 0x04, 7, 0]);
    for (index, word) in golden().iter().enumerate() {
        image[(0x2600 + index) * 2..][..2].copy_from_slice(&word.to_le_bytes());
    }
    let buttons = Backup::parse(&image).unwrap().buttons().unwrap();
    assert!(!buttons.factory);
    assert_eq!(buttons.selected, 1);
    let [first, second] = &buttons.profiles[..] else {
        panic!("two profiles");
    };
    assert_eq!((first.buttons_base, second.buttons_base), (0x2643, 0x2673));
    assert_eq!(first.buttons[1], [0x2502, 0x2402]);
    assert_eq!(second.buttons[1], [0x2104, 0x2004]);
    assert_eq!(first.wheel, [0x262c, 0x262b]);
    assert_eq!(first.wheel, second.wheel);
    assert_eq!(
        (first.buttons[23][0], second.buttons[23][0]),
        (0x2613, 0x2616)
    );
    for (reference, length) in [(0xc010, 16), (0xc014, 12), (0xc018, 8), (0xc020, 5)] {
        let (_, listing) = &buttons.macros[&reference];
        assert_eq!(listing.termination, Termination::End);
        assert_eq!(listing.instructions.len(), length);
    }
    let (start, held) = &buttons.macros[&0xc020];
    assert_eq!(*start, 0x2620);
    assert_eq!(held.instructions[3].target, Some(0));
    assert_eq!(held.instructions[3].text, "相对跳转 -3");
}

#[test]
fn keyboard_combinations_and_click_presets_survive_saved_table_round_trip() {
    use bloody_a90::button_actions;
    let mut config = fixture();
    config.profiles[0].buttons[3] = button_actions::keyboard(27, 0x85).unwrap();
    config.profiles[1].buttons[8] = button_actions::clicks(3).unwrap();
    config.profiles[0].wheel[0] =
        button_actions::wheel(button_actions::keyboard(6, 1).unwrap(), true).unwrap();
    config.profiles[1].wheel[1] =
        button_actions::wheel(button_actions::clicks(2).unwrap(), false).unwrap();
    let words = configuration::build(&config).unwrap();
    let mut image = vec![0xff; 32768];
    image[0x1804 * 2..0x1806 * 2].copy_from_slice(&[0xfd, 4, 7, 0]);
    for (index, word) in words.iter().enumerate() {
        image[(0x2600 + index) * 2..][..2].copy_from_slice(&word.to_le_bytes());
    }
    let restored = Backup::parse(&image).unwrap().configuration().unwrap();
    assert_eq!(
        restored.profiles[0].buttons[3],
        config.profiles[0].buttons[3]
    );
    assert_eq!(
        restored.profiles[1].buttons[8],
        config.profiles[1].buttons[8]
    );
    assert_eq!(restored.profiles[0].wheel, config.profiles[0].wheel);
    assert_eq!(restored.profiles[1].wheel, config.profiles[1].wheel);
    assert_eq!(configuration::build(&restored).unwrap(), words);
}

#[test]
fn response_and_shooting_survive_flash_round_trip() {
    use bloody_a90::shooting::{self, Cycle, Shot};
    let mut config = fixture();
    config.set_debounce_ms(16).unwrap();
    let shots = [Shot {
        interval_ms: 41,
        x: 10,
        y: -20,
    }];
    let program = shooting::cycle(Cycle::B, &shots, 0, true, true).unwrap();
    let binding = Binding {
        press: Action::Macro(program.press),
        release: Action::Macro(program.release),
    };
    config.set_shooting(7, binding.clone()).unwrap();
    config
        .set_shooting_parameters(
            2,
            Some(Shot {
                interval_ms: 999,
                x: 0,
                y: -126,
            }),
        )
        .unwrap();
    let words = configuration::build(&config).unwrap();
    let mut image = vec![0xff; 32768];
    image[0x1804 * 2..0x1806 * 2].copy_from_slice(&[0xfd, 0x04, 7, 0]);
    for (index, word) in words.iter().enumerate() {
        image[(0x2600 + index) * 2..][..2].copy_from_slice(&word.to_le_bytes());
    }
    let parsed = Backup::parse(&image).unwrap().configuration().unwrap();
    assert_eq!(parsed.debounce_ms(), Some(16));
    assert_eq!(parsed.auxiliary[7], binding.press);
    assert_eq!(parsed.auxiliary[15], binding.release);
    assert_eq!(parsed.profiles[0].buttons, config.profiles[0].buttons);
    assert_eq!(parsed.parameters, config.parameters);
    assert_eq!(parsed.parameters[2], 0x85e7);
    assert_eq!(configuration::build(&parsed).unwrap(), words);
}

#[test]
fn summary_keeps_button_tables_when_sensitivity_is_invalid() {
    let mut image = vec![0xff; 32768];
    image[0x1804 * 2..0x1806 * 2].copy_from_slice(&[0xfd, 0x04, 7, 0]);
    for (index, word) in golden().iter().enumerate() {
        image[(0x2600 + index) * 2..][..2].copy_from_slice(&word.to_le_bytes());
    }
    let summary = Backup::parse(&image).unwrap().summary();
    for line in [
        "DPI 参数：备份中的 DPI 档位数量或选择值无效",
        "  记录 04：宏 c020 / 停止宏",
        "  记录 24：射击模式 7 / 空操作",
        "  02：宏 c018",
        "  2623  1ffd           相对跳转 -3",
    ] {
        assert!(summary.lines().any(|l| l == line), "{line}");
    }
}

// Run with A90_BACKUP=/path/to/backup.bin cargo test -- --ignored
#[test]
#[ignore]
fn real_backup_round_trip() {
    let path = std::env::var("A90_BACKUP").expect("A90_BACKUP");
    let backup = Backup::read(path).unwrap();
    let config = backup.configuration().unwrap();
    let words = configuration::build(&config).unwrap();
    let mut image = std::fs::read(std::env::var("A90_BACKUP").unwrap()).unwrap();
    for (index, word) in words.iter().enumerate() {
        image[(0x2600 + index) * 2..][..2].copy_from_slice(&word.to_le_bytes());
    }
    let rebuilt = Backup::parse(&image).unwrap();
    let again = rebuilt.configuration().unwrap();
    assert_eq!(configuration::build(&again).unwrap(), words);
    let (old, new) = (backup.buttons().unwrap(), rebuilt.buttons().unwrap());
    for (a, b) in old.profiles.iter().zip(&new.profiles) {
        for (x, y) in a.buttons.iter().zip(&b.buttons) {
            for (p, q) in x.iter().zip(y) {
                let describe = |w: u16, buttons: &bloody_a90::backup::Buttons| {
                    if w >= 0xc000 {
                        let (_, listing) = &buttons.macros[&w];
                        listing
                            .instructions
                            .iter()
                            .map(|i| i.text.clone())
                            .collect::<Vec<_>>()
                            .join(";")
                    } else {
                        format!("{w:04x}")
                    }
                };
                assert_eq!(describe(*p, &old), describe(*q, &new));
            }
        }
    }
    println!("{} words, {}", words.len(), rebuilt.summary());
}
