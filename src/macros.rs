//! Ordinary M source. EXE 006237f4, 0062200c, 0060c510, 0060ca14.
//! HighLanguage translation is in highlanguage.rs. The firmware executes macros.

use crate::storage::{self, END};

const LIMIT: usize = 16384;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Repeat {
    Once,
    Held,
    Toggle,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Program {
    pub press: Vec<u16>,
    pub release: Vec<u16>,
}

pub fn hex(words: &[u16]) -> String {
    words
        .iter()
        .map(|word| format!("{word:04x}"))
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn bytes(words: &[u16]) -> Vec<u8> {
    words.iter().flat_map(|word| word.to_le_bytes()).collect()
}

fn integer(text: &str, min: i32, max: i32) -> Result<i32, String> {
    let value = text
        .parse::<i32>()
        .map_err(|_| "需要十进制整数".to_string())?;
    if value < min || value > max {
        return Err(format!("整数范围应为 {min} 至 {max}"));
    }
    Ok(value)
}

fn register(text: &str) -> Result<u16, String> {
    let raw = text.as_bytes();
    if raw.len() == 4 && &raw[..3] == b"var" && raw[3].is_ascii_lowercase() {
        Ok((raw[3] - b'a') as u16)
    } else {
        Err("变量名应为 VarA 至 VarZ".into())
    }
}

fn operand(text: &str) -> Result<(bool, u16), String> {
    if text.starts_with("var") {
        Ok((true, register(text)?))
    } else {
        Ok((false, integer(text, 0, 255)? as u16))
    }
}

fn key(text: &str, down: bool) -> Result<u16, String> {
    let mut usage = integer(text, 1, 271)? as u16;
    if (264..=271).contains(&usage) {
        usage -= 40;
    }
    if usage > 255 {
        return Err("不支持此键盘 usage".into());
    }
    Ok(if (0xe0..=0xe7).contains(&usage) {
        (if down { 0x2300 } else { 0x2200 }) | (1 << (usage - 0xe0))
    } else {
        (if down { 0x2100 } else { 0x2000 }) | usage
    })
}

struct Line {
    words: Vec<u16>,
    target: Option<i32>,
    conditional: bool,
    /// Word offset of the branch inside the expanded line. Displacements
    /// count from the branch instruction itself.
    branch: usize,
}

// Expanded text for one source line can hold several statements. These
// helpers build the words of the internal statements the EXE emits.
fn condition(left: u16, right: u16, register: bool, equal: bool) -> [u16; 3] {
    let opcode = (if register { 0x8e00 } else { 0x8600 }) + if equal { 0 } else { 0x100 };
    [opcode | left, right << 8, 0]
}

fn source_target(command: &str, value: &str, index: usize) -> Result<i32, String> {
    let value = integer(value, -(LIMIT as i32), LIMIT as i32)?;
    Ok(match command {
        "goto" => value - 1,
        _ => index as i32 + value,
    })
}

fn line(tokens: &[&str], index: usize, loops: &mut u8) -> Result<Line, String> {
    let mut words = Vec::new();
    let mut target = None;
    let mut conditional = false;
    let mut branch = 0;
    // EXE 006289a0 accepts "VarA = ..." without the var keyword.
    let prefixed;
    let tokens = match tokens {
        [first, "=", ..] if first.len() == 4 && first.starts_with("var") => {
            prefixed = [&["var"], tokens].concat();
            &prefixed[..]
        }
        _ => tokens,
    };
    match tokens {
        [] | ["null"] => {}
        // EXE 006237f4 reads only the first argument. Official files append
        // a legacy count, as in "KeyDownUp 32 1".
        [command @ ("keydown" | "keyup" | "keydownup"), usage, ..] => {
            let down = key(usage, true)?;
            let up = key(usage, false)?;
            words.extend(match *command {
                "keydown" => vec![down],
                "keyup" => vec![up],
                _ => vec![down, 0x2810, up],
            });
        }
        [
            command @ ("modifierkeydown" | "modifierkeyup" | "consumerkeydown" | "consumerkeyup"
            | "systemkeydown" | "systemkeyup"),
            value,
            ..,
        ] => {
            let (base, limit) = match *command {
                "modifierkeydown" => (0x2300, 255),
                "modifierkeyup" => (0x2200, 255),
                "consumerkeydown" => (0x3400, 1023),
                "consumerkeyup" => (0x3000, 1023),
                "systemkeydown" => (0x3900, 255),
                _ => (0x3800, 255),
            };
            words.push(base | integer(value, 1, limit)? as u16);
        }
        [command @ ("delay" | "delays" | "delaym"), value] => {
            let multiplier = match *command {
                "delays" => 1000,
                "delaym" => 60000,
                _ => 1,
            };
            let duration = integer(value, 0, i32::MAX / multiplier)? * multiplier;
            let mut coarse = duration / 1024;
            while coarse > 255 {
                words.push(0x27ff);
                coarse -= 255;
            }
            if coarse > 0 {
                words.push(0x2700 | coarse as u16);
            }
            let remainder = (duration % 1024).min(999);
            if remainder > 0 {
                words.push(0x2800 + remainder as u16);
            }
        }
        ["mover", x, y] => {
            let x = integer(x, -32768, 32767)?;
            let y = integer(y, -32768, 32767)?;
            if x.abs() < 63 && y.abs() < 63 {
                if x != 0 || y != 0 {
                    words.push(0x4000 | ((x as u16 & 127) << 7) | (y as u16 & 127));
                }
            } else if x.abs() < 2047 && y.abs() < 2047 {
                words.extend([
                    0x3b00 | (((x as u16 & 4095) >> 8) << 4) | ((y as u16 & 4095) >> 8),
                    ((x as u16 & 255) << 8) | (y as u16 & 255),
                ]);
            } else {
                words.extend([0x2600, x as u16, y as u16]);
            }
        }
        ["moveto", x, y] => {
            // EXE 006237f4: origin, 48 ms, then a relative move without the
            // 7-bit format. A second origin for screens above 4096 pixels is
            // not emitted.
            let x = integer(x, -32768, 32767)?;
            let y = integer(y, -32768, 32767)?;
            words.extend([0x2620, 0x2830]);
            if x.abs() < 0x7ff && y.abs() < 0x7ff {
                words.extend([
                    0x3b00 | (((x as u16 & 4095) >> 8) << 4) | ((y as u16 & 4095) >> 8),
                    ((x as u16 & 255) << 8) | (y as u16 & 255),
                ]);
            } else {
                words.extend([0x2600, x as u16, y as u16]);
            }
        }
        ["gowhile", start, count] => {
            // Count with VarE, VarF and VarG in turn, so loops nest three deep.
            let counter = 4 + *loops as u16;
            *loops = (*loops + 1) % 3;
            words.extend([0x8100 | counter, counter << 8 | 1]);
            words.extend(condition(
                counter,
                integer(count, 0, 255)? as u16,
                false,
                false,
            ));
            words.extend([0x8000 | counter, 0]);
            target = Some(source_target("goto", start, index)?);
            conditional = true;
            branch = 2;
        }
        ["ifkey", key, state, value] => {
            // core_var selects the state byte of buttons 1-8, 9-16 or 17-24.
            let key = integer(key, 1, 24)? as u16 - 1;
            words.extend([0x9007, key / 8, 0x8307, 0x0700 | 1 << (key % 8)]);
            let pressed = integer(state, i32::MIN, i32::MAX)? != 0;
            words.extend(condition(7, 0, false, !pressed));
            target = Some(source_target("goto", value, index)?);
            conditional = true;
            branch = 4;
        }
        ["lockrightkey"] => words.push(0x260d),
        ["toggleonoffjump"] => words.push(0x2621),
        ["key7functiontoggle"] => words.push(0x261c),
        ["var", destination, "=", value] => {
            let (is_register, value) = operand(value)?;
            words.extend([
                (if is_register { 0x8800 } else { 0x8000 }) | register(destination)?,
                if is_register { value << 8 } else { value },
            ]);
        }
        ["var", destination, "=", left, operation, right] => {
            let left = register(left)?;
            let (is_register, right) = operand(right)?;
            let operation = match *operation {
                "+" => 1,
                "-" => 2,
                "&" => 3,
                "|" => 4,
                "^" => 5,
                _ => return Err("不支持此变量运算".into()),
            };
            let base = if is_register { 0x8800 } else { 0x8000 };
            words.extend([
                (base + (operation << 8)) | register(destination)?,
                left << 8 | right,
            ]);
        }
        ["core_var", value] => words.extend([0x9007, integer(value, 0, 2)? as u16]),
        [
            "if",
            left,
            comparison @ ("==" | "!="),
            right,
            branch @ ("goto" | "rgoto_24bit"),
            value,
        ] => {
            let (is_register, right) = operand(right)?;
            let opcode = (if is_register { 0x8e00 } else { 0x8600 })
                + if *comparison == "!=" { 0x100 } else { 0 };
            words.extend([opcode | register(left)?, right << 8, 0]);
            let value = integer(value, -(LIMIT as i32), LIMIT as i32)?;
            target = Some(if *branch == "goto" {
                value - 1
            } else {
                index as i32 + value
            });
            conditional = true;
        }
        [
            command @ ("goto" | "rgoto" | "rgoto_24bit_highlanguage"),
            value,
        ] => {
            let value = integer(value, -(LIMIT as i32), LIMIT as i32)?;
            target = Some(if *command == "goto" {
                value - 1
            } else {
                index as i32 + value
            });
            words = if *command == "rgoto_24bit_highlanguage" {
                vec![0x3a00, 0]
            } else {
                vec![0]
            };
        }
        ["dpi", action] => words.push(match *action {
            "up" => 0x2626,
            "down" => 0x2624,
            "+" => 0x2627,
            "-" => 0x2625,
            "accuratein" => 0x2628,
            "accurateout" => 0x2629,
            _ => return Err("不支持此 DPI 动作".into()),
        }),
        ["profile", action] => words.push(match *action {
            "up" => 0x2622,
            "down" => 0x2623,
            "recycle" => 0x262a,
            _ => return Err("不支持此配置动作".into()),
        }),
        ["ledchange", action] => words.push(match *action {
            "up" => 0x2618,
            "down" => 0x2619,
            "rotation" => 0x261a,
            _ => return Err("不支持此灯光动作".into()),
        }),
        ["shoot", "index", value] => words.push(0x2610 + integer(value, 1, 8)? as u16 - 1),
        ["shoot", action] => words.push(match *action {
            "exec_delay" => 0x2608,
            "enable_mover" => 0x2609,
            "move_recovery" => 0x260a,
            "paynxlimit" => 0x260c,
            _ => return Err("不支持此射击动作".into()),
        }),
        ["stop"] => words.push(0x262d),
        ["toggleaxisy"] => words.push(0x260b),
        [command] | [command, "1"] => {
            let simple = match *command {
                "wheelup" => Some(0x262b),
                "wheeldown" => Some(0x262c),
                _ => None,
            };
            if let Some(word) = simple {
                words.push(word);
            } else {
                let (stem, up_down) = if let Some(stem) = command.strip_suffix("downup") {
                    (stem, 2)
                } else if let Some(stem) = command.strip_suffix("down") {
                    (stem, 1)
                } else if let Some(stem) = command.strip_suffix("up") {
                    (stem, 0)
                } else {
                    return Err("不支持此指令或参数".into());
                };
                let mask = match stem {
                    "left" => 1,
                    "right" => 2,
                    "middle" => 4,
                    "button4" => 8,
                    "button5" => 16,
                    _ => return Err("不支持此鼠标按键".into()),
                };
                if up_down == 2 {
                    if mask > 2 {
                        return Err("组合点击只支持左键和右键".into());
                    }
                    words.extend([0x2500 | mask, 0x2810, 0x2400 | mask]);
                } else {
                    words.push((if up_down == 1 { 0x2500 } else { 0x2400 }) | mask);
                }
            }
        }
        _ => return Err("不支持此指令或参数".into()),
    }
    Ok(Line {
        words,
        target,
        conditional,
        branch,
    })
}

fn body(source: &str, loops: &mut u8) -> Result<Vec<u16>, String> {
    if source.len() > 1024 * 1024 {
        return Err("宏文本超过一 MiB".into());
    }
    let mut lines = Vec::new();
    let mut offsets = vec![0i32];
    for (index, source_line) in source.lines().enumerate() {
        if index >= LIMIT {
            return Err("宏文本超过 16384 行".into());
        }
        let lower = source_line.to_ascii_lowercase();
        let tokens: Vec<_> = lower.split_whitespace().collect();
        let line = line(&tokens, index, loops).map_err(|e| format!("第 {} 行：{e}", index + 1))?;
        let next = offsets.last().unwrap() + line.words.len() as i32;
        if next >= LIMIT as i32 {
            return Err("展开的宏超过存储上限".into());
        }
        offsets.push(next);
        lines.push(line);
    }
    let mut words = Vec::new();
    let count = lines.len();
    for (index, mut line) in lines.into_iter().enumerate() {
        if let Some(target) = line.target {
            if target < 0 || target as usize > count {
                return Err(format!("第 {} 行：跳转目标超出源代码范围", index + 1));
            }
            let at = line.branch;
            let delta = offsets[target as usize] - (offsets[index] + at as i32);
            if line.conditional {
                line.words[at + 1] |= ((delta >> 16) & 255) as u16;
                line.words[at + 2] = delta as u16;
            } else if line.words.len() == 2 {
                line.words[0] = 0x3a00 | ((delta >> 16) & 255) as u16;
                line.words[1] = delta as u16;
            } else {
                if !(-4096..=4095).contains(&delta) {
                    return Err(format!("第 {} 行：短跳转超出范围", index + 1));
                }
                line.words[0] = (delta & 8191) as u16;
            }
        }
        words.extend(line.words);
    }
    Ok(words)
}

pub fn compile(press: &str, release: &str, repeat: Repeat) -> Result<Program, String> {
    // gowhile counters rotate across the whole compile, as the EXE's global does.
    let mut loops = 0;
    let body = body(press, &mut loops)?;
    let mut press = Vec::new();
    if repeat == Repeat::Toggle {
        press.push(0x2621);
    }
    press.extend(&body);
    let mut release = match repeat {
        Repeat::Once => self::body(release, &mut loops)?,
        Repeat::Held => vec![0x262d],
        Repeat::Toggle => vec![0x262e],
    };
    if repeat != Repeat::Once {
        let delta = -(body.len() as i32);
        if press.len() <= 1000 {
            press.push((delta & 8191) as u16);
        } else {
            press.extend([0x3a00 | ((delta >> 16) & 255) as u16, delta as u16]);
        }
    }
    press.push(END);
    release.push(END);
    if press.len() > LIMIT || release.len() > LIMIT {
        return Err("完整宏超过存储上限".into());
    }
    storage::validate(&press).map_err(|e| format!("按下程序：{e}"))?;
    storage::validate(&release).map_err(|e| format!("释放程序：{e}"))?;
    Ok(Program { press, release })
}

/// One wheel step has no later release edge. Run both source sections once.
/// Each section keeps its relative branches and independent source line numbers.
pub fn compile_wheel(press: &str, release: &str) -> Result<Vec<u16>, String> {
    let Program { mut press, release } = compile(press, release, Repeat::Once)?;
    press.pop(); // Replace the final END with the release section.
    press.extend(release);
    if press.len() > LIMIT {
        return Err("滚轮宏超过存储上限".into());
    }
    storage::validate(&press)?;
    Ok(press)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wheel_program_keeps_relative_branch_targets_across_both_sections() {
        let words =
            compile_wheel("goto 3\nKeyDown 5\nKeyDown 4", "goto 3\nKeyUp 5\nKeyUp 4").unwrap();
        assert_eq!(words, [2, 0x2105, 0x2104, 2, 0x2005, 0x2004, END]);
        // Branching to the press END must enter the release section.
        assert_eq!(
            compile_wheel("goto 2", "KeyUp 4").unwrap(),
            [1, 0x2004, END]
        );
        assert_eq!(compile_wheel("", "").unwrap(), [END]);
        assert!(compile_wheel("KeyDown 4", "invalid release").is_err());
    }
    #[test]
    fn factory_four_clicks() {
        let source =
            "LeftDown\ndelay 25\nLeftUp\ndelay 85\n".repeat(3) + "LeftDown\ndelay 25\nLeftUp";
        assert_eq!(
            compile(&source, "", Repeat::Once).unwrap().press,
            [
                0x2501, 0x2819, 0x2401, 0x2855, 0x2501, 0x2819, 0x2401, 0x2855, 0x2501, 0x2819,
                0x2401, 0x2855, 0x2501, 0x2819, 0x2401, END
            ]
        );
    }
    #[test]
    fn relocation_counts_operand_words() {
        assert_eq!(
            compile(
                "goto 4\nvar VarA = 1\nmover 100 -100\nrgoto -3",
                "",
                Repeat::Once
            )
            .unwrap()
            .press,
            [5, 0x8000, 1, 0x3b0f, 0x649c, 0x1ffb, END]
        );
        assert_eq!(
            compile("if VarZ != VarA goto 1", "", Repeat::Once)
                .unwrap()
                .press,
            [0x8f19, 0, 0, END]
        );
    }
    #[test]
    fn repeat_branch_threshold_and_release() {
        for (repeat, count, long) in [
            (Repeat::Held, 1000, false),
            (Repeat::Held, 1001, true),
            (Repeat::Toggle, 999, false),
            (Repeat::Toggle, 1000, true),
        ] {
            let result = compile(
                &"delay 1\n".repeat(count),
                "invalid ignored release",
                repeat,
            )
            .unwrap();
            let branch = result.press.len() - if long { 3 } else { 2 };
            assert_eq!(
                result.press[branch],
                if long {
                    0x3aff
                } else {
                    (-(count as i32) & 8191) as u16
                }
            );
            assert_eq!(
                result.release,
                [
                    if repeat == Repeat::Held {
                        0x262d
                    } else {
                        0x262e
                    },
                    END
                ]
            );
        }
    }
    #[test]
    fn reject_unknown_and_out_of_range() {
        for source in [
            "keyboard down A",
            "dw 42",
            "macroending",
            "reportrate",
            "sensorlift",
            "goto 0",
            "rgoto -1",
            "goto 9",
            "mover 32768 0",
            "keydown 256",
            "delay -1",
            "delaym 35792",
            "var VarA = 1 + 2",
            "if VarA > 0 goto 1",
        ] {
            assert!(compile(source, "", Repeat::Once).is_err(), "{source}");
        }
    }
    #[test]
    fn delay_clamp_and_zero_width_labels() {
        assert_eq!(
            compile(
                "delay 1000\ndelay 1024\ndelay 2000\ndelays 1\ndelaym 1",
                "",
                Repeat::Once
            )
            .unwrap()
            .press,
            [0x2be7, 0x2701, 0x2701, 0x2bd0, 0x2be7, 0x273a, 0x2a60, END]
        );
        assert_eq!(
            compile("goto 4\nnull\n\nWheelUp", "", Repeat::Once)
                .unwrap()
                .press,
            [1, 0x262b, END]
        );
    }
    #[test]
    fn loops_key_state_and_absolute_moves() {
        let press = |source: &str| compile(source, "", Repeat::Once).unwrap().press;
        // The conditional branch sits two words into the gowhile expansion.
        assert_eq!(
            press("LeftDown\nLeftUp\ngowhile 1 3"),
            [
                0x2501, 0x2401, 0x8104, 0x0401, 0x8704, 0x03ff, 0xfffc, 0x8004, 0, END
            ]
        );
        let nested = press("gowhile 1 2\ngowhile 1 2\ngowhile 1 2\ngowhile 1 2");
        let counters: Vec<_> = nested.chunks(7).map(|c| c[0] & 255).take(4).collect();
        assert_eq!(counters, [4, 5, 6, 4]);
        assert_eq!(
            press("ifkey 10 1 3\nLeftDown\nnull"),
            [0x9007, 1, 0x8307, 0x0702, 0x8707, 0, 4, 0x2501, END]
        );
        assert_eq!(
            press("ifkey 24 0 1")[..6],
            [0x9007, 2, 0x8307, 0x0780, 0x8607, 0x00ff]
        );
        assert_eq!(
            press("moveto 100 -100"),
            [0x2620, 0x2830, 0x3b0f, 0x649c, END]
        );
        assert_eq!(
            press("moveto 3000 5"),
            [0x2620, 0x2830, 0x2600, 3000, 5, END]
        );
        assert_eq!(press("VarA = 3\nlockrightkey"), [0x8000, 3, 0x260d, END]);
        for source in [
            "ifkey 0 1 1",
            "ifkey 25 1 1",
            "gowhile 1 256",
            "gowhile 0 1",
        ] {
            assert!(compile(source, "", Repeat::Once).is_err(), "{source}");
        }
    }
}
