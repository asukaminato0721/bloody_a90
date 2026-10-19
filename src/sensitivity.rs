//! EXE 00459de0 / 0045b5d0, sensor ID 3327. See docs/research/SENSITIVITY.md.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Record {
    pub x_scale: u16,
    pub y_scale: u16,
    pub sensor_code: u16,
}

/// Eight runtime bytes used by EXE 0043c5d8 and command 0d. Firmware
/// 12e1 copies them to RAM 26,27,28,29,2a,23,2d,2e respectively.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Runtime {
    pub count: u8,
    pub selected: u8,
    pub record: Record,
    pub rate_code: u8,
}

impl Runtime {
    pub fn payload(self) -> Result<[u8; 8], String> {
        if !(1..=8).contains(&self.count) || self.selected >= self.count {
            return Err("运行时 DPI 档位数量或选择值无效".into());
        }
        if self.record.x_scale == 0 || self.record.y_scale == 0 || self.record.sensor_code > 255 {
            return Err("运行时 DPI 的轴比例或传感器编码无效".into());
        }
        if ![1, 2, 4, 8].contains(&self.rate_code) {
            return Err("运行时回报率编码应为 1、2、4 或 8".into());
        }
        let [x_low, x_high] = self.record.x_scale.to_le_bytes();
        let [y_low, y_high] = self.record.y_scale.to_le_bytes();
        Ok([
            self.count - 1,
            self.selected,
            x_low,
            y_low,
            self.record.sensor_code as u8,
            self.rate_code,
            x_high,
            y_high,
        ])
    }

    pub fn from_payload(payload: [u8; 8]) -> Result<Self, String> {
        if payload[0] >= 8 {
            return Err("运行时 DPI 档位数量无效".into());
        }
        let runtime = Self {
            count: payload[0] + 1,
            selected: payload[1],
            record: Record {
                x_scale: u16::from_le_bytes([payload[2], payload[6]]),
                y_scale: u16::from_le_bytes([payload[3], payload[7]]),
                sensor_code: payload[4] as u16,
            },
            rate_code: payload[5],
        };
        runtime.payload()?;
        Ok(runtime)
    }

    pub fn from_template(words: &[u16]) -> Result<Self, String> {
        if words.len() != BLOCK || words[4] >= 8 || words[5] > words[4] || words[0x1e] > 255 {
            return Err("灵敏度模板中的运行参数无效".into());
        }
        let at = 6 + words[5] as usize * 3;
        let runtime = Self {
            count: words[4] as u8 + 1,
            selected: words[5] as u8,
            record: Record {
                x_scale: words[at],
                y_scale: words[at + 1],
                sensor_code: words[at + 2],
            },
            rate_code: words[0x1e] as u8,
        };
        runtime.payload()?;
        Ok(runtime)
    }
}

const CODES: [u16; 63] = [
    0, 0, 4, 6, 9, 11, 14, 16, 18, 21, 23, 25, 27, 29, 32, 34, 36, 39, 41, 43, 46, 48, 50, 52, 55,
    57, 59, 62, 64, 66, 69, 72, 74, 77, 78, 81, 84, 85, 88, 91, 93, 95, 98, 100, 102, 105, 107,
    109, 112, 114, 116, 119, 121, 123, 126, 128, 130, 133, 135, 137, 140, 142, 145,
];

pub fn encode(x: u32, y: u32) -> Result<Record, String> {
    if !(100..=6200).contains(&x)
        || !(100..=6200).contains(&y)
        || !x.is_multiple_of(100)
        || !y.is_multiple_of(100)
    {
        return Err("DPI 必须为 100 至 6200，步长为 100".into());
    }
    let mut record = Record {
        x_scale: 64,
        y_scale: 64,
        sensor_code: 0,
    };
    if x == y {
        record.sensor_code = ((x + 22) / 44 - 1) as u16;
    } else {
        let larger = x.max(y);
        let scale = ((x.min(y) * 128 + larger) / (larger * 2)) as u16;
        record.sensor_code = CODES[(larger / 100) as usize];
        if x < y {
            record.x_scale = scale;
        } else {
            record.y_scale = scale;
        }
    }
    Ok(record)
}

