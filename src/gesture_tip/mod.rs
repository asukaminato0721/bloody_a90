//! Non-interactive feedback for Mouse.gestures. No input is injected here.
mod render;
mod wayland;
mod x11;

use std::{
    sync::{Arc, Mutex, mpsc},
    thread::{self, JoinHandle},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    pub points: Vec<[i32; 2]>,
    pub direction: String,
    pub action: String,
}

#[derive(Default)]
struct Shared {
    frame: Option<Frame>,
    revision: u64,
    stop: bool,
    error: Option<String>,
}

enum Native {
    Wayland(Box<wayland::Window>),
    X11(Box<x11::Window>),
}

impl Native {
    fn connect() -> Result<Self, String> {
        if std::env::var_os("WAYLAND_DISPLAY").is_some() {
            wayland::Window::connect().map(|w| Self::Wayland(Box::new(w)))
        } else {
            x11::Window::connect().map(|w| Self::X11(Box::new(w)))
        }
    }
    fn present(&mut self, pixels: &[u8]) -> Result<bool, String> {
        match self {
            Self::Wayland(w) => w.present(pixels),
            Self::X11(w) => w.present(pixels),
        }
    }
    fn hide(&mut self) -> Result<(), String> {
        match self {
            Self::Wayland(w) => w.hide(),
            Self::X11(w) => w.hide(),
        }
    }
    fn pump(&mut self) -> Result<(), String> {
        match self {
            Self::Wayland(w) => w.pump(),
            Self::X11(w) => w.pump(),
        }
    }
}

pub struct Handle {
    shared: Arc<Mutex<Shared>>,
    thread: Option<JoinHandle<()>>,
}

impl Handle {
    pub fn start() -> Result<Self, String> {
        let shared = Arc::new(Mutex::new(Shared::default()));
        let state = shared.clone();
        let (ready, result) = mpsc::sync_channel(1);
        let thread = thread::spawn(move || {
            let initialized = Native::connect()
                .and_then(|native| render::Renderer::new().map(|renderer| (native, renderer)));
            let (mut native, renderer) = match initialized {
                Ok(pair) => {
                    let _ = ready.send(Ok(()));
                    pair
                }
                Err(error) => {
                    let _ = ready.send(Err(error));
                    return;
                }
            };
            let result = (|| {
                let mut rendered = 0;
                loop {
                    let (frame, revision, stop) = {
                        let lock = state.lock().map_err(|_| "提示板状态不可用")?;
                        (lock.frame.clone(), lock.revision, lock.stop)
                    };
                    if stop {
                        break;
                    }
                    if revision != rendered {
                        if let Some(frame) = frame {
                            let image = renderer.draw(&frame);
                            if native.present(&render::bgra(&image))? {
                                rendered = revision;
                            }
                        } else {
                            native.hide()?;
                            rendered = revision;
                        }
                    }
                    native.pump()?;
                }
                native.hide()
            })();
            if let Err(error) = result
                && let Ok(mut state) = state.lock()
            {
                state.error = Some(error);
            }
        });
        match result
            .recv()
            .map_err(|_| "提示板线程未能启动".to_string())?
        {
            Ok(()) => Ok(Self {
                shared,
                thread: Some(thread),
            }),
            Err(error) => {
                let _ = thread.join();
                Err(error)
            }
        }
    }

    pub fn update(&self, frame: Option<Frame>) -> Result<(), String> {
        let mut shared = self.shared.lock().map_err(|_| "提示板状态不可用")?;
        if let Some(error) = &shared.error {
            return Err(error.clone());
        }
        if self
            .thread
            .as_ref()
            .is_some_and(|thread| thread.is_finished())
        {
            return Err("手势提示板线程已退出".into());
        }
        if shared.frame != frame {
            shared.frame = frame;
            shared.revision = shared.revision.wrapping_add(1);
        }
        Ok(())
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        if let Ok(mut state) = self.shared.lock() {
            state.stop = true;
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
