//! Macro storage checks and listings. EXE 00619b60 / 006112e0, firmware 1153.
//! Words are relative to one table base. This module has no device transport.

pub const END: u16 = 0x262f;
pub const NO_OPERATION: u16 = 0x262e;

// Consecutive no-argument words of the direct event encoder, from WHEEL_UP.
const SIMPLE: [(u16, &str); 24] = [
    (0x262b, "滚轮向上"),
    (0x262c, "滚轮向下"),
    (NO_OPERATION, "空操作"),
    (END, "结束"),
    (0x2626, "DPI 档位上移"),
    (0x2624, "DPI 档位下移"),
    (0x2627, "DPI 增加"),
    (0x2625, "DPI 减少"),
    (0x2628, "精确模式进入"),
    (0x2629, "精确模式退出"),
    (0x2622, "配置上移"),
    (0x2623, "配置下移"),
    (0x262a, "配置循环"),
    (0x2618, "灯光上移"),
    (0x2619, "灯光下移"),
    (0x261a, "灯光循环"),
    (0x260e, "回报率循环"),
    (0x260f, "抬起高度切换"),
    (0x260b, "切换 Y 轴"),
    (0x262d, "停止宏"),
    // EXE internal names. Their effects depend on shooting state.
    (0x2608, "shoot exec_delay"),
    (0x2609, "shoot enable_mover"),
    (0x260a, "shoot move_recovery"),
    (0x260c, "shoot paynxlimit"),
];
const TOGGLE: u16 = 0x2621;
// Words the EXE text compiler emits outside the direct event encoder.
// Firmware 07 handles 2620 at 1023 and 260d at 0ead; 261c returns at 0d91.
const TEXT_ONLY: [(u16, &str); 4] = [
    (TOGGLE, "开关重复跳转"),
    (0x2620, "光标回到原点"),
    (0x260d, "锁定右键"),
    (0x261c, "第七键功能切换"),
];

fn conditional(word: u16) -> bool {
    matches!(word >> 8, 0x86 | 0x87 | 0x8e | 0x8f)
}

/// Storage width of an instruction known to the offline compiler.
/// Unknown words need their own operand analysis first.
pub fn width(word: u16) -> Option<usize> {
    match word {
        0x2600 => Some(3),
        _ if matches!(word >> 8, 0x3a | 0x3b) => Some(2),
        0x8000..=0x90ff => ((word & 255) <= 25).then(|| if conditional(word) { 3 } else { 2 }),
        0..=0x25ff
        | 0x2701..=0x27ff
        | 0x2801..=0x2be7
        | 0x3000..=0x39ff
        | 0x4000..=0x7fff
        | 0x2610..=0x2617 => Some(1),
        _ => SIMPLE
            .iter()
            .chain(&TEXT_ONLY)
            .any(|(simple, _)| *simple == word)
            .then_some(1),
    }
}

/// Whether the direct event encoder can produce this word. Raw storage
/// references, jumps and multiword instructions are not direct actions.
pub fn is_direct(word: u16) -> bool {
    let low = word & 255;
    match word >> 8 {
        0x20 | 0x21 => low != 0 && !(0xe0..=0xe7).contains(&low),
        0x22 | 0x23 => low.count_ones() == 1,
        0x24 | 0x25 => matches!(low, 1 | 2 | 4 | 8 | 16),
        0x27 | 0x38 | 0x39 => low != 0,
        0x30..=0x37 => word & 0x3ff != 0,
        _ => {
            (0x2801..=0x2be7).contains(&word)
                || (0x2610..=0x2617).contains(&word)
                || SIMPLE.iter().any(|(simple, _)| *simple == word)
        }
    }
}

fn signed(value: u32, bits: u32) -> i32 {
    (value << (32 - bits)) as i32 >> (32 - bits)
}

/// Relative displacement in words, measured from the branch instruction.
/// The caller supplies the complete instruction.
fn displacement(words: &[u16]) -> Option<i32> {
    let word = words[0];
    if word < 0x2000 {
        Some(signed(word as u32, 13))
    } else if word >> 8 == 0x3a {
        Some(signed(((word as u32 & 255) << 16) | words[1] as u32, 24))
    } else if conditional(word) && (word & 255) <= 25 {
        Some(signed(
            ((words[1] as u32 & 255) << 16) | words[2] as u32,
            24,
        ))
    } else {
        None
    }
}

