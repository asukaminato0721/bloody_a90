//! Read an existing firmware-07 image. This module never accesses hidraw.
use crate::{
    configuration::{self, Action, Binding, Configuration, Profile},
    sensitivity::{self, Record},
    storage::{self, Listing, NO_OPERATION, Termination},
};
use std::{collections::BTreeMap, fs::File, io::Read, path::Path};

const FACTORY_BUTTONS: usize = 0x183c;
const SAVED_BUTTONS: usize = 0x2600;
// Inspection limit for one macro listing. Branches are not followed.
const MACRO_LIMIT: usize = 256;

#[derive(Clone)]
pub struct Backup {
    words: Vec<u16>,
}

/// Static evidence about the left-button dispatch in firmware 07.
/// A present route does not prove device execution or authorization.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShootingRoute {
    Absent,
    Present,
    Unknown,
}

impl ShootingRoute {
    pub fn description(self) -> &'static str {
        match self {
            Self::Absent => "当前固件缺少左键射击入口。保存槽位后，左键不会执行槽位程序。",
            Self::Present => "当前固件包含左键射击入口。执行效果尚未实机验证。",
            Self::Unknown => "尚未识别当前固件的左键射击入口。执行效果尚未验证。",
        }
    }
}

#[derive(Debug)]
pub struct Sensitivity {
    pub base: usize,
    pub count: usize,
    pub selected: usize,
    pub rate_code: u16,
    pub records: Vec<Record>,
    pub checksum_valid: Option<bool>,
}

/// Action records of one profile. Indices are compiler records, not physical
/// button numbers.
#[derive(Debug)]
pub struct Tables {
    pub buttons_base: usize,
    /// Press, release.
    pub buttons: Vec<[u16; 2]>,
    pub wheel_base: usize,
    /// Down, up. Firmware 06c2 selects offset zero or one by direction.
    pub wheel: [u16; 2],
}

#[derive(Debug)]
pub struct Buttons {
    pub base: usize,
    pub factory: bool,
    pub stored_count: usize,
    pub selected: usize,
    pub profiles: Vec<Tables>,
    pub auxiliary_base: usize,
    pub auxiliary: Vec<u16>,
    /// Reference, start word and listing.
    pub macros: BTreeMap<u16, (usize, Listing)>,
}

