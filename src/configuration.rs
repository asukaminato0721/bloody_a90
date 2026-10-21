//! New saved main vector. EXE 00619b60, 00617c8c and 006112e0.
//! Firmware 07 format only. Lighting effect tables, external storage vectors,
//! factory conversion and host actions are not built here.
use crate::storage::{self, END, NO_OPERATION, Pool};

// Firmware 07 erases 256-word pages and keeps code at 3d00, so a main vector
// stored at 2600 must end before it. The EXE limit of 3000 bytes is larger.
pub const WORDS: usize = crate::flash::BUTTONS_END - crate::flash::BUTTONS;
pub const PROFILES: usize = 7;
pub const BUTTONS: usize = 24;
pub const GESTURES: usize = 162;

/// One direct event word or one complete compiled macro.
/// Raw storage references are not accepted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Direct(u16),
    Macro(Vec<u16>),
}

impl Action {
    /// EXE 006112e0 keeps single-word sequences as direct assignments. An
    /// empty sequence becomes a direct no-operation, as in the factory click
    /// macros: a release macro would start on release and stop the press
    /// macro, which the A90 shows when an END-only release is stored.
    pub fn from_program(words: Vec<u16>) -> Self {
        match words[..] {
            [END] => Self::Direct(NO_OPERATION),
            [word, END] if storage::is_direct(word) => Self::Direct(word),
            _ => Self::Macro(words),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Binding {
    pub press: Action,
    pub release: Action,
}

impl Default for Binding {
    fn default() -> Self {
        Self {
            press: Action::Direct(NO_OPERATION),
            release: Action::Direct(NO_OPERATION),
        }
    }
}

/// Button indices are compiler records, not physical button numbers.
#[derive(Clone, Debug)]
pub struct Profile {
    pub buttons: [Binding; BUTTONS],
    /// Down, up. These are not press and release events.
    pub wheel: [Action; 2],
    /// None creates a complete no-operation gesture table.
    pub gestures: Option<Box<[Binding; GESTURES]>>,
    /// Lighting of this profile: effect in the high byte of word 0, then
    /// red, green and blue (EXE TtabBrightness, USB capture of 2026-10-21).
    pub metadata: [u16; 2],
}

/// Effects of the official 「RGB魔幻灯效」 list, in its order. The EXE writes
/// the list index into every profile; the factory tables use 1.
pub const LIGHT_EFFECTS: [&str; 7] = [
    "全彩恒亮",
    "全彩呼吸(慢闪)",
    "全彩呼吸(快闪)",
    "全彩循环(慢闪)",
    "全彩循环(快闪)",
    "微动换彩(慢)",
    "微动换彩(快)",
];

/// Metadata, parameters and flags keep raw EXE values.
#[derive(Clone, Debug)]
pub struct Configuration {
    pub profiles: Vec<Profile>,
    pub selected: usize,
    pub auxiliary: [Action; 16],
    pub parameters: [u16; 6],
    pub flags: u16,
    /// Low byte: button response time plus one (EXE 00619b60, firmware 0ad3).
    pub header_word6: u16,
    /// Raw lighting-effect vectors of header word 13, kept as read. The
    /// official program writes five, empty ones on firmware 0c. Empty means
    /// no list, as this builder wrote before. See docs/research/SHOOTING.md.
    pub lighting: Vec<Vec<u16>>,
}

impl Configuration {
    /// Only slots 1..3 have a packed timing word in firmware 07. The EXE
    /// uses 100 with no Y compensation for types A/B/C/M. Preserve all other
    /// parameter words when replacing one slot.
    pub fn set_shooting_parameters(
        &mut self,
        index: usize,
        vertical: Option<crate::shooting::Shot>,
    ) -> Result<(), String> {
        if !(1..=7).contains(&index) {
            return Err("射击槽位应为 1 至 7".into());
        }
        let word = vertical
            .map(crate::shooting::vertical_parameter)
            .transpose()?
            .unwrap_or(100);
        if index <= 3 {
            self.parameters[index] = word;
        }
        Ok(())
    }

    /// Shooting indices 1..7. Index zero uses the normal left button.
    /// EXE 00619b60 at 61ae3a..61afe3, firmware 0762 and 0773.
    pub fn set_shooting(&mut self, index: usize, binding: Binding) -> Result<(), String> {
        if !(1..=7).contains(&index) {
            return Err("射击槽位应为 1 至 7".into());
        }
        // Validate before changing the configuration.
        let mut pool = Pool::new(16, WORDS - 16)?;
        encode(&binding.press, &mut pool)?;
        encode(&binding.release, &mut pool)?;
        self.auxiliary[index] = binding.press;
        self.auxiliary[index + 8] = binding.release;
        Ok(())
    }

    pub fn debounce_ms(&self) -> Option<u8> {
        let value = self.header_word6 as u8;
        (1..=17).contains(&value).then(|| value - 1)
    }

    /// The effect shared by all profiles, or None when they differ.
    pub fn light_effect(&self) -> Option<u8> {
        let first = (self.profiles.first()?.metadata[0] >> 8) as u8;
        self.profiles
            .iter()
            .all(|profile| (profile.metadata[0] >> 8) as u8 == first)
            .then_some(first)
    }

    /// Set the effect of every profile and keep their colours.
    pub fn set_light_effect(&mut self, effect: u8) -> Result<(), String> {
        if usize::from(effect) >= LIGHT_EFFECTS.len() {
            return Err("灯效编号应为 0 至 6".into());
        }
        for profile in &mut self.profiles {
            profile.metadata[0] = (profile.metadata[0] & 0xff) | u16::from(effect) << 8;
        }
        Ok(())
    }

    /// The official slider accepts 0..16 ms. Keep the unused high byte.
    pub fn set_debounce_ms(&mut self, ms: u8) -> Result<(), String> {
        if ms > 16 {
            return Err("硬件按键响应时间应为 0 至 16 ms".into());
        }
        self.header_word6 = (self.header_word6 & 0xff00) | u16::from(ms + 1);
        Ok(())
    }
}

fn encode(action: &Action, pool: &mut Pool) -> Result<u16, String> {
    match action {
        Action::Direct(word) if storage::is_direct(*word) => Ok(*word),
        Action::Direct(word) => Err(format!("{word:04x} 不是直接动作")),
        Action::Macro(program) => pool.add(program),
    }
}

fn encode_all<'a>(
    actions: impl IntoIterator<Item = &'a Binding>,
    pool: &mut Pool,
) -> Result<Vec<u16>, String> {
    let mut words = Vec::new();
    for binding in actions {
        words.push(encode(&binding.press, pool)?);
        words.push(encode(&binding.release, pool)?);
    }
    Ok(words)
}

fn append(words: &mut Vec<u16>, values: &[u16]) -> Result<u16, String> {
    if values.len() > WORDS - words.len() {
        return Err("配置超过 1700 个字，无法存入 2600 至 3d00".into());
    }
    let at = words.len() as u16;
    words.extend_from_slice(values);
    Ok(at)
}

/// Equal tables share storage. Return one pointer per table.
fn append_shared(words: &mut Vec<u16>, tables: &[Vec<u16>]) -> Result<Vec<u16>, String> {
    let mut pointers = Vec::new();
    for (index, table) in tables.iter().enumerate() {
        pointers.push(
            match tables[..index].iter().position(|other| other == table) {
                Some(equal) => pointers[equal],
                None => append(words, table)?,
            },
        );
    }
    Ok(pointers)
}

/// Build the words of a saved main vector. Allocation follows the EXE order:
/// header, macros, auxiliary, metadata, buttons, list, wheels, list, gestures,
/// list. Equal button and gesture tables share storage. Wheel tables stay
/// separate. Success does not establish device execution or a safe write.
pub fn build(config: &Configuration) -> Result<Vec<u16>, String> {
    let count = config.profiles.len();
    if !(1..=PROFILES).contains(&count) || config.selected >= count {
        return Err("配置数量应为 1 至 7，当前配置必须存在".into());
    }
    // EXE sets bits 2, 3, 4, 6 and 7 in this header field.
    if config.flags & !0x00dc != 0 {
        return Err("头部标志包含未确认的位".into());
    }
    let mut pool = Pool::new(16, WORDS - 16)?;
    let mut buttons = Vec::new();
    let mut wheels = Vec::new();
    let mut gestures = Vec::new();
    for profile in &config.profiles {
        buttons.push(encode_all(&profile.buttons, &mut pool)?);
        wheels.push([
            encode(&profile.wheel[0], &mut pool)?,
            encode(&profile.wheel[1], &mut pool)?,
        ]);
        gestures.push(match &profile.gestures {
            Some(table) => encode_all(table.iter(), &mut pool)?,
            None => vec![NO_OPERATION; GESTURES * 2],
        });
    }
    let mut auxiliary = Vec::new();
    for action in &config.auxiliary {
        auxiliary.push(encode(action, &mut pool)?);
    }
    auxiliary.extend(config.parameters);
    let metadata: Vec<_> = config.profiles.iter().flat_map(|p| p.metadata).collect();

    let mut words = vec![0; 16];
    words.extend_from_slice(pool.words());
    let auxiliary_at = append(&mut words, &auxiliary)?;
    let metadata_at = append(&mut words, &metadata)?;
    let pointers = append_shared(&mut words, &buttons)?;
    let buttons_at = append(&mut words, &pointers)?;
    let mut pointers = Vec::new();
    for wheel in &wheels {
        pointers.push(append(&mut words, wheel)?);
    }
    let wheel_at = append(&mut words, &pointers)?;
    let pointers = append_shared(&mut words, &gestures)?;
    let gestures_at = append(&mut words, &pointers)?;
    // EXE 00619b60: each vector followed by a4a4, then (offset, 0) pairs.
    let mut list = Vec::new();
    for vector in &config.lighting {
        list.extend([append(&mut words, vector)?, 0]);
        append(&mut words, &[0xa4a4])?;
    }
    let lighting_at = if list.is_empty() {
        0
    } else {
        append(&mut words, &list)?
    };

    words[..13].copy_from_slice(&[
        0xa4a4,
        0xffff,
        0,
        0,
        ((count - 1) * 256 + config.selected) as u16,
        config.flags,
        config.header_word6,
        buttons_at,
        wheel_at,
        gestures_at,
        metadata_at,
        auxiliary_at,
        0xffff,
    ]);
    words[13] = lighting_at;
    (words[2], words[3]) = storage::checksums(&words);
    Ok(words)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn light_effect_changes_only_the_high_byte() {
        let mut config = configuration(3);
        // Factory colours of profiles 1 to 3: red, green, yellow.
        for (profile, metadata) in
            config
                .profiles
                .iter_mut()
                .zip([[0x1ff, 0], [0x100, 0xff00], [0x1ff, 0xff00]])
        {
            profile.metadata = metadata;
        }
        assert_eq!(config.light_effect(), Some(1));
        config.set_light_effect(5).unwrap();
        let metadata: Vec<_> = config.profiles.iter().map(|p| p.metadata).collect();
        assert_eq!(metadata, [[0x5ff, 0], [0x500, 0xff00], [0x5ff, 0xff00]]);
        assert!(config.set_light_effect(7).is_err());
        config.profiles[1].metadata[0] = 0x200;
        assert_eq!(config.light_effect(), None);
    }

    #[test]
    fn lighting_vectors_survive_a_rebuild() {
        let mut config = configuration(7);
        // Five empty vectors as the official program writes them on 0c.
        config.lighting = vec![vec![0, 0]; 5];
        let words = build(&config).unwrap();
        // Each vector is two words and a4a4; the list follows the last one.
        let list = words[13];
        let expected: Vec<u16> = (0..5).flat_map(|i| [list - 15 + i * 3, 0]).collect();
        assert_eq!(words[list as usize..], expected);
        let mut image = vec![0; 32768];
        image[0x1804 * 2..0x1806 * 2].copy_from_slice(&[0xfd, 4, 7, 0]);
        for (index, word) in words.iter().enumerate() {
            image[(0x2600 + index) * 2..][..2].copy_from_slice(&word.to_le_bytes());
        }
        let saved = crate::backup::Backup::parse(&image)
            .unwrap()
            .configuration()
            .unwrap();
        assert_eq!(saved.lighting, config.lighting);
        assert_eq!(build(&saved).unwrap(), words);
        image[(0x2600 + list as usize + 1) * 2] = 1;
        let damaged = crate::backup::Backup::parse(&image).unwrap();
        assert!(damaged.configuration().is_err());
    }

    #[test]
    fn response_time_and_shooting_slots() {
        let mut config = configuration(7);
        config.header_word6 = 0xab01;
        for (ms, word) in [(0, 0xab01), (8, 0xab09), (16, 0xab11)] {
            config.set_debounce_ms(ms).unwrap();
            assert_eq!(config.header_word6, word);
            assert_eq!(config.debounce_ms(), Some(ms));
            assert_eq!(build(&config).unwrap()[6], word);
        }
        assert!(config.set_debounce_ms(17).is_err());
        assert_eq!(config.header_word6, 0xab11);
        for index in 1..=7 {
            config
                .set_shooting(
                    index,
                    Binding {
                        press: Action::Direct(0x2100 + index as u16),
                        release: Action::Direct(0x2000 + index as u16),
                    },
                )
                .unwrap();
        }
        let words = build(&config).unwrap();
        let base = words[11] as usize;
        assert_eq!(
            &words[base..base + 16],
            &[
                NO_OPERATION,
                0x2101,
                0x2102,
                0x2103,
                0x2104,
                0x2105,
                0x2106,
                0x2107,
                NO_OPERATION,
                0x2001,
                0x2002,
                0x2003,
                0x2004,
                0x2005,
                0x2006,
                0x2007,
            ]
        );
        for index in [0, 8, usize::MAX] {
            assert!(config.set_shooting(index, Binding::default()).is_err());
        }
    }

    #[test]
    fn shooting_parameters_patch_only_the_selected_slot() {
        let mut config = configuration(7);
        let initial = config.parameters;
        let shot = crate::shooting::Shot {
            interval_ms: 100,
            x: 0,
            y: -20,
        };
        for index in 1..=7 {
            config.parameters = initial;
            config.set_shooting_parameters(index, Some(shot)).unwrap();
            let mut expected = initial;
            if index <= 3 {
                expected[index] = 0xec64;
            }
            assert_eq!(config.parameters, expected);
            let words = build(&config).unwrap();
            let base = words[11] as usize + 16;
            assert_eq!(words[base..base + 6], expected);
            config.set_shooting_parameters(index, None).unwrap();
            if index <= 3 {
                expected[index] = 100;
            }
            assert_eq!(config.parameters, expected);
        }
        let before = config.parameters;
        assert!(config.set_shooting_parameters(8, Some(shot)).is_err());
        assert_eq!(config.parameters, before);
    }

    fn profile() -> Profile {
        let mut buttons: [Binding; BUTTONS] = Default::default();
        for (index, binding) in buttons[..5].iter_mut().enumerate() {
            *binding = Binding {
                press: Action::Direct(0x2500 | 1 << index),
                release: Action::Direct(0x2400 | 1 << index),
            };
        }
        Profile {
            buttons,
            wheel: [Action::Direct(0x262c), Action::Direct(0x262b)],
            gestures: None,
            metadata: [0x01ff, 0xff00],
        }
    }

    fn configuration(count: usize) -> Configuration {
        Configuration {
            profiles: vec![profile(); count],
            selected: count - 1,
            auxiliary: std::array::from_fn(|_| Action::Direct(NO_OPERATION)),
            parameters: [50, 50, 50, 50, 50, 0x0101],
            flags: 0x10,
            header_word6: 1,
            lighting: Vec::new(),
        }
    }

    fn assert_checksums(words: &[u16]) {
        assert_eq!(storage::checksums(words), (words[2], words[3]));
    }

    #[test]
    fn profile_limits_and_sharing() {
        for count in 1..=PROFILES {
            let words = build(&configuration(count)).unwrap();
            // Only metadata, lists and the EXE's unshared wheel tables grow.
            assert_eq!(words.len(), 410 + count * 7);
            assert_eq!(words[4] as usize, (count - 1) * 257);
            let (buttons, wheels, gestures) =
                (words[7] as usize, words[8] as usize, words[9] as usize);
            for profile in 0..count {
                assert_eq!(words[buttons + profile], words[buttons]);
                assert_eq!(words[gestures + profile], words[gestures]);
                assert_eq!(
                    words[wheels + profile] as usize,
                    words[wheels] as usize + profile * 2
                );
            }
            assert_checksums(&words);
        }
        // Each unique table needs independent storage.
        let mut config = configuration(7);
        for (index, profile) in config.profiles.iter_mut().enumerate() {
            profile.buttons[23].press = Action::Direct(0x2101 + index as u16);
        }
        assert_eq!(build(&config).unwrap().len(), 459 + 6 * 48);
        for (index, profile) in config.profiles.iter_mut().enumerate() {
            let mut gestures: Box<[Binding; GESTURES]> =
                Box::new(std::array::from_fn(|_| Binding::default()));
            gestures[161].press = Action::Direct(0x2101 + index as u16);
            profile.gestures = Some(gestures);
        }
        config.flags = 0xdc;
        config.header_word6 = 0x5a17;
        let words = build(&config).unwrap();
        assert_eq!(words.len(), 2691);
        assert_eq!(words[5..7], [0xdc, 0x5a17]);
        assert_checksums(&words);
    }

    #[test]
    fn rejects_invalid_input() {
        let valid = configuration(1);
        let mut cases = Vec::new();
        for (selected, count) in [(0, 0), (0, 8), (1, 1)] {
            let mut config = valid.clone();
            config.profiles = vec![profile(); count];
            config.selected = selected;
            cases.push(config);
        }
        let mut config = valid.clone();
        config.flags |= 0x100;
        cases.push(config);
        let mut config = valid.clone();
        config.profiles[0].buttons[23].release = Action::Macro(vec![0x2104, 0x2819]);
        cases.push(config);
        let mut config = valid.clone();
        config.profiles[0].buttons[23].release = Action::Macro(Vec::new());
        cases.push(config);
        let mut config = valid.clone();
        config.auxiliary[15] = Action::Direct(0x2520);
        cases.push(config);
        let mut config = valid.clone();
        config.auxiliary[15] = Action::Direct(0xc010);
        cases.push(config);
        let mut program = vec![NO_OPERATION; WORDS - 16];
        *program.last_mut().unwrap() = END;
        let mut config = valid.clone();
        config.profiles[0].buttons[0].press = Action::Macro(program.clone());
        cases.push(config);
        // The macro fits, but no space remains for configuration tables.
        program.truncate(WORDS - 32);
        *program.last_mut().unwrap() = END;
        let mut config = valid;
        config.profiles[0].buttons[0].press = Action::Macro(program);
        cases.push(config);
        for config in cases {
            assert!(build(&config).is_err());
        }
    }

    #[test]
    fn single_word_sequences_are_direct() {
        assert_eq!(
            Action::from_program(vec![0x262d, END]),
            Action::Direct(0x262d)
        );
        assert_eq!(
            Action::from_program(vec![0, END]),
            Action::Macro(vec![0, END])
        );
        assert_eq!(
            Action::from_program(vec![END]),
            Action::Direct(NO_OPERATION)
        );
    }
}
