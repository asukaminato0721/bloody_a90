//! Configuration files: the active sensitivity block and button main vector.
//! Layout: "A90CFG", version (u16), both lengths (u16) and little-endian
//! words. Version 2 appends a u32 byte length and host-button XML.
//! Importing overlays the hardware words on a device image, so editors and the
//! backup parser can read them exactly like saved tables.
use crate::{backup::Backup, configuration, device, sensitivity, storage};

const MAGIC: &[u8; 6] = b"A90CFG";

fn words(bytes: &[u8]) -> Vec<u16> {
    bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_le_bytes(*pair))
        .collect()
}

pub fn export(backup: &Backup) -> Result<Vec<u8>, String> {
    let sensitivity = sensitivity::finish(backup.sensitivity_template()?);
    let buttons = configuration::build(&backup.configuration()?)?;
    let mut bytes = MAGIC.to_vec();
    for value in [1, sensitivity.len() as u16, buttons.len() as u16] {
        bytes.extend(value.to_le_bytes());
    }
    bytes.extend(
        sensitivity
            .iter()
            .chain(&buttons)
            .flat_map(|w| w.to_le_bytes()),
    );
    Ok(bytes)
}

pub fn import(bytes: &[u8], base: &Backup) -> Result<Backup, String> {
    import_all(bytes, base).map(|(backup, _)| backup)
}

pub fn export_all(
    backup: &Backup,
    host: &crate::host_buttons::Settings,
) -> Result<Vec<u8>, String> {
    let mut bytes = export(backup)?;
    if host.is_empty() {
        return Ok(bytes);
    }
    let config = backup.configuration()?;
    let header = configuration::build(&config)?;
    if !host.matches(&header[..16]) {
        return Err("主机动作与导出的按键表不一致。请先应用或重新读取设置。".into());
    }
    host.validate_hardware(&config)?;
    let xml = host.to_bytes();
    crate::host_buttons::Settings::parse(&xml)?;
    bytes[6..8].copy_from_slice(&2u16.to_le_bytes());
    bytes.extend((xml.len() as u32).to_le_bytes());
    bytes.extend(xml);
    Ok(bytes)
}

pub fn import_all(
    bytes: &[u8],
    base: &Backup,
) -> Result<(Backup, Option<crate::host_buttons::Settings>), String> {
    if bytes.len() < 12 || &bytes[..6] != MAGIC {
        return Err("不是 A90 配置文件".into());
    }
    if bytes[6..8] != 2u16.to_le_bytes() {
        return import_hardware(bytes, base).map(|backup| (backup, None));
    }
    let lengths = words(&bytes[8..12]);
    let hardware_end = 12 + (lengths[0] as usize + lengths[1] as usize) * 2;
    if hardware_end + 4 > bytes.len() {
        return Err("配置文件缺少主机动作数据".into());
    }
    let count =
        u32::from_le_bytes(bytes[hardware_end..hardware_end + 4].try_into().unwrap()) as usize;
    if count > 1024 * 1024 || bytes.len() - hardware_end - 4 != count {
        return Err("配置文件的主机动作长度无效".into());
    }
    let mut hardware = bytes[..hardware_end].to_vec();
    hardware[6..8].copy_from_slice(&1u16.to_le_bytes());
    let backup = import_hardware(&hardware, base)?;
    let host = crate::host_buttons::Settings::parse(&bytes[hardware_end + 4..])?;
    let config = backup.configuration()?;
    if !host.matches(&configuration::build(&config)?[..16]) {
        return Err("配置文件的主机动作签名与按键表不一致".into());
    }
    host.validate_hardware(&config)?;
    Ok((backup, Some(host)))
}