/// Only for a known factory record. This is not an inverse of `encode`.
pub fn factory_cpi(record: Record) -> Option<u32> {
    if record.x_scale != 64 || record.y_scale != 64 {
        return None;
    }
    CODES
        .iter()
        .enumerate()
        .skip(1)
        .find(|(_, code)| **code == record.sensor_code)
        .map(|(index, _)| index as u32 * 100)
}

/// X/Y DPI of a stored record: an exact `encode` result on the 100-DPI grid,
/// otherwise a factory table code. Other records have no known DPI.
pub fn decode(record: Record) -> Option<[u32; 2]> {
    let grid = (1..=62).map(|step| step * 100);
    for x in grid.clone() {
        for y in grid.clone() {
            if encode(x, y) == Ok(record) {
                return Some([x, y]);
            }
        }
    }
    factory_cpi(record).map(|cpi| [cpi, cpi])
}

/// Words in a saved sensitivity block, EXE 0043c5d8.
pub const BLOCK: usize = 128;
/// Words in the factory table at 1806. Firmware 07 reads offsets up to 34.
pub const FACTORY: usize = 54;
/// Presets the official program shows for this model.
pub const PRESETS: usize = 5;

/// Lift cutoff for sensor 3327. EXE 0044bff0 maps level 1..16 to code 15..0.
/// These levels are relative settings, not distances in millimetres.
pub fn lift_height(template: &[u16]) -> Result<Option<u8>, String> {
    if template.len() != BLOCK {
        return Err("灵敏度模板必须包含 128 个字".into());
    }
    let word = template[0x1f];
    if word & 0x40 == 0 {
        return Ok(None);
    }
    let code = word & 0x1f;
    if code > 15 {
        return Err(format!("抬起高度编码 {code:02x} 不在 A90 的范围内"));
    }
    Ok(Some(16 - code as u8))
}

/// Change the lift cutoff in word 1f. Firmware 0acd loads its low byte into
/// RAM 24. Keep the other bits, including the factory word's high byte.
pub fn with_lift_height(template: &[u16], level: Option<u8>) -> Result<Vec<u16>, String> {
    if template.len() != BLOCK {
        return Err("灵敏度模板必须包含 128 个字".into());
    }
    let mut words = template.to_vec();
    words[0x1f] = match level {
        Some(level @ 1..=16) => (words[0x1f] & !0x5f) | 0x40 | u16::from(16 - level),
        Some(_) => return Err("抬起高度应为 1 至 16 级".into()),
        None => words[0x1f] & !0x40,
    };
    Ok(finish(words))
}

/// Patch active presets, the selected index and an optional report rate into
/// a complete 128-word template. Inactive records and other words stay
/// unchanged. The rate code is the EXE encoding, not a measured report rate.
pub fn block(
    template: &[u16],
    presets: &[[u32; 2]],
    selected: usize,
    rate_hz: Option<u32>,
) -> Result<Vec<u16>, String> {
    if template.len() != BLOCK {
        return Err("灵敏度模板必须包含 128 个字".into());
    }
    if presets.is_empty() || presets.len() > PRESETS || selected >= presets.len() {
        return Err("DPI 档位数量应为 1 至 5，当前档位必须启用".into());
    }
    let rate = rate_hz.map(rate_code).transpose()?;
    let records = presets
        .iter()
        .map(|[x, y]| encode(*x, *y))
        .collect::<Result<Vec<_>, _>>()?;
    let mut words = template.to_vec();
    words[4] = presets.len() as u16 - 1;
    words[5] = selected as u16;
    for (index, record) in records.iter().enumerate() {
        words[6 + index * 3..9 + index * 3].copy_from_slice(&[
            record.x_scale,
            record.y_scale,
            record.sensor_code,
        ]);
    }
    if let Some(code) = rate {
        words[0x1e] = code;
    }
    Ok(finish(words))
}

