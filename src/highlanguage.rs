//! HighLanguage text, translated to ordinary M text. EXE 006134b4 with
//! 006144a8 (line preparation), 00615f18 (if), 00616474 (ifkey), 006168f0
//! (repeat), 00614ad8..00615aac (statements), 00617438 and 006177e4 (flags).
//!
//! One deliberate difference: the EXE replaces any other statement with an
//! empty line. The official Sniper AMC files mix plain M lines such as
//! `RightDown 1` into HighLanguage text, so such lines pass through here.

/// USB keyboard usage names, from the EXE table at 0159e8dc. Lookup ignores case.
#[rustfmt::skip]
const KEYS: [(u16, &str); 118] = [
    (4, "A"), (5, "B"), (6, "C"), (7, "D"), (8, "E"), (9, "F"), (10, "G"), (11, "H"),
    (12, "I"), (13, "J"), (14, "K"), (15, "L"), (16, "M"), (17, "N"), (18, "O"), (19, "P"),
    (20, "Q"), (21, "R"), (22, "S"), (23, "T"), (24, "U"), (25, "V"), (26, "W"), (27, "X"),
    (28, "Y"), (29, "Z"), (30, "1"), (31, "2"), (32, "3"), (33, "4"), (34, "5"), (35, "6"),
    (36, "7"), (37, "8"), (38, "9"), (39, "0"), (40, "Enter"), (41, "Esc"), (42, "Back"),
    (43, "Tab"), (44, "Space"), (45, "-"), (46, "="), (47, "["), (48, "]"), (49, "\\"),
    (51, ";"), (52, "'"), (53, "`"), (54, ","), (55, "."), (56, "/"), (57, "CapsLock"),
    (58, "F1"), (59, "F2"), (60, "F3"), (61, "F4"), (62, "F5"), (63, "F6"), (64, "F7"),
    (65, "F8"), (66, "F9"), (67, "F10"), (68, "F11"), (69, "F12"), (70, "PrintScreen"),
    (71, "ScrollLock"), (72, "Pause"), (73, "Insert"), (74, "Home"), (75, "PageUp"),
    (76, "Delete"), (77, "End"), (78, "PageDown"), (79, "RightArrow"), (80, "LeftArrow"),
    (81, "DownArrow"), (82, "UpArrow"), (83, "NumLock"), (84, "NUM_/"), (85, "NUM_*"),
    (86, "NUM_-"), (87, "NUM_+"), (88, "NUM_Enter"), (89, "NUM_1"), (90, "NUM_2"),
    (91, "NUM_3"), (92, "NUM_4"), (93, "NUM_5"), (94, "NUM_6"), (95, "NUM_7"), (96, "NUM_8"),
    (97, "NUM_9"), (98, "NUM_0"), (99, "NUM_."), (101, "Menu"), (102, "SYSTEM_Power"),
    (116, "CONSUMER_Execute"), (117, "CONSUMER_Help"), (119, "CONSUMER_Select"),
    (120, "CONSUMER_Stop"), (121, "CONSUMER_Again"), (122, "CONSUMER_Undo"),
    (123, "CONSUMER_Cut"), (124, "CONSUMER_Copy"), (125, "CONSUMER_Paste"),
    (126, "CONSUMER_Find"), (127, "CONSUMER_Mute"), (128, "CONSUMER_VolumeUp"),
    (129, "CONSUMER_VolumeDown"), (224, "Ctrl"), (225, "Shift"), (226, "Alt"),
    (227, "Window"), (228, "RCtrl"), (229, "RShift"), (230, "RAlt"), (231, "RWindow"),
];

pub fn key(name: &str) -> Option<u16> {
    KEYS.iter()
        .find(|(_, key)| key.eq_ignore_ascii_case(name))
        .map(|(usage, _)| *usage)
}

/// Every named USB keyboard usage, in table order.
pub fn keys() -> impl Iterator<Item = (u16, &'static str)> {
    KEYS.iter().copied()
}

pub fn key_name(usage: u16) -> Option<&'static str> {
    KEYS.iter()
        .find(|(key, _)| *key == usage)
        .map(|(_, name)| *name)
}

fn tokens(line: &str) -> Vec<&str> {
    line.split_whitespace().collect()
}