fn import_hardware(bytes: &[u8], base: &Backup) -> Result<Backup, String> {
    if bytes.len() < 12 || &bytes[..6] != MAGIC {
        return Err("不是 A90 配置文件".into());
    }
    let header = words(&bytes[6..12]);
    let (sensitivity_len, buttons_len) = (header[1] as usize, header[2] as usize);
    let body = words(&bytes[12..]);
    if header[0] != 1
        || sensitivity_len != sensitivity::BLOCK
        || !(32..=configuration::WORDS).contains(&buttons_len)
        || body.len() != sensitivity_len + buttons_len
        || !bytes.len().is_multiple_of(2)
    {
        return Err("配置文件版本或长度无效".into());
    }
    let (sensitivity, buttons) = body.split_at(sensitivity_len);
    for block in [sensitivity, buttons] {
        if block[..2] != [0xa4a4, 0xffff] || storage::checksums(block) != (block[2], block[3]) {
            return Err("配置文件的头部或校验值无效".into());
        }
    }
    let mut image = base.to_bytes();
    for (address, block) in [
        (device::SENSITIVITY, sensitivity),
        (device::BUTTONS, buttons),
    ] {
        for (index, word) in block.iter().enumerate() {
            image[(address + index) * 2..][..2].copy_from_slice(&word.to_le_bytes());
        }
    }
    let backup = Backup::parse(&image)?;
    backup.sensitivity()?;
    backup.configuration()?;
    Ok(backup)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn factory_image() -> Backup {
        let mut image = vec![0; 32768];
        let mut put = |address: usize, values: &[u16]| {
            for (index, word) in values.iter().enumerate() {
                image[(address + index) * 2..][..2].copy_from_slice(&word.to_le_bytes());
            }
        };
        put(
            0x1804,
            &[
                0x04fd, 7, 0xa4a4, 0xa4a4, 0, 0, 1, 0, 64, 64, 0x17, 64, 64, 0x1b,
            ],
        );
        // A one-profile factory button table built with the same builder.
        let config = configuration::Configuration {
            profiles: vec![configuration::Profile {
                buttons: Default::default(),
                wheel: [
                    configuration::Action::Direct(0x262c),
                    configuration::Action::Direct(0x262b),
                ],
                gestures: None,
                metadata: [0x01ff, 0],
            }],
            selected: 0,
            auxiliary: std::array::from_fn(|_| configuration::Action::Direct(0x262e)),
            parameters: [50; 6],
            flags: 0x10,
            header_word6: 1,
        };
        put(0x2600, &configuration::build(&config).unwrap());
        Backup::parse(&image).unwrap()
    }

    #[test]
    fn profile_v2_keeps_host_actions_and_rejects_a_mismatched_attachment() {
        let base = factory_image();
        let config = base.configuration().unwrap();
        let mut host = crate::host_buttons::Settings::default();
        host.resize(config.profiles.len());
        host.profiles[0][3] = Some(
            crate::gestures::Action::input_text("中😀\n<&")
                .unwrap()
                .with_mode(0),
        );
        host.profiles[0][8] = Some(crate::gestures::Action::new(2, 0, 0, "16 合一"));
        let mut documents = [
            crate::gestures::Document::new(crate::gestures::Kind::Sixteen),
            crate::gestures::Document::new(crate::gestures::Kind::Mouse),
        ];
        documents[0].actions[0] = crate::gestures::Action::keyboard(27, 1, "剪切");
        host.gestures = Some(Box::new(crate::host_buttons::Gestures {
            enabled: false,
            documents,
        }));
        host.bind(&config).unwrap();
        let bytes = export_all(&base, &host).unwrap();
        assert_eq!(&bytes[6..8], &2u16.to_le_bytes());
        let (imported, attached) = import_all(&bytes, &base).unwrap();
        assert_eq!(attached.unwrap(), host);
        assert_eq!(
            configuration::build(&imported.configuration().unwrap()).unwrap(),
            configuration::build(&config).unwrap()
        );
        assert!(
            import_all(&export(&base).unwrap(), &base)
                .unwrap()
                .1
                .is_none()
        );
        assert!(import_all(&bytes[..bytes.len() - 1], &base).is_err());
        let mut invalid = bytes.clone();
        let hardware_end =
            12 + (sensitivity::BLOCK + configuration::build(&config).unwrap().len()) * 2;
        let text = String::from_utf8(bytes[hardware_end + 4..].to_vec()).unwrap();
        let old_signature = host
            .signature
            .unwrap()
            .iter()
            .map(|w| format!("{w:04x}"))
            .collect::<Vec<_>>()
            .join(",");
        let mut wrong = host.signature.unwrap();
        wrong[3] ^= 1;
        let wrong_signature = wrong
            .iter()
            .map(|w| format!("{w:04x}"))
            .collect::<Vec<_>>()
            .join(",");
        invalid.truncate(hardware_end + 4);
        invalid.extend(text.replace(&old_signature, &wrong_signature).bytes());
        assert!(import_all(&invalid, &base).is_err());
    }

    #[test]
    fn round_trip_and_rejections() {
        let base = factory_image();
        let file = export(&base).unwrap();
        let imported = import(&file, &base).unwrap();
        assert_eq!(imported.sensitivity().unwrap().count, 2);
        assert_eq!(export(&imported).unwrap(), file);
        let mut broken = file.clone();
        *broken.last_mut().unwrap() ^= 1;
        assert!(import(&broken, &base).is_err());
        assert!(import(&file[..file.len() - 2], &base).is_err());
        assert!(import(b"nope", &base).is_err());
    }
}