/// Check instruction boundaries and relative branch targets in storage order.
/// Branches are not executed and termination is not proven. One END must be
/// the final instruction. Operand words equal to END remain ordinary data.
pub fn validate(words: &[u16]) -> Result<(), String> {
    if words.is_empty() || words.len() > 0x4000 {
        return Err("宏长度应为 1 至 16384 个字".into());
    }
    let mut boundaries = vec![false; words.len()];
    let mut offset = 0;
    while offset < words.len() {
        let word = words[offset];
        let size = width(word).ok_or_else(|| format!("不支持的宏指令 {word:04x}"))?;
        if size > words.len() - offset {
            return Err(format!("宏指令 {word:04x} 缺少操作数"));
        }
        boundaries[offset] = true;
        if (word == END) != (offset + size == words.len()) {
            return Err("宏必须只在最后一条指令处结束".into());
        }
        offset += size;
    }
    offset = 0;
    while offset < words.len() {
        let size = width(words[offset]).unwrap_or(1);
        if let Some(delta) = displacement(&words[offset..offset + size]) {
            let target = offset as i64 + delta as i64;
            if target < 0 || target >= words.len() as i64 || !boundaries[target as usize] {
                return Err(format!("第 {offset} 字的跳转目标不在指令边界"));
            }
        }
        offset += size;
    }
    Ok(())
}

/// Both checksums cover word 4 to the end of a block and wrap to 16 bits.
pub fn checksums(words: &[u16]) -> (u16, u16) {
    words
        .iter()
        .enumerate()
        .skip(4)
        .fold((0u16, 0u16), |(sum, weighted), (index, word)| {
            (
                sum.wrapping_add(*word),
                weighted.wrapping_add((index as u16).wrapping_mul(*word)),
            )
        })
}

/// Validated macro storage that starts at a word offset from the table base.
/// Equal programs and suffixes at instruction boundaries share storage. This is
/// a storage optimization, not a reconstruction of the EXE allocator.
pub struct Pool {
    start: usize,
    capacity: usize,
    words: Vec<u16>,
}

impl Pool {
    pub fn new(start: usize, capacity: usize) -> Result<Self, String> {
        if !(16..=0x3fff).contains(&start) || capacity > 0x4000 - start {
            return Err("宏存储范围超出 14 位引用".into());
        }
        Ok(Self {
            start,
            capacity,
            words: Vec::new(),
        })
    }

    pub fn words(&self) -> &[u16] {
        &self.words
    }

