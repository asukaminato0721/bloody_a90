//! A persistent KWin timer supplies pointer-window geometry. D-Bus callbacks
//! update a bounded cache. The evdev thread never makes a desktop call.
use super::Settings;
use std::{
    fs,
    io::Write,
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::PathBuf,
    sync::{Arc, Mutex, mpsc},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use zbus::{
    blocking::{Connection, Proxy},
    message::Header,
};

const PATH: &str = "/org/bloody/A90/Wheel";
const INTERFACE: &str = "org.bloody.A90.Wheel";
type Edits = Arc<Mutex<Option<mpsc::SyncSender<(String, u16)>>>>;

#[derive(Clone, Debug)]
struct Sample {
    received: Instant,
    point: [f64; 2],
    rect: Option<[f64; 4]>,
    app: String,
    error: String,
}

struct Reply {
    owner: String,
    sample: Arc<Mutex<Option<Sample>>>,
    ready: mpsc::SyncSender<()>,
    direction: Arc<Mutex<super::divider::Live>>,
    direction_ready: mpsc::SyncSender<()>,
    divider_ready: mpsc::SyncSender<()>,
    edits: Edits,
}

#[zbus::interface(name = "org.bloody.A90.Wheel")]
impl Reply {
    fn divider_frame(
        &self,
        #[zbus(header)] header: Header<'_>,
    ) -> (bool, f64, f64, f64, f64, String, u16) {
        let hidden = (false, 0.0, 0.0, 0.0, 0.0, String::new(), 900);
        if !header.sender().is_some_and(|s| s.as_str() == self.owner) {
            return hidden;
        }
        let _ = self.divider_ready.try_send(());
        let Ok(edits) = self.edits.lock() else {
            return hidden;
        };
        if edits.is_none() {
            return hidden;
        }
        drop(edits);
        let Ok(config) = self.direction.lock() else {
            return hidden;
        };
        let Some(settings) = config.settings.as_ref() else {
            return hidden;
        };
        if !settings.divider_icon || config.error.is_some() {
            return hidden;
        }
        let Ok(sample) = self.sample.lock() else {
            return hidden;
        };
        let Some(sample) = sample.as_ref() else {
            return hidden;
        };
        if sample.received.elapsed() > Duration::from_millis(600)
            || !sample.error.is_empty()
            || sample.app.is_empty()
        {
            return hidden;
        }
        let Some([left, top, width, height]) = sample.rect else {
            return hidden;
        };
        let divider = config.divider(settings, &sample.app);
        let [x, y, _, _] = super::divider::bounds([left, top, width, height], divider);
        (true, x, y, left, width, sample.app.clone(), divider)
    }

    fn update_divider(&self, app: &str, divider: u16, #[zbus(header)] header: Header<'_>) -> bool {
        if !header.sender().is_some_and(|s| s.as_str() == self.owner)
            || !(100..=900).contains(&divider)
            || app.trim().is_empty()
            || app.len() > 512
            || app.chars().any(char::is_control)
        {
            return false;
        }
        let Ok(edits) = self.edits.lock() else {
            return false;
        };
        let Some(sender) = edits.as_ref() else {
            return false;
        };
        match sender.try_send((app.into(), divider)) {
            Ok(()) => true,
            Err(_) => {
                if let Ok(mut live) = self.direction.lock() {
                    live.error = Some("滚轮分界保存队列不可用，请重新启用区域滚动".into());
                }
                false
            }
        }
    }

    fn direction_frame(&self, #[zbus(header)] header: Header<'_>) -> String {
        if !header.sender().is_some_and(|s| s.as_str() == self.owner) {
            return String::new();
        }
        let _ = self.direction_ready.try_send(());
        let Ok(config) = self.direction.lock() else {
            return String::new();
        };
        let Some(settings) = config.settings.as_ref() else {
            return String::new();
        };
        let Ok(sample) = self.sample.lock() else {
            return String::new();
        };
        let Some(sample) = sample.as_ref() else {
            return String::new();
        };
        if sample.received.elapsed() > Duration::from_millis(600) || !sample.error.is_empty() {
            return String::new();
        }
        if settings.direction_icon
            && sample.rect.is_some_and(|r| {
                settings.horizontal_at(sample.point, r, config.divider(settings, &sample.app))
            })
        {
            format!("{},{}", sample.point[0], sample.point[1])
        } else {
            String::new()
        }
    }

    fn sample(
        &self,
        coordinates: &str,
        app: &str,
        error: &str,
        #[zbus(header)] header: Header<'_>,
    ) -> bool {
        if !header.sender().is_some_and(|s| s.as_str() == self.owner) {
            return false;
        }
        let values = coordinates
            .split(',')
            .map(str::parse::<f64>)
            .collect::<Result<Vec<_>, _>>()
            .unwrap_or_default();
        let sample = match values.as_slice() {
            [x, y] => Sample {
                received: Instant::now(),
                point: [*x, *y],
                rect: None,
                app: String::new(),
                error: error.into(),
            },
            [x, y, left, top, width, height] if values.iter().all(|v| v.is_finite()) => Sample {
                received: Instant::now(),
                point: [*x, *y],
                rect: Some([*left, *top, *width, *height]),
                app: app.chars().take(512).collect(),
                error: error.into(),
            },
            _ => Sample {
                received: Instant::now(),
                point: [0.0; 2],
                rect: None,
                app: String::new(),
                error: "窗口区域数据无效".into(),
            },
        };
        if let Ok(mut current) = self.sample.lock() {
            *current = Some(sample);
        }
        let _ = self.ready.try_send(());
        true
    }
}

pub(crate) struct Monitor {
    bus: Connection,
    plugin: String,
    directory: PathBuf,
    file: PathBuf,
    sample: Arc<Mutex<Option<Sample>>>,
    direction: Arc<Mutex<super::divider::Live>>,
    direction_started: mpsc::Receiver<()>,
    icon: Option<super::direction::Icon>,
    divider_icon: Option<super::direction::Icon>,
    divider_started: mpsc::Receiver<()>,
    edits: Edits,
    store: Option<super::divider::Store>,
}

impl Monitor {
    pub(crate) fn observer(&self) -> super::Observer {
        super::Observer(self.direction.clone())
    }

    pub(crate) fn start_with_direction(settings: &Settings) -> Result<Self, String> {
        let mut monitor = Self::start()?;
        monitor
            .direction
            .lock()
            .map_err(|_| "滚轮图标设置不可用")?
            .settings = Some(settings.clone());
        if settings.direction_icon {
            monitor
                .direction
                .lock()
                .map_err(|_| "方向图标设置不可用")?
                .settings = Some(settings.clone());
            monitor.icon = Some(super::direction::Icon::start(
                monitor.bus.clone(),
                &monitor.directory,
                &monitor.plugin,
            )?);
            monitor
                .direction_started
                .recv_timeout(Duration::from_secs(2))
                .map_err(|_| "滚轮方向图标未启动，请关闭图标后重试")?;
        }
        if settings.divider_icon {
            monitor.divider_icon = Some(super::direction::Icon::start_kind(
                monitor.bus.clone(),
                &monitor.directory,
                &monitor.plugin,
                "divider",
                include_str!("divider.qml"),
            )?);
            monitor
                .divider_started
                .recv_timeout(Duration::from_secs(2))
                .map_err(|_| "滚轮分隔图标未启动，请关闭图标后重试")?;
        }
        Ok(monitor)
    }

    pub(crate) fn start_runtime(settings: &Settings) -> Result<Self, String> {
        let path = super::path().ok_or("无法确定滚轮设置文件路径")?;
        Self::start_editable(settings, &path)
    }

    pub(crate) fn start_editable(
        settings: &Settings,
        path: &std::path::Path,
    ) -> Result<Self, String> {
        let mut monitor = Self::start_with_direction(settings)?;
        if settings.divider_icon {
            let store = super::divider::Store::start(path, settings, monitor.direction.clone())?;
            *monitor.edits.lock().map_err(|_| "滚轮分界保存服务不可用")? = Some(store.sender());
            monitor.store = Some(store);
        }
        Ok(monitor)
    }

    pub(crate) fn start() -> Result<Self, String> {
        if std::env::var_os("WAYLAND_DISPLAY").is_none() {
            return Err("4-Way Wheel 当前需要 KDE Wayland".into());
        }
        let bus = zbus::blocking::connection::Builder::session()
            .and_then(|b| b.method_timeout(Duration::from_secs(2)).build())
            .map_err(|e| e.to_string())?;
        let dbus = Proxy::new(
            &bus,
            "org.freedesktop.DBus",
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
        )
        .map_err(|e| e.to_string())?;
        let owner: String = dbus
            .call("GetNameOwner", &"org.kde.KWin")
            .map_err(|e| format!("4-Way Wheel 需要 KWin：{e}"))?;
        drop(dbus);
        let sample = Arc::new(Mutex::new(None));
        let (ready, started) = mpsc::sync_channel(1);
        let direction = Arc::new(Mutex::new(super::divider::Live::default()));
        let (direction_ready, direction_started) = mpsc::sync_channel(1);
        let (divider_ready, divider_started) = mpsc::sync_channel(1);
        let edits = Arc::new(Mutex::new(None));
        bus.object_server()
            .at(
                PATH,
                Reply {
                    owner,
                    sample: sample.clone(),
                    ready,
                    direction: direction.clone(),
                    direction_ready,
                    divider_ready,
                    edits: edits.clone(),
                },
            )
            .map_err(|e| e.to_string())?;
        let plugin = format!(
            "a90-wheel-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|e| e.to_string())?
                .as_nanos()
        );
        let directory = std::env::temp_dir().join(&plugin);
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&directory)
            .map_err(|e| e.to_string())?;
        let monitor = Self {
            file: directory.join("wheel.js"),
            directory,
            plugin,
            bus,
            sample,
            direction,
            direction_started,
            icon: None,
            divider_icon: None,
            divider_started,
            edits,
            store: None,
        };
        let destination = monitor
            .bus
            .unique_name()
            .ok_or("滚轮服务缺少总线名称")?
            .as_str();
        // All interpolated strings are D-Bus names or constant identifiers.
        let source = format!(
            r#"
var pending = false;
var missed = 0;
function sample() {{
    if (pending) {{
        if (++missed >= 10) {{
            timer.stop();
            callDBus('org.kde.KWin', '/Scripting', 'org.kde.kwin.Scripting', 'unloadScript', '{plugin}');
        }}
        return;
    }}
    let values = [];
    let app = '';
    let error = '';
    try {{
        const p = workspace.cursorPos;
        values = [p.x, p.y];
        const windows = workspace.windowAt(p, 0).filter(w => w.caption !== '{plugin}-divider');
        if (windows.length && windows[0].normalWindow && !windows[0].minimized) {{
            const r = windows[0].frameGeometry;
            values = [p.x, p.y, r.x, r.y, r.width, r.height];
            app = String(windows[0].desktopFileName || windows[0].resourceClass || '');
        }}
    }} catch (e) {{ error = String(e); }}
    pending = true;
    callDBus('{destination}', '{PATH}', '{INTERFACE}', 'Sample', values.join(','), app, error, function() {{ pending = false; missed = 0; }});
}}
var timer = new QTimer();
timer.interval = 100;
timer.timeout.connect(sample);
timer.start();
sample();
"#,
            plugin = monitor.plugin
        );
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&monitor.file)
            .map_err(|e| e.to_string())?;
        file.write_all(source.as_bytes())
            .map_err(|e| e.to_string())?;
        drop(file);
        let scripting = Proxy::new(
            &monitor.bus,
            "org.kde.KWin",
            "/Scripting",
            "org.kde.kwin.Scripting",
        )
        .map_err(|e| e.to_string())?;
        let id: i32 = scripting
            .call(
                "loadScript",
                &(
                    monitor.file.to_str().ok_or("滚轮脚本路径不是 UTF-8")?,
                    &monitor.plugin,
                ),
            )
            .map_err(|e| e.to_string())?;
        if id < 0 {
            return Err("KWin 拒绝加载滚轮区域脚本".into());
        }
        let path = format!("/Scripting/Script{id}");
        let script = Proxy::new(
            &monitor.bus,
            "org.kde.KWin",
            path.as_str(),
            "org.kde.kwin.Script",
        )
        .map_err(|e| e.to_string())?;
        let _: () = script.call("run", &()).map_err(|e| e.to_string())?;
        started
            .recv_timeout(Duration::from_secs(2))
            .map_err(|_| "滚轮区域监视器未返回数据")?;
        monitor.horizontal(&Settings::default())?;
        drop(script);
        drop(scripting);
        Ok(monitor)
    }

    pub(crate) fn horizontal(&self, settings: &Settings) -> Result<bool, String> {
        let config = self.direction.lock().map_err(|_| "滚轮分界缓存不可用")?;
        if let Some(error) = &config.error {
            return Err(error.clone());
        }
        let sample = self.sample.lock().map_err(|_| "滚轮区域缓存不可用")?;
        let sample = sample.as_ref().ok_or("缺少滚轮区域数据")?;
        if sample.received.elapsed() > Duration::from_millis(600) {
            return Err("滚轮区域监视器已停止。输入接管已释放，请重新启用。".into());
        }
        if !sample.error.is_empty() {
            return Err(format!("无法读取滚轮区域：{}", sample.error));
        }
        Ok(sample.rect.is_some_and(|r| {
            settings.horizontal_at(sample.point, r, config.divider(settings, &sample.app))
        }))
    }

    #[cfg(test)]
    pub(crate) fn application(&self) -> String {
        self.sample.lock().unwrap().as_ref().unwrap().app.clone()
    }

    pub(crate) fn divider_hit(&self) -> bool {
        let Ok(edits) = self.edits.lock() else {
            return false;
        };
        if edits.is_none() {
            return false;
        }
        drop(edits);
        let Ok(config) = self.direction.lock() else {
            return false;
        };
        let Some(settings) = config.settings.as_ref() else {
            return false;
        };
        let Ok(sample) = self.sample.lock() else {
            return false;
        };
        let Some(sample) = sample.as_ref() else {
            return false;
        };
        let Some(rect) = sample.rect else {
            return false;
        };
        if !settings.divider_icon || sample.received.elapsed() > Duration::from_millis(600) {
            return false;
        }
        let [x, y, w, h] = super::divider::bounds(rect, config.divider(settings, &sample.app));
        sample.point[0] >= x
            && sample.point[0] < x + w
            && sample.point[1] >= y
            && sample.point[1] < y + h
    }
}