/// Report-rate code of the EXE: the polling interval in milliseconds.
pub fn rate_code(hz: u32) -> Result<u16, String> {
    match hz {
        125 | 250 | 500 | 1000 => Ok((1000 / hz) as u16),
        _ => Err("回报率应为 125、250、500 或 1000 Hz".into()),
    }
}

/// Change only the report rate of a complete template.
pub fn with_rate(template: &[u16], hz: u32) -> Result<Vec<u16>, String> {
    if template.len() != BLOCK {
        return Err("灵敏度模板必须包含 128 个字".into());
    }
    let mut words = template.to_vec();
    words[0x1e] = rate_code(hz)?;
    Ok(finish(words))
}

/// Saved header a4a4/ffff and both checksums over words 4 to 127.
pub fn finish(mut words: Vec<u16>) -> Vec<u16> {
    words[..2].copy_from_slice(&[0xa4a4, 0xffff]);
    (words[2], words[3]) = crate::storage::checksums(&words);
    words
}

/// Complete template from the factory table. The words after the 54 that
/// firmware 07 reads are zero. Do not read 128 words at 1806: other factory
/// tables follow the sensitivity table.
pub fn template_from_factory(factory: &[u16]) -> Result<Vec<u16>, String> {
    if factory.len() != FACTORY {
        return Err("出厂灵敏度表必须包含 54 个字".into());
    }
    let mut words = factory.to_vec();
    words.resize(BLOCK, 0);
    Ok(words)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_payload_matches_exe_and_firmware_order() {
        let mut words = vec![0; BLOCK];
        words[4] = 4;
        words[5] = 1;
        words[9..12].copy_from_slice(&[0x1234, 0xabcd, 0x56]);
        words[0x1e] = 2;
        let runtime = Runtime::from_template(&words).unwrap();
        let payload = [4, 1, 0x34, 0xcd, 0x56, 2, 0x12, 0xab];
        assert_eq!(runtime.payload(), Ok(payload));
        assert_eq!(Runtime::from_payload(payload), Ok(runtime));
        for bad in [
            [255, 0, 64, 64, 0x23, 2, 0, 0],
            [4, 5, 64, 64, 0x23, 2, 0, 0],
            [4, 1, 0, 64, 0x23, 2, 0, 0],
            [4, 1, 64, 64, 0x23, 0x11, 0, 0],
        ] {
            assert!(Runtime::from_payload(bad).is_err());
        }
        words[11] = 256;
        assert!(Runtime::from_template(&words).is_err());
    }

    #[test]
    fn lift_cutoff_reference_and_preservation() {
        let mut template = vec![0; BLOCK];
        template[0x1f] = 0xff00;
        assert_eq!(lift_height(&template), Ok(None));
        for (level, code) in [(1, 0xff4f), (8, 0xff48), (16, 0xff40)] {
            let words = with_lift_height(&template, Some(level)).unwrap();
            assert_eq!(words[0x1f], code);
            assert_eq!(lift_height(&words), Ok(Some(level)));
            for i in (4..BLOCK).filter(|i| *i != 0x1f) {
                assert_eq!(words[i], template[i]);
            }
            assert_eq!(crate::storage::checksums(&words), (words[2], words[3]));
            let off = with_lift_height(&words, None).unwrap();
            assert_eq!(off[0x1f], code & !0x40);
            assert_eq!(lift_height(&off), Ok(None));
        }
        assert!(with_lift_height(&template, Some(0)).is_err());
        assert!(with_lift_height(&template, Some(17)).is_err());
        assert!(with_lift_height(&template[..127], None).is_err());
        template[0x1f] = 0x5f;
        assert!(lift_height(&template).is_err());
    }

    #[test]
    fn decode_grid_and_factory_codes() {
        for [x, y] in [[800, 800], [1000, 1200], [6200, 100], [3200, 3200]] {
            assert_eq!(decode(encode(x, y).unwrap()), Some([x, y]));
        }
        let factory = Record {
            x_scale: 64,
            y_scale: 64,
            sensor_code: 0x2e,
        };
        assert_eq!(decode(factory), Some([2000, 2000]));
        assert_eq!(
            decode(Record {
                x_scale: 7,
                ..factory
            }),
            None
        );
    }

    #[test]
    fn independent_block() {
        let mut template = vec![0; BLOCK];
        template[30] = 2;
        let words = block(&template, &[[1000, 1000], [1000, 1200]], 1, None).unwrap();
        assert_eq!(
            words[..12],
            [
                0xa4a4, 0xffff, 0x012a, 0x09bb, 1, 1, 0x40, 0x40, 0x16, 0x35, 0x40, 0x1b
            ]
        );
        assert_eq!(words[12..30], [0; 18]);
        assert_eq!(words[30], 2);
    }

    #[test]
    fn preservation_rates_and_errors() {
        let template: Vec<u16> = (0..BLOCK as u16).map(|i| i.wrapping_mul(0x2537)).collect();
        for (hz, code) in [(125, 8), (250, 4), (500, 2), (1000, 1)] {
            let words = block(&template, &[[1000, 1000], [1000, 1200]], 0, Some(hz)).unwrap();
            assert_eq!(words[0x1e], code);
            for index in (12..BLOCK).filter(|i| *i != 0x1e) {
                assert_eq!(words[index], template[index]);
            }
        }
        let five = [
            [100, 100],
            [1000, 1000],
            [1600, 1600],
            [2000, 2000],
            [6200, 6200],
        ];
        let words = block(&template, &five[..1], 0, None).unwrap();
        assert_eq!(words[4..6], [0, 0]);
        assert_eq!(words[9..], template[9..]);
        let words = block(&template, &five, 4, None).unwrap();
        assert_eq!(words[4..6], [4, 4]);
        assert_eq!(words[18..21], [64, 64, 140]);
        assert_eq!(words[21..], template[21..]);
        for (presets, selected, rate) in [
            (&five[..0], 0, None),
            (&five[..2], 2, None),
            (&five[..2], 0, Some(2000)),
            (&[[1000, 1000], [6300, 1000]][..], 0, None),
        ] {
            assert!(block(&template, presets, selected, rate).is_err());
        }
        assert!(block(&template[1..], &five[..1], 0, None).is_err());
        let six = [[1000, 1000]; 6];
        assert!(block(&template, &six, 0, None).is_err());
    }

    #[test]
    fn official_conversion_vectors() {
        assert_eq!(
            encode(1000, 1000).unwrap(),
            Record {
                x_scale: 64,
                y_scale: 64,
                sensor_code: 0x16
            }
        );
        assert_eq!(
            encode(1000, 1200).unwrap(),
            Record {
                x_scale: 53,
                y_scale: 64,
                sensor_code: 0x1b
            }
        );
        assert_eq!(
            encode(2000, 1600).unwrap(),
            Record {
                x_scale: 64,
                y_scale: 51,
                sensor_code: 0x2e
            }
        );
        assert_eq!(encode(100, 800).unwrap().x_scale, 8);
        assert_eq!(encode(6200, 6200).unwrap().sensor_code, 0x8c);
        for (code, cpi) in [
            (0x17, 1000),
            (0x1b, 1200),
            (0x24, 1600),
            (0x2e, 2000),
            (0x91, 6200),
        ] {
            assert_eq!(
                factory_cpi(Record {
                    x_scale: 64,
                    y_scale: 64,
                    sensor_code: code
                }),
                Some(cpi)
            );
        }
    }
    #[test]
    fn reject_invalid_grid() {
        for (x, y) in [
            (0, 1000),
            (1000, 0),
            (1050, 1000),
            (1000, 6300),
            (u32::MAX, 100),
        ] {
            assert!(encode(x, y).is_err());
        }
    }
}