    /// Return a `c000 | word offset` reference. Errors leave the pool unchanged.
    pub fn add(&mut self, program: &[u16]) -> Result<u16, String> {
        validate(program)?;
        let mut offset = 0;
        while offset < self.words.len() {
            if self.words[offset..].starts_with(program) {
                return Ok(0xc000 | (self.start + offset) as u16);
            }
            // Stored words passed validation, so every boundary has a width.
            offset += width(self.words[offset]).unwrap_or(1);
        }
        if program.len() > self.capacity - self.words.len() {
            return Err("宏存储空间不足".into());
        }
        let reference = 0xc000 | (self.start + self.words.len()) as u16;
        self.words.extend_from_slice(program);
        Ok(reference)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Instruction {
    pub offset: usize,
    pub words: Vec<u16>,
    pub text: String,
    /// Branch target relative to the listing start.
    pub target: Option<i64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Termination {
    End,
    Limit,
    Incomplete,
    Unparsed,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Listing {
    pub instructions: Vec<Instruction>,
    pub termination: Termination,
}

fn register(index: u16) -> String {
    format!("Var{}", (b'A' + index as u8) as char)
}

fn text(words: &[u16]) -> Option<String> {
    let word = words[0];
    let low = word & 255;
    let operand = |register_kind: bool, value: u16| {
        if register_kind {
            register(value)
        } else {
            value.to_string()
        }
    };
    Some(match word {
        0..=0x1fff | 0x3a00..=0x3aff => format!("相对跳转 {:+}", displacement(words)?),
        0x2600 | 0x3b00..=0x3bff | 0x4000..=0x7fff => {
            let (x, y) = match words.len() {
                3 => (words[1] as i16 as i32, words[2] as i16 as i32),
                2 => (
                    signed(
                        (((word as u32 >> 4) & 15) << 8) | (words[1] as u32 >> 8),
                        12,
                    ),
                    signed(((word as u32 & 15) << 8) | (words[1] as u32 & 255), 12),
                ),
                _ => (
                    signed((word as u32 >> 7) & 127, 7),
                    signed(word as u32 & 127, 7),
                ),
            };
            format!("相对位移 {x} {y}")
        }
        0x2000..=0x25ff => format!(
            "{} {low:02x}",
            [
                "键盘松开",
                "键盘按下",
                "修饰键松开",
                "修饰键按下",
                "鼠标松开",
                "鼠标按下"
            ][(word >> 8) as usize - 0x20]
        ),
        0x2701..=0x27ff => format!("延迟计数 {low} × 1024"),
        0x2801..=0x2be7 => format!("延迟 {} ms", word - 0x2800),
        0x3000..=0x37ff => format!(
            "多媒体{} {:03x}",
            if word & 0x400 != 0 {
                "按下"
            } else {
                "松开"
            },
            word & 0x3ff
        ),
        0x3800..=0x39ff => format!(
            "系统键{} {low:02x}",
            if word & 0x100 != 0 {
                "按下"
            } else {
                "松开"
            }
        ),
        0x2610..=0x2617 => format!("shoot index {}", word - 0x2610 + 1),
        0x8000..=0x90ff => {
            let destination = register(low);
            let argument = words[1];
            match word >> 8 {
                0x86 | 0x87 | 0x8e | 0x8f => format!(
                    "若 {destination} {} {} 相对跳转 {:+}",
                    if word & 0x100 != 0 { "!=" } else { "==" },
                    operand(word >= 0x8e00, argument >> 8),
                    displacement(words)?
                ),
                0x90 => format!("{destination} = 内部状态 {}", argument & 255),
                0x80 => format!("{destination} = {}", argument & 255),
                0x88 => format!("{destination} = {}", register(argument >> 8)),
                opcode => format!(
                    "{destination} = {} {} {}",
                    register(argument >> 8),
                    ["+", "-", "&", "|", "^"][(opcode as usize & 7) - 1],
                    operand(opcode >= 0x89, argument & 255)
                ),
            }
        }
        _ => SIMPLE
            .iter()
            .chain(&TEXT_ONLY)
            .find(|(simple, _)| *simple == word)?
            .1
            .into(),
    })
}

/// List instructions in storage order until END, an unknown word or the end
/// of the supplied words. Branches are not followed.
pub fn list(words: &[u16]) -> Listing {
    let mut instructions = Vec::new();
    let mut offset = 0;
    while offset < words.len() {
        let word = words[offset];
        let Some(size) = width(word) else {
            instructions.push(Instruction {
                offset,
                words: vec![word],
                text: format!("未解析 {word:04x}"),
                target: None,
            });
            return Listing {
                instructions,
                termination: Termination::Unparsed,
            };
        };
        if size > words.len() - offset {
            return Listing {
                instructions,
                termination: Termination::Incomplete,
            };
        }
        let instruction = &words[offset..offset + size];
        instructions.push(Instruction {
            offset,
            words: instruction.to_vec(),
            text: text(instruction).unwrap_or_else(|| format!("未解析 {word:04x}")),
            target: displacement(instruction).map(|delta| offset as i64 + delta as i64),
        });
        if word == END {
            return Listing {
                instructions,
                termination: Termination::End,
            };
        }
        offset += size;
    }
    Listing {
        instructions,
        termination: Termination::Limit,
    }
}

/// Describe one table entry: a direct action or a macro reference.
pub fn describe(word: u16) -> String {
    if word >= 0xc000 {
        return format!("宏 {word:04x}");
    }
    match list(&[word]).instructions.first() {
        Some(instruction) if width(word) == Some(1) => instruction.text.clone(),
        _ => format!("原始 {word:04x}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const FOUR_CLICKS: [u16; 16] = [
        0x2501, 0x2819, 0x2401, 0x2855, 0x2501, 0x2819, 0x2401, 0x2855, 0x2501, 0x2819, 0x2401,
        0x2855, 0x2501, 0x2819, 0x2401, END,
    ];

    #[test]
    fn branch_targets() {
        for valid in [
            &[2, NO_OPERATION, END][..],
            &[NO_OPERATION, 0x1fff, END],
            &[0, END],
            &[0x3a00, 3, NO_OPERATION, END],
            &[NO_OPERATION, 0x3aff, 0xffff, END],
            &[0x8619, 0x0100, 3, END],
        ] {
            assert_eq!(validate(valid), Ok(()), "{valid:04x?}");
        }
        for invalid in [
            &[0x1fff, END][..],
            &[2, END],
            // Targets inside motion and jump operands.
            &[2, 0x2600, NO_OPERATION, 1, END],
            &[0x3a00, 1, END],
            &[0x3a80, 0, END],
            &[0x3a7f, 0xffff, END],
            &[0x8700, 0, 1, END],
        ] {
            assert!(validate(invalid).is_err(), "{invalid:04x?}");
        }
    }

    #[test]
    fn boundaries_and_unknown_words() {
        // The first END value is an X operand, not an instruction.
        assert_eq!(validate(&[0x2600, END, 1, END]), Ok(()));
        for invalid in [
            &[][..],
            &[0x2601, END],
            &[0x2600, END],
            &[0x3b00],
            &[0x3a00],
            &[END, NO_OPERATION],
            &[NO_OPERATION],
            &[0x801a, 0, END],
            &[0xc010, END],
        ] {
            assert!(validate(invalid).is_err(), "{invalid:04x?}");
        }
        let shooting: Vec<u16> = (0x2610..=0x2617)
            .chain([0x2608, 0x2609, 0x260a, 0x260c, END])
            .collect();
        assert_eq!(validate(&shooting), Ok(()));
        // The last valid offset of the longest program.
        let mut longest = vec![NO_OPERATION; 0x4000];
        longest[0x3fff] = END;
        longest[..2].copy_from_slice(&[0x3a00, 0x3fff]);
        assert_eq!(validate(&longest), Ok(()));
        longest.push(END);
        assert!(validate(&longest).is_err());
    }

    #[test]
    fn pool_reuses_programs_and_suffixes() {
        let mut pool = Pool::new(0x6b, 16).unwrap();
        assert_eq!(pool.add(&FOUR_CLICKS), Ok(0xc06b));
        assert_eq!(pool.add(&FOUR_CLICKS[4..]), Ok(0xc06f));
        assert_eq!(pool.add(&FOUR_CLICKS[8..]), Ok(0xc073));
        // Reuse remains possible when no free storage remains.
        assert_eq!(pool.add(&FOUR_CLICKS), Ok(0xc06b));
        assert!(pool.add(&[NO_OPERATION, END]).is_err());
        assert!(pool.add(&[NO_OPERATION]).is_err());
        assert_eq!(pool.words(), FOUR_CLICKS);

        // Operands are not entry points.
        let mut pool = Pool::new(0x100, 16).unwrap();
        assert_eq!(pool.add(&[0x2600, END, 1, END]), Ok(0xc100));
        assert_eq!(pool.add(&[END]), Ok(0xc103));
        assert_eq!(pool.words().len(), 4);

        let mut pool = Pool::new(0x3fff, 1).unwrap();
        assert_eq!(pool.add(&[END]), Ok(0xffff));
        assert!(Pool::new(0x3fff, 2).is_err());
        assert!(Pool::new(15, 0).is_err());
        assert!(Pool::new(16, 0).unwrap().add(&[END]).is_err());
    }

    #[test]
    fn direct_actions_follow_the_event_encoder() {
        for word in [
            0x2004, 0x2380, 0x2510, 0x27ff, 0x2be7, 0x37ff, 0x3901, 0x2617, 0x262d,
        ] {
            assert!(is_direct(word), "{word:04x}");
        }
        for word in [
            0, 0x2000, 0x20e0, 0x2303, 0x2520, 0x2700, 0x2be8, 0x3000, 0x3800, 0x2600, 0x2621,
            0x3a00, 0x4001, 0x8000, 0xc010,
        ] {
            assert!(!is_direct(word), "{word:04x}");
        }
    }

    #[test]
    fn listing_text_and_termination() {
        let listing = list(&[
            0x2104, 0x2819, 0x3b0f, 0x649c, 0x8f19, 0x00ff, 0xfffc, 0x9007, 2, 0x8b02, 0x0103,
            0x1ffb, END, 0x1234,
        ]);
        assert_eq!(listing.termination, Termination::End);
        let text: Vec<_> = listing
            .instructions
            .iter()
            .map(|i| i.text.as_str())
            .collect();
        assert_eq!(
            text,
            [
                "键盘按下 04",
                "延迟 25 ms",
                "相对位移 100 -100",
                "若 VarZ != VarA 相对跳转 -4",
                "VarH = 内部状态 2",
                "VarC = VarB & VarD",
                "相对跳转 -5",
                "结束"
            ]
        );
        assert_eq!(listing.instructions[3].target, Some(0));
        assert_eq!(listing.instructions[6].target, Some(6));
        assert_eq!(list(&[0x2600, END]).termination, Termination::Incomplete);
        assert_eq!(list(&[0x2601]).termination, Termination::Unparsed);
        assert_eq!(list(&[NO_OPERATION]).termination, Termination::Limit);
        assert_eq!(describe(0xc010), "宏 c010");
        assert_eq!(describe(0x4081), "相对位移 1 1");
        assert_eq!(describe(0x2600), "原始 2600");
    }

    #[test]
    fn checksum_wraps() {
        let mut words = vec![0xffff; 128];
        words[..4].fill(0);
        let (sum, weighted) = checksums(&words);
        assert_eq!(sum, 0xff84);
        assert_eq!(
            weighted,
            (4..128u32).map(|i| i * 0xffff).sum::<u32>() as u16
        );
    }
}
