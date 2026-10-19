//! Read the vendor HID state outside the evdev forwarding thread.
use crate::device::Device;
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

type Sample = ([u8; 3], Instant);

fn bits(device: &Device) -> Result<[u8; 3], String> {
    let mut state = [0; 3];
    for button in device.held_buttons()? {
        state[(button as usize - 1) / 8] |= 1 << ((button - 1) % 8);
    }
    Ok(state)
}

pub(super) struct Reader {
    requests: SyncSender<()>,
    results: Receiver<Result<Sample, String>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    pending: bool,
    cached: Option<Sample>,
}

impl Reader {
    pub(super) fn start() -> Result<Self, String> {
        // Check HID access before the caller grabs the physical pointer.
        let device = Device::open()?;
        let state = bits(&device)?;
        drop(device);
        let mut reader = Self::with_query(|stop| {
            let deadline = Instant::now() + Duration::from_secs(3);
            loop {
                if stop.load(Ordering::Acquire) {
                    return Err("按键状态读取已停止".into());
                }
                if let Some(device) = Device::try_open()? {
                    return bits(&device);
                }
                if Instant::now() >= deadline {
                    return Err("鼠标接口持续被占用，无法读取宏需要的按键状态".into());
                }
                thread::sleep(Duration::from_millis(5));
            }
        });
        reader.cached = Some((state, Instant::now()));
        Ok(reader)
    }

    pub(super) fn with_query(
        mut query: impl FnMut(&AtomicBool) -> Result<[u8; 3], String> + Send + 'static,
    ) -> Self {
        let (requests, pending) = mpsc::sync_channel(1);
        let (replies, results) = mpsc::sync_channel(1);
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let thread = thread::spawn(move || {
            while !stopping.load(Ordering::Acquire) {
                match pending.recv_timeout(Duration::from_millis(10)) {
                    Ok(()) => {
                        let result = query(&stopping).map(|state| (state, Instant::now()));
                        let _ = replies.try_send(result);
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }
        });
        Self {
            requests,
            results,
            stop,
            thread: Some(thread),
            pending: false,
            cached: None,
        }
    }

    /// None pauses the VM for this tick. Pointer events keep moving.
    pub(super) fn sample(&mut self) -> Result<Option<[u8; 3]>, String> {
        if self.pending {
            match self.results.try_recv() {
                Ok(result) => {
                    self.pending = false;
                    self.cached = Some(result?);
                }
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => {
                    return Err("按键状态读取线程已退出".into());
                }
            }
        }
        if let Some((state, time)) = self.cached
            && time.elapsed() < Duration::from_millis(10)
        {
            return Ok(Some(state));
        }
        if !self.pending {
            self.requests
                .try_send(())
                .map_err(|_| "无法请求按键状态".to_string())?;
            self.pending = true;
        }
        Ok(None)
    }
}

impl Drop for Reader {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_hid_query_does_not_block_and_errors_reach_the_caller() {
        let (started, waiting) = mpsc::channel();
        let (release, gate) = mpsc::channel();
        let mut first = true;
        let mut reader = Reader::with_query(move |_| {
            if first {
                first = false;
                started.send(()).unwrap();
                gate.recv_timeout(Duration::from_secs(2)).unwrap();
                Ok([0, 4, 0])
            } else {
                Err("device removed".into())
            }
        });
        assert_eq!(reader.sample().unwrap(), None);
        waiting.recv_timeout(Duration::from_secs(1)).unwrap();
        let start = Instant::now();
        for _ in 0..100 {
            assert_eq!(reader.sample().unwrap(), None);
        }
        assert!(start.elapsed() < Duration::from_millis(100));
        release.send(()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(1);
        loop {
            if let Some(state) = reader.sample().unwrap() {
                assert_eq!(state, [0, 4, 0]);
                break;
            }
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(1));
        }
        thread::sleep(Duration::from_millis(11));
        loop {
            match reader.sample() {
                Err(error) => {
                    assert_eq!(error, "device removed");
                    break;
                }
                Ok(None) => {}
                other => panic!("unexpected sample: {other:?}"),
            }
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn shutdown_cancels_a_reader_waiting_for_device_access() {
        let (started, waiting) = mpsc::channel();
        let mut reader = Reader::with_query(move |stop| {
            started.send(()).unwrap();
            while !stop.load(Ordering::Acquire) {
                thread::sleep(Duration::from_millis(1));
            }
            Err("stopped".into())
        });
        assert_eq!(reader.sample().unwrap(), None);
        waiting.recv_timeout(Duration::from_secs(1)).unwrap();
        let start = Instant::now();
        drop(reader);
        assert!(start.elapsed() < Duration::from_secs(1));
    }
}