// 006144a8: lower case, drop comments, split at " , " and move a trailing
// "{" of if, ifkey and repeat onto its own line.
fn prepare(source: &str) -> Vec<String> {
    let mut lines = Vec::new();
    for line in source.trim().to_lowercase().lines() {
        let line = line.trim();
        let first = tokens(line).first().copied().unwrap_or("");
        if first == "comment" {
            continue;
        }
        let split = line
            .find(" , , ")
            .map(|at| (at, 5))
            .or_else(|| line.find(" , ").map(|at| (at, 3)));
        if let Some((at, width)) = split {
            lines.push(line[..at].trim().to_owned());
            lines.push(line[at + width..].trim().to_owned());
        } else if matches!(first, "if" | "ifkey" | "repeat") && line.contains('{') {
            lines.push(line.replacen('{', "", 1).trim().to_owned());
            lines.push("{".to_owned());
        } else {
            lines.push(line.to_owned());
        }
    }
    lines
}

// 006175b0: the "}" that closes the "{" at `open`.
fn close(lines: &[String], open: usize) -> Result<usize, String> {
    if lines.get(open).map(String::as_str) != Some("{") {
        return Err(format!("第 {} 行后缺少 {{", open));
    }
    let mut depth = 0;
    for (index, line) in lines.iter().enumerate().skip(open + 1) {
        match line.as_str() {
            "{" => depth += 1,
            "}" if depth == 0 => return Ok(index),
            "}" => depth -= 1,
            _ => {}
        }
    }
    Err(format!("第 {} 行的 {{ 没有对应的 }}", open + 1))
}

fn flag(lines: &[String], name: &str) -> Result<usize, String> {
    lines
        .iter()
        .position(|line| matches!(tokens(line)[..], ["flag", label, ..] if label == name))
        .ok_or_else(|| format!("找不到标记 {name}"))
}