impl Backup {
    pub fn read(path: impl AsRef<Path>) -> Result<Self, String> {
        let file = File::open(path).map_err(|e| format!("无法读取备份：{e}"))?;
        let mut bytes = Vec::new();
        file.take(32769)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        Self::parse(&bytes)
    }

    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() != 32768 {
            return Err("备份必须恰好包含 32768 字节".into());
        }
        let words: Vec<_> = bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|b| u16::from_le_bytes([b[0], b[1]]))
            .collect();
        if words[0x1804] != 0x04fd || words[0x1805] != 7 {
            return Err("备份不匹配 A90 型号 04fd、固件 07".into());
        }
        Ok(Self { words })
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        self.words
            .iter()
            .flat_map(|word| word.to_le_bytes())
            .collect()
    }

    /// Firmware 07: 0708/0709 and 072a/072b are either NOP pairs or left-bit
    /// tests followed by jumps to 0762 and 0773. Check their surrounding
    /// code and both auxiliary readers before interpreting these words.
    pub fn shooting_route(&self) -> ShootingRoute {
        if self.words[0x704..0x708] != [0x6140, 0x2e83, 0x1dff, 0x291e]
            || self.words[0x727..0x72a] != [0x2e83, 0x1dff, 0x2921]
            || self.words[0x762..0x770]
                != [
                    0x2b1b, 0x2b1a, 0x2b1d, 0x2b1c, 0x2e30, 0x7086, 0x8770, 0xc7f6, 0x2d0b, 0xc7fe,
                    0x2e30, 0xc23a, 0x0d00, 0x86b4,
                ]
            || self.words[0x773..0x77e]
                != [
                    0x2e30, 0x7086, 0x877e, 0xc7f6, 0x2d0b, 0xc7fe, 0x2e30, 0x1408, 0xc23a, 0x0d00,
                    0x86b4,
                ]
        {
            return ShootingRoute::Unknown;
        }
        match (&self.words[0x708..0x70a], &self.words[0x72a..0x72c]) {
            ([0, 0], [0, 0]) => ShootingRoute::Absent,
            ([0x7083, 0x8762], [0x7083, 0x8773]) => ShootingRoute::Present,
            _ => ShootingRoute::Unknown,
        }
    }

    /// The factory button configuration, ignoring any saved table. Firmware
    /// falls back to it when the saved marker is absent.
    pub fn factory_configuration(&self) -> Result<Configuration, String> {
        let mut factory = self.clone();
        factory.words[SAVED_BUTTONS] &= 0xff00;
        factory.configuration()
    }

    pub fn word(&self, address: usize) -> Result<u16, String> {
        self.words
            .get(address)
            .copied()
            .ok_or_else(|| "地址超出备份范围".into())
    }

    fn span(&self, address: usize, count: usize) -> Result<&[u16], String> {
        self.words
            .get(address..address + count)
            .ok_or_else(|| "表格超出备份范围".into())
    }

    // Firmware 0244 masks pointer bits 14 and 15 before adding the base.
    fn resolve(&self, base: usize, pointer: u16) -> Result<usize, String> {
        let address = base + (pointer & 0x3fff) as usize;
        self.word(address)?;
        Ok(address)
    }

    // One complete macro at a reference: instructions up to END, validated.
    fn program(&self, base: usize, reference: u16) -> Result<Vec<u16>, String> {
        let start = self.resolve(base, reference)?;
        let listing = storage::list(&self.words[start..]);
        if listing.termination != Termination::End {
            return Err(format!("宏 {reference:04x} 没有完整的结束指令"));
        }
        let words: Vec<u16> = listing
            .instructions
            .into_iter()
            .flat_map(|instruction| instruction.words)
            .collect();
        storage::validate(&words).map_err(|e| format!("宏 {reference:04x}：{e}"))?;
        Ok(words)
    }

    fn action(&self, base: usize, word: u16) -> Result<Action, String> {
        if word >= 0xc000 {
            Ok(Action::Macro(self.program(base, word)?))
        } else if storage::is_direct(word) {
            Ok(Action::Direct(word))
        } else {
            Err(format!("{word:04x} 不是可转换的动作"))
        }
    }

    fn bindings<const N: usize>(&self, base: usize, words: &[u16]) -> Result<[Binding; N], String> {
        let mut bindings: [Binding; N] = std::array::from_fn(|_| Binding::default());
        for (binding, pair) in bindings.iter_mut().zip(words.as_chunks::<2>().0) {
            binding.press = self.action(base, pair[0])?;
            binding.release = self.action(base, pair[1])?;
        }
        Ok(bindings)
    }

    /// Convert the active button table into builder input, so an edit can
    /// rebuild a complete main vector. Firmware 080c looks up factory tables
    /// with profile zero, but the runtime profile index (RAM 263) still cycles
    /// through all profiles in the factory header. A factory conversion
    /// therefore keeps every profile, with shared tables and each profile's
    /// own metadata. The factory gesture pointer reaches program code, so a
    /// factory conversion uses an empty gesture table.
    pub fn configuration(&self) -> Result<Configuration, String> {
        let buttons = self.buttons()?;
        let base = buttons.base;
        let list = |header: usize, index: usize| -> Result<usize, String> {
            let list = self.resolve(base, self.words[base + header])?;
            self.resolve(base, self.word(list + index)?)
        };
        let metadata = self.resolve(base, self.words[base + 10])?;
        let mut profiles = Vec::new();
        for index in 0..buttons.stored_count {
            let tables = &buttons.profiles[if buttons.factory { 0 } else { index }];
            let gestures = if buttons.factory {
                None
            } else {
                let at = list(9, index)?;
                let words = self.span(at, configuration::GESTURES * 2)?;
                Some(Box::new(self.bindings(base, words)?))
            };
            let pair = self.span(metadata + index * 2, 2)?;
            profiles.push(Profile {
                buttons: self.bindings(base, &tables.buttons.concat())?,
                wheel: [
                    self.action(base, tables.wheel[0])?,
                    self.action(base, tables.wheel[1])?,
                ],
                gestures,
                metadata: [pair[0], pair[1]],
            });
        }
        let auxiliary = self.span(buttons.auxiliary_base, 22)?;
        let mut actions: [Action; 16] = std::array::from_fn(|_| Action::Direct(NO_OPERATION));
        for (action, word) in actions.iter_mut().zip(&auxiliary[..16]) {
            *action = self.action(base, *word)?;
        }
        Ok(Configuration {
            profiles,
            selected: self.words[base + 4] as usize & 255,
            auxiliary: actions,
            parameters: auxiliary[16..].try_into().unwrap(),
            flags: self.words[base + 5],
            header_word6: self.words[base + 6],
        })
    }

    /// Button, wheel and auxiliary tables. Sources: firmware 07fe, 080c, 0812,
    /// 0244 and EXE 00619b60. Gesture and lighting tables are not parsed.
    pub fn buttons(&self) -> Result<Buttons, String> {
        // Firmware 117c and 1185 test only the low marker byte.
        let factory = self.words[SAVED_BUTTONS] & 255 != 0xa4;
        let base = if factory {
            FACTORY_BUTTONS
        } else {
            SAVED_BUTTONS
        };
        let state = self.words[base + 4];
        let stored_count = (state >> 8) as usize + 1;
        let selected = (state & 255) as usize;
        if stored_count > 7 || selected >= stored_count {
            return Err("按键表中的配置数量或选择值无效".into());
        }
        // Firmware 080c forces profile zero without the saved marker.
        let count = if factory { 1 } else { stored_count };
        let mut profiles = Vec::new();
        let mut references = Vec::new();
        for index in 0..count {
            let mut table = |header: usize, entries: usize| -> Result<_, String> {
                let list = self.resolve(base, self.words[base + header])?;
                let address = self.resolve(base, self.word(list + index)?)?;
                let words = self.span(address, entries * 2)?;
                references.extend(words.iter().copied().filter(|word| *word >= 0xc000));
                Ok((address, words))
            };
            let (buttons_base, buttons) = table(7, 24)?;
            let buttons = buttons.as_chunks::<2>().0.to_vec();
            let (wheel_base, wheel) = table(8, 1)?;
            let wheel = [wheel[0], wheel[1]];
            profiles.push(Tables {
                buttons_base,
                buttons,
                wheel_base,
                wheel,
            });
        }
        let auxiliary_base = self.resolve(base, self.words[base + 11])?;
        let auxiliary = self.span(auxiliary_base, 16)?.to_vec();
        references.extend(auxiliary.iter().copied().filter(|word| *word >= 0xc000));
        let mut macros = BTreeMap::new();
        for reference in references {
            if let std::collections::btree_map::Entry::Vacant(entry) = macros.entry(reference) {
                let start = self.resolve(base, reference)?;
                let end = (start + MACRO_LIMIT).min(self.words.len());
                entry.insert((start, storage::list(&self.words[start..end])));
            }
        }
        Ok(Buttons {
            base,
            factory,
            stored_count,
            selected: if factory { 0 } else { selected },
            profiles,
            auxiliary_base,
            auxiliary,
            macros,
        })
    }

    /// Template for a new saved sensitivity block: the saved block when its
    /// marker is present, otherwise the factory table padded to 128 words.
    pub fn sensitivity_template(&self) -> Result<Vec<u16>, String> {
        if self.words[0x2400] & 255 == 0xa4 {
            Ok(self.words[0x2400..0x2400 + sensitivity::BLOCK].to_vec())
        } else {
            sensitivity::template_from_factory(&self.words[0x1806..0x1806 + sensitivity::FACTORY])
        }
    }

    pub fn sensitivity(&self) -> Result<Sensitivity, String> {
        let saved = self.words[0x2400] & 255 == 0xa4;
        let base = if saved { 0x2400 } else { 0x1806 };
        let count = (self.words[base + 4] & 255) as usize + 1;
        let selected = (self.words[base + 5] & 255) as usize;
        if count > 8 || selected >= count {
            return Err("备份中的 DPI 档位数量或选择值无效".into());
        }
        let records = (0..count)
            .map(|index| {
                let at = base + 6 + index * 3;
                Record {
                    x_scale: self.words[at],
                    y_scale: self.words[at + 1],
                    sensor_code: self.words[at + 2],
                }
            })
            .collect();
        let checksum_valid = saved.then(|| {
            let block = &self.words[base..base + 128];
            storage::checksums(block) == (block[2], block[3])
        });
        Ok(Sensitivity {
            base,
            count,
            selected,
            rate_code: self.words[base + 0x1e],
            records,
            checksum_valid,
        })
    }

    /// Each section reports its own error, so one damaged table does not
    /// hide the others.
    pub fn summary(&self) -> String {
        let mut output = "Bloody A90 · 型号 04fd · 固件 07\n".to_owned();
        output.push_str(self.shooting_route().description());
        output.push('\n');
        match self.sensitivity_summary() {
            Ok(text) => output.push_str(&text),
            Err(error) => output.push_str(&format!("DPI 参数：{error}\n")),
        }
        match self.buttons() {
            Ok(buttons) => output.push_str(&buttons.summary()),
            Err(error) => output.push_str(&format!("\n按键表：{error}\n")),
        }
        output
    }

    fn sensitivity_summary(&self) -> Result<String, String> {
        let data = self.sensitivity()?;
        let mut output = format!(
            "DPI 参数来源：{} · 字地址 {:04x}\n启用档位：{} · 当前档位：{}\n回报率原始编码：{:04x}\n校验：{}\n",
            if data.base == 0x1806 {
                "出厂表"
            } else {
                "保存表"
            },
            data.base,
            data.count,
            data.selected + 1,
            data.rate_code,
            match data.checksum_valid {
                Some(true) => "通过",
                Some(false) => "不匹配",
                None => "出厂表不使用此校验",
            }
        );
        for (index, record) in data.records.iter().enumerate() {
            let cpi = if data.base == 0x1806 {
                sensitivity::factory_cpi(*record)
                    .map(|v| format!(" · {v} DPI"))
                    .unwrap_or_default()
            } else {
                String::new()
            };
            output.push_str(&format!(
                "档位 {}：X 比例 {} · Y 比例 {} · 传感器编码 {:04x}{cpi}\n",
                index + 1,
                record.x_scale,
                record.y_scale,
                record.sensor_code
            ));
        }
        Ok(output)
    }
}

