//! Simulator 24: minimize or maximize the window under the pointer.
mod kwin;
mod x11;

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operation {
    Minimize,
    Maximize,
}

trait Backend {
    fn execute(&mut self, operation: Operation) -> Result<(), String>;
}

pub struct Handle {
    requests: SyncSender<(u64, Operation)>,
    results: Receiver<String>,
    stop: Arc<AtomicBool>,
    generation: Arc<AtomicU64>,
    thread: Option<JoinHandle<()>>,
}

impl Handle {
    pub fn start() -> Result<Self, String> {
        let (requests, pending) = mpsc::sync_channel(16);
        let (replies, results) = mpsc::sync_channel(16);
        let (ready, started) = mpsc::sync_channel(1);
        let stop = Arc::new(AtomicBool::new(false));
        let generation = Arc::new(AtomicU64::new(0));
        let stopping = stop.clone();
        let current = generation.clone();
        let thread = thread::spawn(move || {
            let connect = || -> Result<Box<dyn Backend>, String> {
                if std::env::var_os("WAYLAND_DISPLAY").is_some() {
                    Ok(Box::new(kwin::KWin::connect()?))
                } else {
                    Ok(Box::new(x11::X11::connect()?))
                }
            };
            let mut backend = match connect() {
                Ok(backend) => {
                    let _ = ready.send(Ok(()));
                    backend
                }
                Err(error) => {
                    let _ = ready.send(Err(error));
                    return;
                }
            };
            while !stopping.load(Ordering::Acquire) {
                match pending.recv_timeout(Duration::from_millis(10)) {
                    Ok((generation, operation)) => {
                        if stopping.load(Ordering::Acquire)
                            || current.load(Ordering::Acquire) != generation
                        {
                            continue;
                        }
                        if let Err(error) = backend.execute(operation) {
                            let _ = replies.try_send(error);
                        }
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                }
            }
        });
        match started.recv_timeout(Duration::from_secs(8)) {
            Ok(Ok(())) => Ok(Self {
                requests,
                results,
                stop,
                generation,
                thread: Some(thread),
            }),
            Ok(Err(error)) => {
                let _ = thread.join();
                Err(error)
            }
            Err(_) => {
                stop.store(true, Ordering::Release);
                Err("窗口动作接口初始化超时".into())
            }
        }
    }

    pub fn execute(&self, operation: Operation) -> Result<(), String> {
        self.requests
            .try_send((self.generation.load(Ordering::Acquire), operation))
            .map_err(|e| format!("无法提交窗口动作：{e}"))
    }
    pub fn poll(&self) -> Vec<String> {
        self.results.try_iter().collect()
    }
    pub fn cancel_pending(&self) {
        self.generation.fetch_add(1, Ordering::AcqRel);
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
