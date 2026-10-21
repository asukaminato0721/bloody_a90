mod app;
mod skin;
mod worker;

use bloody_a90::{
    backup::Backup,
    device::{self, Device},
    macros::{self, Repeat},
    sensitivity,
};
use eframe::egui;

fn main() -> eframe::Result {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let smoke_test = args == ["--smoke-test"];
    if !args.is_empty() && !smoke_test {
        if let Err(error) = cli(&args) {
            eprintln!("{error}");
            std::process::exit(1);
        }
        return Ok(());
    }
    // The App outlives each window. Closing to the tray ends eframe's run;
    // the tray's 开启 starts a new one on the same App.
    let zoom = app::zoom_for(bloody_a90::screen::logical_size());
    let shared: std::rc::Rc<std::cell::RefCell<Option<std::rc::Rc<std::cell::RefCell<app::App>>>>> =
        Default::default();
    loop {
        let slot = shared.clone();
        eframe::run_native(
            "Bloody A90",
            eframe::NativeOptions {
                viewport: egui::ViewportBuilder::default()
                    .with_inner_size(app::WINDOW * zoom)
                    .with_min_inner_size(app::MINIMUM * zoom),
                ..Default::default()
            },
            Box::new(move |cc| {
                cc.egui_ctx.set_zoom_factor(zoom);
                let mut slot = slot.borrow_mut();
                let app = match slot.as_ref() {
                    Some(app) => {
                        app.borrow_mut().attach(&cc.egui_ctx);
                        app.clone()
                    }
                    None => {
                        let app = app::App::new(cc);
                        let app = if smoke_test {
                            app.smoke_test()
                        } else {
                            app.connect_at_start()
                        };
                        slot.insert(std::rc::Rc::new(std::cell::RefCell::new(app)))
                            .clone()
                    }
                };
                Ok(Box::new(app::Window(app)))
            }),
        )?;
        let Some(app) = shared.borrow().clone() else {
            return Ok(());
        };
        let mut app = app.borrow_mut();
        if !app.stays_in_tray() || !app.run_in_tray() {
            return Ok(());
        }
    }
}

const HELP: &str = "\
bloody_a90                              Open the configuration editor
bloody_a90 --inspect BACKUP             Inspect an existing 32768-byte backup
bloody_a90 --compile SOURCE             Compile plain macro text and print words
bloody_a90 --amc FILE                   Compile an official AMC macro file
bloody_a90 --device                     Show identity, memory map and brightness
bloody_a90 --device-backup PATH         Read the complete flash into a new file
bloody_a90 --brightness 0-3             Set and read back the LED brightness
bloody_a90 --ram ADDRESS COUNT          Read RAM bytes (hexadecimal address)
bloody_a90 --runtime-sensitivity        Show running DPI and report-rate parameters
bloody_a90 --save-dpi LIST SELECTED     Save presets, e.g. 800,1600:1200 2
bloody_a90 --watch-buttons SECONDS      Print button numbers while they are held
bloody_a90 --watch-native-input SECONDS Show vendor button, wheel and selection notifications
bloody_a90 --save-rate HZ                Save 125, 250, 500 or 1000 Hz
bloody_a90 --save-lift LEVEL             Save lift cutoff level 1-16, or off
bloody_a90 --save-debounce MS            Save button response time 0-16 ms
bloody_a90 --calibrate-lift SECONDS      Measure lift level for 1-30 seconds
bloody_a90 --select-profile 1-7          Select a running profile without a flash write
bloody_a90 --foreground                 Show the foreground application identifier
bloody_a90 --input-info                 Show the A90 pointer input interface
bloody_a90 --set-key BUTTON KEY         Map a button (1-24) to a keyboard key in every mode
bloody_a90 --set-macro BUTTON SOURCE [once|held|toggle]
                                        Map a button to an M text macro in every mode
bloody_a90 --interrupt                  Stop running macros (keyboard LED report 07)
bloody_a90 --host                       Show desktop pointer, double-click and wheel settings
bloody_a90 --light-effect N             Set the RGB effect 0-6 of every profile and reload
bloody_a90 --save-buttons               Rebuild the active button table and save it
bloody_a90 --restore BACKUP             Restore writable pages that differ from a backup";

fn write_new(path: &str, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| format!("无法创建 {path}：{e}"))?;
    file.write_all(bytes).map_err(|e| e.to_string())
}