impl Drop for Monitor {
    fn drop(&mut self) {
        drop(self.icon.take());
        drop(self.divider_icon.take());
        if let Ok(mut edits) = self.edits.lock() {
            edits.take();
        }
        drop(self.store.take());
        if let Ok(proxy) = Proxy::new(
            &self.bus,
            "org.kde.KWin",
            "/Scripting",
            "org.kde.kwin.Scripting",
        ) {
            let _: Result<bool, _> = proxy.call("unloadScript", &self.plugin);
        }
        let _ = fs::remove_file(&self.file);
        let _ = fs::remove_dir(&self.directory);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires A90_TEST_KWIN_ISOLATED=1 and private KWin"]
    fn isolated_kwin_wheel_monitor_timer_and_cleanup() {
        assert_eq!(std::env::var("A90_TEST_KWIN_ISOLATED").as_deref(), Ok("1"));
        let monitor = Monitor::start_with_direction(&Settings::default()).unwrap();
        assert!(monitor.icon.as_ref().unwrap().loaded());
        let directory = monitor.directory.clone();
        let plugin = monitor.plugin.clone();
        let bus = monitor.bus.clone();
        let first = monitor.sample.lock().unwrap().as_ref().unwrap().received;
        let until = Instant::now() + Duration::from_secs(2);
        loop {
            assert!(
                Instant::now() < until,
                "KWin timer did not update the cache"
            );
            if monitor.sample.lock().unwrap().as_ref().unwrap().received > first {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        drop(monitor);
        assert!(!directory.exists());
        let scripting =
            Proxy::new(&bus, "org.kde.KWin", "/Scripting", "org.kde.kwin.Scripting").unwrap();
        let loaded: bool = scripting.call("isScriptLoaded", &plugin).unwrap();
        assert!(!loaded);
        // Simulate a vanished callback endpoint. Stale geometry must fail,
        // and the KWin timer must unload itself after its callback lease ends.
        let abandoned = Monitor::start_with_direction(&Settings::default()).unwrap();
        abandoned
            .bus
            .object_server()
            .remove::<Reply, _>(PATH)
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            let loaded: bool = scripting.call("isScriptLoaded", &abandoned.plugin).unwrap();
            if !loaded && !abandoned.icon.as_ref().unwrap().loaded() {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "abandoned wheel script remained loaded"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            abandoned
                .horizontal(&Settings::default())
                .unwrap_err()
                .contains("已停止")
        );
        let disabled = Monitor::start_with_direction(&Settings {
            direction_icon: false,
            ..Default::default()
        })
        .unwrap();
        assert!(disabled.icon.is_none());
    }
}
