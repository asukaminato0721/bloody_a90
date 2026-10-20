//! Divider changes use a separate file worker. Input routing never waits for IO.
use super::{Application, Settings};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        mpsc::{self, SyncSender},
    },
    thread::{self, JoinHandle},
};

#[derive(Default)]
pub(super) struct Live {
    pub settings: Option<Settings>,
    pub changed: BTreeMap<String, u16>,
    pub error: Option<String>,
}

#[derive(Clone)]
pub(crate) struct Observer(pub(super) Arc<Mutex<Live>>);

impl Observer {
    pub(crate) fn settings(&self) -> Option<Settings> {
        let live = self.0.lock().ok()?;
        if live.changed.is_empty() {
            None
        } else {
            live.settings.clone()
        }
    }
}

pub(super) fn bounds(rect: [f64; 4], divider: u16) -> [f64; 4] {
    let [left, top, width, height] = rect;
    [
        left + width * f64::from(divider) / 1000.0 - 16.0,
        top.max((top + 24.0).min(top + height - 24.0)),
        32.0,
        24.0,
    ]
}

impl Live {
    pub fn divider(&self, fallback: &Settings, app: &str) -> u16 {
        self.changed
            .get(&app.to_ascii_lowercase())
            .copied()
            .unwrap_or_else(|| fallback.divider_for(app))
    }
}

pub(super) struct Store {
    sender: Option<SyncSender<(String, u16)>>,
    thread: Option<JoinHandle<()>>,
}

fn update(settings: &mut Settings, app: &str, divider: u16) -> Result<(), String> {
    if let Some(rule) = settings
        .applications
        .iter_mut()
        .find(|r| r.app.trim().eq_ignore_ascii_case(app))
    {
        rule.divider = divider;
    } else {
        settings.applications.push(Application {
            app: app.into(),
            divider,
        });
    }
    settings.validate()
}

impl Store {
    pub fn start(path: &Path, settings: &Settings, live: Arc<Mutex<Live>>) -> Result<Self, String> {
        let (saved, mut expected) = super::load(path)?;
        if expected.is_some() && saved != *settings {
            return Err("滚轮文件与当前运行设置不同。请重新读取并应用。".into());
        }
        let path: PathBuf = path.into();
        let mut settings = settings.clone();
        let (sender, changes) = mpsc::sync_channel::<(String, u16)>(16);
        let thread = thread::spawn(move || {
            while let Ok((app, divider)) = changes.recv() {
                let mut proposed = settings.clone();
                let saved = update(&mut proposed, &app, divider)
                    .and_then(|()| super::save(&path, &proposed, expected.as_deref()));
                let Ok(mut state) = live.lock() else {
                    break;
                };
                match saved {
                    Ok(()) => {
                        expected = Some(proposed.to_bytes());
                        state.changed.insert(app.to_ascii_lowercase(), divider);
                        state.settings = Some(proposed.clone());
                        settings = proposed;
                    }
                    Err(error) => {
                        state.error = Some(format!("滚轮分界未保存，区域滚动已停止：{error}"));
                        break;
                    }
                }
            }
        });
        Ok(Self {
            sender: Some(sender),
            thread: Some(thread),
        })
    }

    pub fn sender(&self) -> SyncSender<(String, u16)> {
        self.sender.as_ref().unwrap().clone()
    }

    // Callback sender copies must be removed before stopping this worker.
    pub fn stop(&mut self) {
        self.sender.take();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for Store {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn divider_worker_saves_checked_files_and_refuses_external_changes() {
        let root = std::env::temp_dir().join(format!("a90-divider-store-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("wheel.xml");
        let settings = Settings::default();
        let live = Arc::new(Mutex::new(Live::default()));
        let mut store = Store::start(&path, &settings, live.clone()).unwrap();
        let sender = store.sender();
        sender.send(("org.kde.kwrite".into(), 500)).unwrap();
        let wait = |failure: bool| {
            let deadline = Instant::now() + Duration::from_secs(2);
            loop {
                let state = live.lock().unwrap();
                if state.error.is_some() == failure && (failure || !state.changed.is_empty()) {
                    break;
                }
                drop(state);
                assert!(Instant::now() < deadline);
                thread::sleep(Duration::from_millis(2));
            }
        };
        wait(false);
        assert_eq!(
            super::super::load(&path)
                .unwrap()
                .0
                .divider_for("org.kde.kwrite"),
            500
        );
        assert_eq!(
            live.lock().unwrap().divider(&settings, "ORG.KDE.KWRITE"),
            500
        );
        std::fs::write(&path, b"external change").unwrap();
        sender.send(("org.kde.kwrite".into(), 700)).unwrap();
        wait(true);
        assert_eq!(std::fs::read(&path).unwrap(), b"external change");
        assert_eq!(
            live.lock().unwrap().divider(&settings, "org.kde.kwrite"),
            500
        );
        drop(sender);
        store.stop();
        std::fs::remove_dir_all(root).unwrap();
    }
}
