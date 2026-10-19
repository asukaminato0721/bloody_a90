//! Host settings that the official program changes in Windows: pointer
//! speed, double-click time and wheel scroll lines. On Linux they belong to
//! the desktop. KDE Plasma and GNOME are supported; nothing reaches the mouse.
use std::process::Command;

const VENDOR: u32 = 0x09da;
const PRODUCT: u32 = 0xfafd;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Desktop {
    Kde,
    Gnome,
    Other,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub desktop: Desktop,
    /// libinput acceleration, -1 (slow) to 1 (fast).
    pub pointer: Option<f64>,
    pub double_click_ms: Option<u32>,
    pub scroll_lines: Option<u32>,
}

pub fn desktop() -> Desktop {
    let name = std::env::var("XDG_CURRENT_DESKTOP")
        .unwrap_or_default()
        .to_uppercase();
    if name.contains("KDE") {
        Desktop::Kde
    } else if name.contains("GNOME") {
        Desktop::Gnome
    } else {
        Desktop::Other
    }
}

fn run(program: &str, args: &[&str]) -> Result<String, String> {
    let output = Command::new(program)
        .args(args)
        .output()
        .map_err(|e| format!("无法运行 {program}：{e}"))?;
    if !output.status.success() {
        return Err(format!(
            "{program} 失败：{}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

// busctl prints a type signature before the value, for example `d 0.5`.
fn busctl_value(output: &str) -> &str {
    output
        .split_once(' ')
        .map_or(output, |(_, value)| value.trim().trim_matches('"'))
}

const KWIN: &str = "org.kde.KWin";
const DEVICE: &str = "org.kde.KWin.InputDevice";

fn kwin_property(device: &str, name: &str) -> Result<String, String> {
    let path = format!("/org/kde/KWin/InputDevice/{device}");
    run(
        "busctl",
        &["--user", "get-property", KWIN, &path, DEVICE, name],
    )
    .map(|output| busctl_value(&output).to_owned())
}

/// A90 KWin pointers that support acceleration, with their libinput names.
fn kwin_devices() -> Result<Vec<(String, String)>, String> {
    let list = run(
        "busctl",
        &[
            "--user",
            "call",
            KWIN,
            "/org/kde/KWin/InputDevice",
            "org.kde.KWin.InputDeviceManager",
            "ListPointers",
        ],
    )?;
    let mut devices = Vec::new();
    for name in list
        .split_whitespace()
        .skip(2)
        .map(|item| item.trim_matches('"'))
    {
        let vendor = kwin_property(name, "vendor")?;
        let product = kwin_property(name, "product")?;
        // KWin also lists the A90 keyboard interface in ListPointers. Its
        // acceleration getter returns zero and its setter does nothing.
        if vendor == VENDOR.to_string()
            && product == PRODUCT.to_string()
            && kwin_property(name, "supportsPointerAcceleration")? == "true"
        {
            let path = format!("/org/kde/KWin/InputDevice/{name}");
            // Keep the trailing space of the libinput name for kcminputrc.
            let raw = run(
                "busctl",
                &["--user", "get-property", KWIN, &path, DEVICE, "name"],
            )?;
            let label = raw
                .strip_prefix("s \"")
                .and_then(|r| r.strip_suffix('"'))
                .unwrap_or("");
            devices.push((name.to_owned(), label.to_owned()));
        }
    }
    Ok(devices)
}

fn kde_read(group: &str, key: &str) -> Option<u32> {
    run(
        "kreadconfig6",
        &["--file", "kdeglobals", "--group", group, "--key", key],
    )
    .ok()
    .and_then(|value| value.parse().ok())
}

fn gsettings(key: &str) -> Option<String> {
    run(
        "gsettings",
        &["get", "org.gnome.desktop.peripherals.mouse", key],
    )
    .ok()
}

pub fn read() -> Settings {
    let desktop = desktop();
    match desktop {
        Desktop::Kde => Settings {
            desktop,
            pointer: kwin_devices().ok().and_then(|devices| {
                let (name, _) = devices.first()?;
                kwin_property(name, "pointerAcceleration")
                    .ok()?
                    .parse()
                    .ok()
            }),
            // KDE defaults when the keys are absent.
            double_click_ms: Some(kde_read("KDE", "DoubleClickInterval").unwrap_or(400)),
            scroll_lines: Some(kde_read("KDE", "WheelScrollLines").unwrap_or(3)),
        },
        Desktop::Gnome => Settings {
            desktop,
            // gsettings may prefix a type, as in "int32 400".
            pointer: gsettings("speed").and_then(|v| v.split_whitespace().last()?.parse().ok()),
            double_click_ms: gsettings("double-click")
                .and_then(|v| v.split_whitespace().last()?.parse().ok()),
            scroll_lines: None,
        },
        Desktop::Other => Settings {
            desktop,
            pointer: None,
            double_click_ms: None,
            scroll_lines: None,
        },
    }
}

pub fn write(settings: &Settings) -> Result<String, String> {
    match settings.desktop {
        Desktop::Kde => {
            if let Some(speed) = settings.pointer {
                let speed = format!("{:.2}", speed.clamp(-1.0, 1.0));
                let devices = kwin_devices()?;
                if devices.is_empty() {
                    return Err("KWin 中没有找到 A90".into());
                }
                for (name, label) in &devices {
                    let path = format!("/org/kde/KWin/InputDevice/{name}");
                    run(
                        "busctl",
                        &[
                            "--user",
                            "set-property",
                            KWIN,
                            &path,
                            DEVICE,
                            "pointerAcceleration",
                            "d",
                            &speed,
                        ],
                    )?;
                    let (vendor, product) = (VENDOR.to_string(), PRODUCT.to_string());
                    run(
                        "kwriteconfig6",
                        &[
                            "--file",
                            "kcminputrc",
                            "--group",
                            "Libinput",
                            "--group",
                            &vendor,
                            "--group",
                            &product,
                            "--group",
                            label,
                            "--key",
                            "PointerAcceleration",
                            &speed,
                        ],
                    )?;
                }
            }
            for (key, value) in [
                ("DoubleClickInterval", settings.double_click_ms),
                ("WheelScrollLines", settings.scroll_lines),
            ] {
                if let Some(value) = value {
                    run(
                        "kwriteconfig6",
                        &[
                            "--file",
                            "kdeglobals",
                            "--group",
                            "KDE",
                            "--key",
                            key,
                            &value.to_string(),
                        ],
                    )?;
                }
            }
            Ok("已写入 KDE 设置。指针速度立即生效，双击和滚动行数在程序重新启动后生效。".into())
        }
        Desktop::Gnome => {
            let schema = "org.gnome.desktop.peripherals.mouse";
            if let Some(speed) = settings.pointer {
                run(
                    "gsettings",
                    &[
                        "set",
                        schema,
                        "speed",
                        &format!("{:.2}", speed.clamp(-1.0, 1.0)),
                    ],
                )?;
            }
            if let Some(ms) = settings.double_click_ms {
                run(
                    "gsettings",
                    &["set", schema, "double-click", &ms.to_string()],
                )?;
            }
            Ok("已写入 GNOME 设置。GNOME 没有滚动行数设置。".into())
        }
        Desktop::Other => Err("当前桌面不受支持。请在桌面的鼠标设置中调整。".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn busctl_values() {
        assert_eq!(busctl_value("d 0.5"), "0.5");
        assert_eq!(busctl_value("u 2522"), "2522");
        assert_eq!(
            busctl_value("s \"COMPANY  USB Device \""),
            "COMPANY  USB Device "
        );
        assert_eq!(busctl_value("400"), "400");
    }

    #[test]
    #[ignore = "requires A90_TEST_KDE_HOST=1, KDE Wayland, Qt6Widgets development files; temporarily writes and restores desktop settings"]
    fn kde_settings_write_read_and_restore() {
        assert_eq!(std::env::var("A90_TEST_KDE_HOST").as_deref(), Ok("1"));
        assert_eq!(desktop(), Desktop::Kde);
        assert!(std::env::var_os("WAYLAND_DISPLAY").is_some());
        let devices = kwin_devices().unwrap();
        assert!(!devices.is_empty(), "KWin has no A90 pointer");
        struct Entry {
            file: &'static str,
            groups: Vec<String>,
            key: &'static str,
            original: Option<String>,
            proposed: String,
            restored_acceleration: Option<f64>,
        }
        impl Entry {
            fn read(&self) -> Result<Option<String>, String> {
                let mut args = vec!["--file", self.file];
                for group in &self.groups {
                    args.extend(["--group", group]);
                }
                args.extend(["--key", self.key, "--default", "A90_TEST_ABSENT"]);
                let value = run("kreadconfig6", &args)?;
                Ok((value != "A90_TEST_ABSENT").then_some(value))
            }

            fn restore(&self) -> Result<(), String> {
                let current = self.read()?;
                if current == self.original {
                    return Ok(());
                }
                let runtime_write = self.restored_acceleration.is_some_and(|original| {
                    current
                        .as_deref()
                        .and_then(|value| value.parse::<f64>().ok())
                        .is_some_and(|value| (value - original).abs() < 0.000001)
                });
                if current.as_deref() != Some(&self.proposed) && !runtime_write {
                    return Err(format!(
                        "{} {} changed during the test",
                        self.file, self.key
                    ));
                }
                let mut args = vec!["--file", self.file];
                for group in &self.groups {
                    args.extend(["--group", group]);
                }
                args.extend(["--key", self.key]);
                if let Some(original) = &self.original {
                    args.push(original);
                } else {
                    args.push("--delete");
                }
                run("kwriteconfig6", &args).map(|_| ())
            }
        }
        struct Restore {
            runtime: Vec<(String, String)>,
            entries: Vec<Entry>,
            acceleration: f64,
            armed: bool,
        }
        impl Restore {
            fn restore(&mut self) -> Result<(), String> {
                let mut errors = Vec::new();
                for (device, original) in &self.runtime {
                    let result = (|| {
                        let current = kwin_property(device, "pointerAcceleration")?;
                        if current == *original {
                            return Ok(());
                        }
                        if current
                            .parse::<f64>()
                            .ok()
                            .is_none_or(|value| (value - self.acceleration).abs() > 0.000001)
                        {
                            return Err("pointer acceleration changed during the test".into());
                        }
                        run(
                            "busctl",
                            &[
                                "--user",
                                "set-property",
                                KWIN,
                                &format!("/org/kde/KWin/InputDevice/{device}"),
                                DEVICE,
                                "pointerAcceleration",
                                "d",
                                original,
                            ],
                        )
                        .map(|_| ())
                    })();
                    if let Err(error) = result {
                        errors.push(error);
                    }
                }
                for entry in &self.entries {
                    if let Err(error) = entry.restore() {
                        errors.push(error);
                    }
                }
                if errors.is_empty() {
                    Ok(())
                } else {
                    Err(errors.join("; "))
                }
            }
        }
        impl Drop for Restore {
            fn drop(&mut self) {
                if self.armed
                    && let Err(error) = self.restore()
                {
                    eprintln!("KDE test restore failed: {error}");
                }
            }
        }
        let original = read();
        let proposed = Settings {
            desktop: Desktop::Kde,
            pointer: Some(if original.pointer == Some(0.15) {
                0.0
            } else {
                0.15
            }),
            double_click_ms: Some(if original.double_click_ms == Some(420) {
                400
            } else {
                420
            }),
            scroll_lines: Some(if original.scroll_lines == Some(4) {
                3
            } else {
                4
            }),
        };
        let mut entries = vec![];
        let mut runtime = vec![];
        for (device, label) in devices {
            runtime.push((
                device.clone(),
                kwin_property(&device, "pointerAcceleration").unwrap(),
            ));
            entries.push(Entry {
                file: "kcminputrc",
                groups: vec![
                    "Libinput".into(),
                    VENDOR.to_string(),
                    PRODUCT.to_string(),
                    label,
                ],
                key: "PointerAcceleration",
                original: None,
                proposed: format!("{:.2}", proposed.pointer.unwrap()),
                restored_acceleration: runtime.last().unwrap().1.parse().ok(),
            });
        }
        for (key, value) in [
            ("DoubleClickInterval", proposed.double_click_ms.unwrap()),
            ("WheelScrollLines", proposed.scroll_lines.unwrap()),
        ] {
            entries.push(Entry {
                file: "kdeglobals",
                groups: vec!["KDE".into()],
                key,
                original: None,
                proposed: value.to_string(),
                restored_acceleration: None,
            });
        }
        for entry in &mut entries {
            entry.original = entry.read().unwrap();
        }
        let mut restore = Restore {
            runtime,
            entries,
            acceleration: proposed.pointer.unwrap(),
            armed: true,
        };
        // Compile the independent Qt reader before changing any desktop value.
        let executable =
            std::env::temp_dir().join(format!("a90-desktop-settings-{}", std::process::id()));
        struct Probe(std::path::PathBuf);
        impl Drop for Probe {
            fn drop(&mut self) {
                let _ = std::fs::remove_file(&self.0);
            }
        }
        let probe = Probe(executable);
        let flags = run("pkg-config", &["--cflags", "--libs", "Qt6Widgets"]).unwrap();
        let compiled = Command::new("c++")
            .args(["-std=c++17", "-Wall", "-Wextra", "-Werror", "-fPIC"])
            .arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/desktop_settings.cpp"
            ))
            .args(flags.split_whitespace())
            .arg("-o")
            .arg(&probe.0)
            .output()
            .unwrap();
        assert!(
            compiled.status.success(),
            "{}",
            String::from_utf8_lossy(&compiled.stderr)
        );
        write(&proposed).unwrap();
        assert_eq!(read(), proposed);
        for entry in &restore.entries {
            assert_eq!(
                entry.read().unwrap().as_deref(),
                Some(entry.proposed.as_str())
            );
        }
        let values = run(probe.0.to_str().unwrap(), &[]).unwrap();
        assert_eq!(
            values,
            format!(
                "{} {}",
                proposed.double_click_ms.unwrap(),
                proposed.scroll_lines.unwrap()
            )
        );
        restore.restore().unwrap();
        restore.armed = false;
        assert_eq!(read(), original);
        for entry in &restore.entries {
            assert_eq!(entry.read().unwrap(), entry.original);
        }
        for (device, original) in &restore.runtime {
            assert_eq!(
                kwin_property(device, "pointerAcceleration").unwrap(),
                *original
            );
        }
    }
}