impl Buttons {
    pub fn summary(&self) -> String {
        let mut output = format!(
            "\n按键表来源：{} · 字地址 {:04x}\n保存的配置数量：{} · 解析配置数量：{} · 当前配置：{}\n",
            if self.factory {
                "出厂表"
            } else {
                "保存表"
            },
            self.base,
            self.stored_count,
            self.profiles.len(),
            self.selected + 1,
        );
        let pair = |words: &[u16]| {
            format!(
                "{} / {}",
                storage::describe(words[0]),
                storage::describe(words[1])
            )
        };
        for (index, profile) in self.profiles.iter().enumerate() {
            output.push_str(&format!(
                "\n配置 {} · 按键表 {:04x}",
                index + 1,
                profile.buttons_base
            ));
            if let Some(shared) = self.profiles[..index]
                .iter()
                .position(|other| other.buttons_base == profile.buttons_base)
            {
                output.push_str(&format!(" · 与配置 {} 共用", shared + 1));
            }
            output.push('\n');
            for (record, words) in profile.buttons.iter().enumerate() {
                if *words != [NO_OPERATION; 2] {
                    output.push_str(&format!("  记录 {:02}：{}\n", record + 1, pair(words)));
                }
            }
            output.push_str(&format!(
                "  滚轮 {:04x}：向下 {} · 向上 {}\n",
                profile.wheel_base,
                storage::describe(profile.wheel[0]),
                storage::describe(profile.wheel[1])
            ));
        }
        output.push_str("未列出的按键记录为空操作。记录序号不是物理按键编号。\n");
        output.push_str(&format!(
            "\n辅助动作 · 字地址 {:04x}\n",
            self.auxiliary_base
        ));
        for (index, word) in self.auxiliary.iter().enumerate() {
            if *word != NO_OPERATION {
                output.push_str(&format!(
                    "  {:02}：{}\n",
                    index + 1,
                    storage::describe(*word)
                ));
            }
        }
        for (reference, (start, listing)) in &self.macros {
            output.push_str(&format!(
                "\n宏 {reference:04x} · 字地址 {start:04x} · {}\n",
                match listing.termination {
                    Termination::End => "结束指令",
                    Termination::Limit => "达到 256 字检查上限",
                    Termination::Incomplete => "指令不完整",
                    Termination::Unparsed => "含未解析指令",
                }
            ));
            for instruction in &listing.instructions {
                output.push_str(&format!(
                    "  {:04x}  {:<15}{}\n",
                    start + instruction.offset,
                    crate::macros::hex(&instruction.words),
                    instruction.text
                ));
            }
        }
        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn image() -> Vec<u8> {
        let mut data = vec![0; 32768];
        for (at, value) in [(0x1804, 0x04fdu16), (0x1805, 7), (0x180a, 4), (0x180b, 0)] {
            data[at * 2..at * 2 + 2].copy_from_slice(&value.to_le_bytes());
        }
        data
    }
    #[test]
    fn bounds_and_identity() {
        assert!(Backup::parse(&[]).is_err());
        assert!(Backup::parse(&vec![0; 32768]).is_err());
        let backup = Backup::parse(&image()).unwrap();
        assert_eq!(backup.sensitivity().unwrap().count, 5);
        assert!(backup.word(0x4000).is_err());
    }
    #[test]
    fn shooting_route_checks_both_hooks_and_their_code() {
        let mut backup = Backup::parse(&image()).unwrap();
        assert_eq!(backup.shooting_route(), ShootingRoute::Unknown);
        // Instruction windows from the installed firmware-07 backup.
        for (at, words) in [
            (0x704, &[0x6140, 0x2e83, 0x1dff, 0x291e][..]),
            (0x727, &[0x2e83, 0x1dff, 0x2921][..]),
            (
                0x762,
                &[
                    0x2b1b, 0x2b1a, 0x2b1d, 0x2b1c, 0x2e30, 0x7086, 0x8770, 0xc7f6, 0x2d0b, 0xc7fe,
                    0x2e30, 0xc23a, 0x0d00, 0x86b4,
                ][..],
            ),
            (
                0x773,
                &[
                    0x2e30, 0x7086, 0x877e, 0xc7f6, 0x2d0b, 0xc7fe, 0x2e30, 0x1408, 0xc23a, 0x0d00,
                    0x86b4,
                ][..],
            ),
        ] {
            backup.words[at..at + words.len()].copy_from_slice(words);
        }
        assert_eq!(backup.shooting_route(), ShootingRoute::Absent);
        // The core-3/4 branch pattern, adjusted to firmware-07 addresses.
        backup.words[0x708..0x70a].copy_from_slice(&[0x7083, 0x8762]);
        assert_eq!(backup.shooting_route(), ShootingRoute::Unknown);
        backup.words[0x72a..0x72c].copy_from_slice(&[0x7083, 0x8773]);
        assert_eq!(backup.shooting_route(), ShootingRoute::Present);
        backup.words[0x709] = 0x8763;
        assert_eq!(backup.shooting_route(), ShootingRoute::Unknown);
        backup.words[0x709] = 0x8762;
        backup.words[0x773] ^= 1;
        assert_eq!(backup.shooting_route(), ShootingRoute::Unknown);
    }
    #[test]
    fn saved_checksum_and_invalid_preset() {
        let mut data = image();
        data[0x2400 * 2] = 0xa4;
        let mut backup = Backup::parse(&data).unwrap();
        assert_eq!(backup.sensitivity().unwrap().checksum_valid, Some(true));
        backup.words[0x2406] = 64;
        assert_eq!(backup.sensitivity().unwrap().checksum_valid, Some(false));
        backup.words[0x2405] = 1;
        assert!(backup.sensitivity().is_err());
    }
}