fn presets(list: &str) -> Result<Vec<[u32; 2]>, String> {
    list.split(',')
        .map(|item| {
            let mut axes = item.split(':').map(|value| {
                value
                    .trim()
                    .parse::<u32>()
                    .map_err(|_| format!("无效的 DPI：{item}"))
            });
            let x = axes.next().ok_or("缺少 DPI")??;
            let y = axes.next().transpose()?.unwrap_or(x);
            Ok([x, y])
        })
        .collect()
}

fn cli(args: &[String]) -> Result<(), String> {
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match args[..] {
        ["--help"] => println!("{HELP}"),
        ["--input-info"] => {
            let info = bloody_a90::linux_input::find()?;
            let source = bloody_a90::linux_input::Source::open(&info.path)?;
            println!("{} · {}", info.path.display(), info.name);
            println!("identity={:?}", info.identity);
            println!("keys={:?}", source.capabilities(1, 767)?);
            println!("axes={:?}", source.capabilities(2, 15)?);
            println!("held={:?}", source.held()?);
        }
        ["--foreground"] => {
            let mut monitor = bloody_a90::foreground::Monitor::connect()?;
            println!(
                "{}",
                monitor.current()?.as_deref().unwrap_or("(无前台应用)")
            );
        }
        ["--select-profile", index] => {
            let index: u8 = index.parse().map_err(|_| "配置序号应为 1 至 7")?;
            if !(1..=7).contains(&index) {
                return Err("配置序号应为 1 至 7".into());
            }
            let (current, maximum) = Device::open()?.select_profile(index - 1)?;
            println!("当前配置 {} / {}", current + 1, maximum + 1);
        }
        ["--inspect", path] => println!("{}", Backup::read(path)?.summary()),
        ["--compile", path] => {
            let source = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
            let program = macros::compile(&source, "", Repeat::Once)?;
            println!(
                "press: {}\nrelease: {}",
                macros::hex(&program.press),
                macros::hex(&program.release)
            );
        }
        ["--amc", path] => {
            let bytes = std::fs::read(path).map_err(|e| format!("无法读取 {path}：{e}"))?;
            let program = bloody_a90::amc::parse(&bytes)?.compile()?;
            println!(
                "press: {}\nrelease: {}",
                macros::hex(&program.press),
                macros::hex(&program.release)
            );
        }
        ["--device"] => {
            let device = Device::open()?;
            let identity = device.identity()?;
            let info = device.memory_info()?;
            println!(
                "{} · 型号 {:04x} · 固件 {:02x} · 核心 {}\n回报率 {} Hz · 灵敏度 {:04x} · 按键 {:04x} · 设置 {:04x}\n出厂灵敏度 {:04x} · 出厂按键 {:04x} · 设备数据 {:04x}\n亮度 {}",
                device.path().display(),
                identity.model,
                identity.firmware,
                identity.core,
                info.rate_hz,
                info.sensitivity,
                info.buttons,
                info.settings,
                info.factory_sensitivity,
                info.factory_buttons,
                info.device_data,
                device.brightness()?
            );
        }
        ["--device-backup", path] => {
            let bytes = Device::open()?.read_backup(|done, total| {
                if done % 2800 < 28 || done == total {
                    eprintln!("{done} / {total}");
                }
            })?;
            Backup::parse(&bytes)?;
            write_new(path, &bytes)?;
            println!("已保存 {} 字节到 {path}", bytes.len());
        }
        ["--brightness", level] => {
            let level = level.parse().map_err(|_| "亮度应为 0 至 3")?;
            Device::open()?.set_brightness(level)?;
            println!("亮度已设为 {level}");
        }
        ["--ram", address, count] => {
            let address = u16::from_str_radix(address, 16).map_err(|_| "无效地址")?;
            let count: u16 = count.parse().map_err(|_| "无效数量")?;
            let device = Device::open()?;
            let bytes = (0..count)
                .map(|offset| device.ram(address + offset).map(|b| format!("{b:02x}")))
                .collect::<Result<Vec<_>, _>>()?;
            println!("{address:04x}: {}", bytes.join(" "));
        }
        ["--runtime-sensitivity"] => {
            let runtime = Device::open()?.runtime_sensitivity()?;
            println!(
                "当前档位 {} / {} · X 比例 {} · Y 比例 {} · 传感器编码 {:02x} · 回报率 {} Hz",
                runtime.selected + 1,
                runtime.count,
                runtime.record.x_scale,
                runtime.record.y_scale,
                runtime.record.sensor_code,
                1000 / runtime.rate_code as u32
            );
        }
        ["--save-dpi", list, selected] => {
            let selected: usize = selected.parse().map_err(|_| "无效的当前档位")?;
            let device = Device::open()?;
            let backup = Backup::parse(&device.read_backup(|_, _| {})?)?;
            let template = backup.sensitivity_template()?;
            // "factory" keeps the template records and only adds the saved header.
            let block = if list == "factory" {
                let mut words = template;
                words[5] = selected.wrapping_sub(1) as u16;
                if words[5] > words[4] {
                    return Err("当前档位超出启用档位".into());
                }
                sensitivity::finish(words)
            } else {
                sensitivity::block(&template, &presets(list)?, selected.wrapping_sub(1), None)?
            };
            println!("写入 {}", macros::hex(&block[..30]));
            device.save_sensitivity(&block)?;
            println!("灵敏度块已保存并立即应用，运行参数已读回一致");
        }
        ["--watch-native-input", seconds] => {
            let seconds: u64 = seconds.parse().map_err(|_| "无效秒数")?;
            if !(1..=300).contains(&seconds) {
                return Err("监听时间应为 1 至 300 秒".into());
            }
            let device = Device::open()?;
            let mut source = bloody_a90::vendor_input::Session::start(&device)?;
            let end = std::time::Instant::now() + std::time::Duration::from_secs(seconds);
            let mut last = None;
            while std::time::Instant::now() < end {
                for state in source.read()? {
                    if last != Some(state) || state.wheel != 0 {
                        println!(
                            "按住：{:?} · 滚轮 {} · 配置 {} · DPI 档位 {}",
                            state.held_buttons(),
                            state.wheel,
                            state.profile + 1,
                            state.sensitivity + 1
                        );
                    }
                    last = Some(state);
                }
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
            source.restore()?;
        }
        ["--watch-buttons", seconds] => {
            let seconds: u64 = seconds.parse().map_err(|_| "无效秒数")?;
            let device = Device::open()?;
            let end = std::time::Instant::now() + std::time::Duration::from_secs(seconds);
            let mut last = Vec::new();
            while std::time::Instant::now() < end {
                let held = device.held_buttons()?;
                if held != last {
                    println!("按住：{held:?}");
                    last = held;
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
        }
        ["--save-rate", hz] => {
            let hz: u32 = hz.parse().map_err(|_| "无效回报率")?;
            let device = Device::open()?;
            let backup = Backup::parse(&device.read_backup(|_, _| {})?)?;
            let block = sensitivity::with_rate(&backup.sensitivity_template()?, hz)?;
            device.save_rate(&block)?;
            println!("回报率 {hz} Hz 已保存并立即应用，运行参数已读回一致");
        }
        ["--save-lift", level] => {
            let level = if level == "off" {
                None
            } else {
                Some(
                    level
                        .parse::<u8>()
                        .map_err(|_| "抬起高度应为 1 至 16 级，或 off")?,
                )
            };
            let device = Device::open()?;
            let backup = Backup::parse(&device.read_backup(|_, _| {})?)?;
            let block = sensitivity::with_lift_height(&backup.sensitivity_template()?, level)?;
            device.save_lift(&block)?;
            println!("抬起高度已保存。运行值已读回一致。");
        }
        ["--calibrate-lift", seconds] => {
            let seconds = seconds.parse().map_err(|_| "校准时间应为 1 至 30 秒")?;
            let device = Device::open()?;
            eprintln!("请在使用表面上持续移动鼠标，直到校准结束。");
            let level = device.calibrate_lift(seconds)?;
            println!("校准结果：{level} 级。使用 --save-lift {level} 保存并应用。");
        }
        ["--save-debounce", ms] => {
            let ms: u8 = ms.parse().map_err(|_| "硬件按键响应时间应为 0 至 16 ms")?;
            let device = Device::open()?;
            let backup = Backup::parse(&device.read_backup(|_, _| {})?)?;
            let mut config = backup.configuration()?;
            config.set_debounce_ms(ms)?;
            device.save(device::BUTTONS, &bloody_a90::configuration::build(&config)?)?;
            println!("硬件按键响应时间 {ms} ms 已保存并读回一致");
        }
        ["--set-key", button, key] => {
            let button: usize = button
                .parse()
                .ok()
                .filter(|b| (1..=24).contains(b))
                .ok_or("按键编号应为 1 至 24")?;
            let usage = bloody_a90::highlanguage::key(key).ok_or("未知按键名称")?;
            let (press, release) = if (0xe0..=0xe7).contains(&usage) {
                (0x2300 | 1 << (usage - 0xe0), 0x2200 | 1 << (usage - 0xe0))
            } else {
                (0x2100 | usage, 0x2000 | usage)
            };
            let device = Device::open()?;
            let backup = Backup::parse(&device.read_backup(|_, _| {})?)?;
            let mut config = backup.configuration()?;
            for profile in &mut config.profiles {
                profile.buttons[button - 1] = bloody_a90::configuration::Binding {
                    press: bloody_a90::configuration::Action::Direct(press),
                    release: bloody_a90::configuration::Action::Direct(release),
                };
            }
            device.save(device::BUTTONS, &bloody_a90::configuration::build(&config)?)?;
            println!("按键 {button} 已设为 {key}，写入并读回一致");
        }
        ["--set-macro", button, path, ..] if args.len() <= 4 => {
            let repeat = match args.get(3).copied() {
                None | Some("once") => Repeat::Once,
                Some("held") => Repeat::Held,
                Some("toggle") => Repeat::Toggle,
                Some(_) => return Err("重复方式应为 once、held 或 toggle".into()),
            };
            let button: usize = button
                .parse()
                .ok()
                .filter(|b| (1..=24).contains(b))
                .ok_or("按键编号应为 1 至 24")?;
            let source = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
            let program = macros::compile(&source, "", repeat)?;
            let device = Device::open()?;
            let backup = Backup::parse(&device.read_backup(|_, _| {})?)?;
            let mut config = backup.configuration()?;
            for profile in &mut config.profiles {
                profile.buttons[button - 1] = bloody_a90::configuration::Binding {
                    press: bloody_a90::configuration::Action::from_program(program.press.clone()),
                    release: bloody_a90::configuration::Action::from_program(
                        program.release.clone(),
                    ),
                };
            }
            device.save(device::BUTTONS, &bloody_a90::configuration::build(&config)?)?;
            println!(
                "按键 {button} 已设为宏（{} 字），写入并读回一致",
                program.press.len()
            );
        }
        ["--interrupt"] => {
            device::interrupt_macros()?;
            println!("已发送中断信号");
        }
        ["--host"] => println!("{:?}", bloody_a90::host::read()),
        ["--light-effect", effect] => {
            let effect: u8 = effect.parse().map_err(|_| "灯效编号应为 0 至 6")?;
            let device = Device::open()?;
            let backup = Backup::parse(&device.read_backup(|_, _| {})?)?;
            let mut config = backup.configuration()?;
            config.set_light_effect(effect)?;
            device.save(device::BUTTONS, &bloody_a90::configuration::build(&config)?)?;
            let (current, _) = device.runtime_profile()?;
            device.select_profile(current)?;
            println!("灯效已保存并应用");
        }
        ["--save-buttons"] => {
            let device = Device::open()?;
            let backup = Backup::parse(&device.read_backup(|_, _| {})?)?;
            let words = bloody_a90::configuration::build(&backup.configuration()?)?;
            println!("写入 {} 个字", words.len());
            device.save(device::BUTTONS, &words)?;
            println!("按键表已保存并读回一致");
        }
        ["--restore", path] => {
            let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
            let backup = Backup::parse(&bytes)?;
            let words: Vec<u16> = bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|pair| u16::from_le_bytes(*pair))
                .collect();
            let device = Device::open()?;
            let current = device.read_flash(0, words.len(), |_, _| {})?;
            let pages = std::iter::once(device::SENSITIVITY)
                .chain((device::BUTTONS..device::BUTTONS_END).step_by(0x100));
            let mut restored = 0;
            for page in pages {
                let range = page..page + 0x100;
                if current[range.clone()] != words[range.clone()] {
                    device.restore(page, &words[range])?;
                    println!("已恢复字地址 {page:04x} 至 {:04x}", page + 0xff);
                    restored += 1;
                }
            }
            if let Ok(config) = backup.configuration()
                && let Some(ms) = config.debounce_ms()
            {
                device.set_debounce_ms(ms)?;
            }
            if let Ok(template) = backup.sensitivity_template() {
                device.apply_sensitivity(&template)?;
            }
            println!("恢复了 {restored} 页");
        }
        _ => return Err("Use --help to see the command syntax".into()),
    }
    Ok(())
}