/// Translate HighLanguage into M text for `macros::compile`.
pub fn translate(source: &str) -> Result<String, String> {
    let lines = prepare(source);
    let mut out = vec![String::new(); lines.len()];
    let (mut ifs, mut keys, mut repeats, mut registers) = (0, 0, 0, 0);
    for index in 0..lines.len() {
        let t = tokens(&lines[index]);
        let error = |text: &str| format!("第 {} 行：{text}", index + 1);
        match t[..] {
            ["if", left, operator, right, ..] => {
                let negated = match operator {
                    "==" => "!=",
                    "!=" => "==",
                    _ => return Err(error("条件只支持 == 和 !=")),
                };
                let label = format!("if_end{ifs}");
                ifs += 1;
                out[index] = format!("if {left} {negated} {right} gotoflag {label}");
                let end = close(&lines, index + 1)?;
                out[index + 1] = "null".into();
                out[end] = format!("flag {label}");
            }
            ["ifkey", key, state, ..] => {
                let key: u16 = key
                    .parse()
                    .ok()
                    .filter(|key| (1..=24).contains(key))
                    .ok_or_else(|| error("ifkey 的按键应为 1 至 24"))?;
                let label = format!("ifkey_end{keys}");
                keys += 1;
                out[index] = format!(
                    "core_var {}\nvar varh = varh & {}",
                    (key - 1) / 8,
                    1 << ((key - 1) % 8)
                );
                let end = close(&lines, index + 1)?;
                let skip = if state == "press" { "==" } else { "!=" };
                out[index + 1] = format!("if varh {skip} 0 gotoflag {label}");
                out[end] = format!("flag {label}");
            }
            ["repeat", count, ..] => {
                let register = ["vare", "varf", "varg"][registers];
                registers = (registers + 1) % 3;
                let label = format!("repeat_begin{repeats}");
                repeats += 1;
                out[index] = format!("var {register} = 0");
                let end = close(&lines, index + 1)?;
                out[index + 1] = format!("flag {label}");
                out[end] = format!(
                    "var {register} = {register} + 1\nif {register} != {count} gotoflag {label}"
                );
            }
            ["keyboard", direction, name, ..] => {
                let usage = key(name).ok_or_else(|| error(&format!("未知按键名称 {name}")))?;
                let action = if direction == "down" {
                    "keydown"
                } else {
                    "keyup"
                };
                out[index] = format!("{action} {usage}");
            }
            ["mousebutton", direction, button, ..] => {
                let stem = match button {
                    "left" | "right" | "middle" => button,
                    "key4" => "button4",
                    "key5" => "button5",
                    _ => return Err(error("未知鼠标按键")),
                };
                let action = if direction == "down" { "down" } else { "up" };
                out[index] = format!("{stem}{action}");
            }
            ["mousewheel", direction, count, ..] => {
                let word = match direction {
                    "forward" => "wheelup",
                    "backward" => "wheeldown",
                    _ => return Err(error("滚轮方向应为 forward 或 backward")),
                };
                let count: usize = count.parse().map_err(|_| error("滚轮次数无效"))?;
                out[index] = vec![word; count].join("\n");
            }
            ["mousemove", mode, x, y, ..] => {
                let command = match mode {
                    "absolute" => "moveto",
                    "relative" => "mover",
                    _ => return Err(error("移动方式应为 absolute 或 relative")),
                };
                out[index] = format!("{command} {x} {y}");
            }
            ["delay", value, unit, ..] => {
                let command = match unit {
                    "ms" => "delay",
                    "s" => "delays",
                    "min" => "delaym",
                    _ => return Err(error("延迟单位应为 ms、s 或 min")),
                };
                out[index] = format!("{command} {value}");
            }
            ["math", ..] => out[index] = format!("var {}", t[1..t.len().min(6)].join(" ")),
            ["flag", ..] => out[index] = lines[index].clone(),
            ["gotoflag", name, ..] => {
                let near = flag(&lines, name).is_ok_and(|at| at.abs_diff(index) < 0x400);
                out[index] = if near {
                    lines[index].clone()
                } else {
                    format!("lgotoflag {name}")
                };
            }
            ["{"] | ["}"] => {}
            _ => out[index] = lines[index].clone(),
        }
    }
    // Statements may expand to several lines. Flags resolve on final lines.
    let mut lines: Vec<String> = out.join("\n").lines().map(str::to_owned).collect();
    for index in 0..lines.len() {
        let t = tokens(&lines[index]);
        let resolved = match t[..] {
            ["if", left, operator, right, "gotoflag", name, ..] => {
                let offset = flag(&lines, name)? as i64 - index as i64;
                Some(format!("if {left} {operator} {right} rgoto_24bit {offset}"))
            }
            [command @ ("gotoflag" | "lgotoflag"), name, ..] => {
                let offset = flag(&lines, name)? as i64 - index as i64;
                let jump = if command == "gotoflag" {
                    "rgoto"
                } else {
                    "rgoto_24bit_highlanguage"
                };
                Some(format!("{jump} {offset}"))
            }
            _ => None,
        };
        if let Some(line) = resolved {
            lines[index] = line;
        }
    }
    for line in &mut lines {
        if tokens(line).first() == Some(&"flag") {
            *line = "null".into();
        }
    }
    Ok(lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::macros::{Repeat, compile};

    #[test]
    fn statements() {
        assert_eq!(
            translate("Keyboard down A\nkeyboard up shift\nMouseButton down key4\nmousewheel backward 2\nmousemove absolute 10 20\nmousemove relative -1 2\ndelay 3 s\nmath VarA = VarB + 1").unwrap(),
            "keydown 4\nkeyup 225\nbutton4down\nwheeldown\nwheeldown\nmoveto 10 20\nmover -1 2\ndelays 3\nvar vara = varb + 1"
        );
        assert_eq!(key("num_enter"), Some(88));
        assert_eq!(key_name(0xe3), Some("Window"));
    }

    #[test]
    fn blocks_resolve_to_relative_jumps() {
        let text =
            translate("repeat 3 {\n  if VarA == 1 {\n    keyboard down a\n  }\n}\ncomment ignored")
                .unwrap();
        assert_eq!(
            text,
            "var vare = 0\nnull\nif vara != 1 rgoto_24bit 3\nnull\nkeydown 4\nnull\nvar vare = vare + 1\nif vare != 3 rgoto_24bit -6"
        );
        let words = compile(&text, "", Repeat::Once).unwrap().press;
        assert_eq!(
            words,
            [
                0x8004,
                0,
                0x8700,
                0x0100,
                4,
                0x2104,
                0x8104,
                0x0401,
                0x8704,
                0x03ff,
                0xfffa,
                crate::storage::END
            ]
        );
        let text = translate("ifkey 3 press {\nLeftDown 1\n}\nflag a\ngotoflag a").unwrap();
        assert_eq!(
            text,
            "core_var 0\nvar varh = varh & 4\nif varh == 0 rgoto_24bit 2\nleftdown 1\nnull\nnull\nrgoto -1"
        );
        assert!(compile(&text, "", Repeat::Once).is_ok());
    }

    #[test]
    fn official_sniper_files_and_errors() {
        assert_eq!(
            translate("RightDown 1\r\ndpi accuratein").unwrap(),
            "rightdown 1\ndpi accuratein"
        );
        assert_eq!(translate("a , b").unwrap(), "a\nb");
        for source in [
            "if VarA > 1 {\n}",
            "repeat 2 {",
            "gotoflag missing",
            "keyboard down nokey",
            "ifkey 25 press {\n}",
            "delay 1 h",
        ] {
            assert!(translate(source).is_err(), "{source}");
        }
    }
}
