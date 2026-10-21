//! Desktop status window, like the official TformMouseStatusDialog
//! (skin Data/Mouse/Forms/MouseLEDEx): the running profile's light colour and
//! the X/Y CPI of the running DPI level. It reads RAM only; nothing is written.
//!
//! Notification mode belongs to the host-action service, and firmware resets
//! can clear it. The window therefore polls RAM with non-blocking handles,
//! and uses notifications only as an early wake-up when they are on.
use crate::{
    configuration::Configuration,
    device::Device,
    overlay::{self, Native, Place, Spec},
    sensitivity::{self, Record},
    vendor_input::Source,
};
use ab_glyph::FontArc;
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use tiny_skia::{FillRule, Paint, PathBuilder, Pixmap, Rect, Transform};

const WIDTH: u32 = 124;
const HEIGHT: u32 = 40;
const SPEC: Spec = Spec {
    width: WIDTH,
    height: HEIGHT,
    name: "状态浮窗",
    id: "bloody-a90-status",
    place: Place::BottomRight,
};
/// Few queries per poll. A full sensitivity read follows only a change.
const POLL: Duration = Duration::from_millis(300);
/// Saves can change the X/Y scales without changing the checked bytes.
const FULL: Duration = Duration::from_secs(3);
/// EXE 00502384 sets tm_CPI, tm_Profile and the other change timers to
/// 3000 ms (the form resource says 5000). Each timer clears its change flag,
/// and the window hides when no flag is left.
const SHOW: Duration = Duration::from_secs(3);

/// The official tray menu 开启功能显示 (mi_MouseLedFunc_*). Its third
/// choice, 不显示, means that no window is started.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Display {
    /// 永远显示
    Always,
    /// 改变时显示: three seconds after the profile or CPI changes.
    Changed,
}

/// Factory profile colours, the indicator cycle (docs/research/PROFILES.md).
pub const FACTORY_COLORS: [[u8; 3]; 7] = [
    [255, 0, 0],
    [0, 255, 0],
    [255, 255, 0],
    [0, 0, 255],
    [0, 255, 255],
    [255, 0, 255],
    [255, 255, 255],
];

/// The stored light colour of each profile: red in the low byte of
/// metadata word 0, green and blue in word 1.
pub fn colors(config: &Configuration) -> Vec<[u8; 3]> {
    config
        .profiles
        .iter()
        .map(|profile| {
            let [word0, word1] = profile.metadata;
            [word0 as u8, (word1 >> 8) as u8, word1 as u8]
        })
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Sample {
    profile: u8,
    level: u8,
    record: Record,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Frame {
    profile: u8,
    color: [u8; 3],
    cpi: Option<[u32; 2]>,
}

struct Poller {
    source: Option<Source>,
    last: Option<Sample>,
    full: Instant,
    decoded: Option<(Record, Option<[u32; 2]>)>,
}

impl Poller {
    /// None hides the window: no mouse, or no access to it.
    fn poll(&mut self) -> Option<Sample> {
        let device = match Device::try_open() {
            Ok(Some(device)) => device,
            // A save or another process holds the interface. Keep the value.
            Ok(None) => return self.last,
            Err(_) => {
                self.source = None;
                self.last = None;
                return None;
            }
        };
        if self.source.is_none() {
            self.source = Source::for_device(&device).ok();
        }
        let read = (|| {
            let quick = [device.ram(0x263)?, device.ram(0x27)?, device.ram(0x2a)?];
            if let Some(last) = self.last
                && self.full.elapsed() < FULL
                && quick == [last.profile, last.level, last.record.sensor_code as u8]
            {
                return Ok(last);
            }
            let runtime = device.runtime_sensitivity()?;
            self.full = Instant::now();
            Ok::<_, String>(Sample {
                profile: quick[0],
                level: runtime.selected,
                record: runtime.record,
            })
        })();
        // A failed query during a firmware reload keeps the shown value.
        if let Ok(sample) = read {
            self.last = Some(sample);
        }
        self.last
    }

    /// Notifications only make the next poll happen sooner.
    fn notified(&mut self) -> bool {
        let Some(source) = &mut self.source else {
            return false;
        };
        match source.read() {
            Ok(states) => states.iter().any(|state| {
                self.last.is_none_or(|last| {
                    (state.profile, state.sensitivity) != (last.profile, last.level)
                })
            }),
            Err(_) => {
                self.source = None;
                false
            }
        }
    }

    fn cpi(&mut self, record: Record) -> Option<[u32; 2]> {
        match self.decoded {
            Some((known, cpi)) if known == record => cpi,
            _ => {
                let cpi = sensitivity::decode(record);
                self.decoded = Some((record, cpi));
                cpi
            }
        }
    }
}

#[derive(Default)]
struct Shared {
    colors: Vec<[u8; 3]>,
    error: Option<String>,
}

pub struct Handle {
    shared: Arc<Mutex<Shared>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Handle {
    pub fn start(display: Display, colors: Vec<[u8; 3]>) -> Result<Self, String> {
        let shared = Arc::new(Mutex::new(Shared {
            colors,
            error: None,
        }));
        let stop = Arc::new(AtomicBool::new(false));
        let (state, stopping) = (shared.clone(), stop.clone());
        let (ready, result) = mpsc::sync_channel(1);
        let thread = thread::spawn(move || {
            let initialized =
                Native::connect(SPEC).and_then(|native| overlay::font().map(|font| (native, font)));
            let (mut native, font) = match initialized {
                Ok(pair) => {
                    let _ = ready.send(Ok(()));
                    pair
                }
                Err(error) => {
                    let _ = ready.send(Err(error));
                    return;
                }
            };
            let mut poller = Poller {
                source: None,
                last: None,
                full: Instant::now(),
                decoded: None,
            };
            let result = (|| {
                let (mut sampled, mut shown, mut until) = (None, None, None);
                let (mut next, mut pending) = (Instant::now(), false);
                while !stopping.load(Ordering::Acquire) {
                    if Instant::now() >= next || poller.notified() {
                        let sample = poller.poll();
                        next = Instant::now() + POLL;
                        let colors = state.lock().map_err(|_| "状态浮窗不可用")?.colors.clone();
                        let frame = sample.map(|sample| Frame {
                            profile: sample.profile,
                            color: colors
                                .get(usize::from(sample.profile))
                                .or(FACTORY_COLORS.get(usize::from(sample.profile)))
                                .copied()
                                .unwrap_or([255; 3]),
                            cpi: poller.cpi(sample.record),
                        });
                        // The first reading after a connection is not a change.
                        if frame != sampled {
                            if sampled.is_some() && frame.is_some() {
                                until = Some(Instant::now() + SHOW);
                            }
                            sampled = frame;
                        }
                    }
                    let wanted = match display {
                        Display::Always => sampled,
                        Display::Changed => {
                            sampled.filter(|_| until.is_some_and(|t| Instant::now() < t))
                        }
                    };
                    if wanted != shown {
                        shown = wanted;
                        pending = true;
                    }
                    if pending {
                        pending = match &shown {
                            Some(frame) => !native.present(&overlay::bgra(&draw(&font, frame)))?,
                            None => native.hide().map(|()| false)?,
                        };
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
            .map_err(|_| "状态浮窗线程未能启动".to_string())?
        {
            Ok(()) => Ok(Self {
                shared,
                stop,
                thread: Some(thread),
            }),
            Err(error) => {
                let _ = thread.join();
                Err(error)
            }
        }
    }

    /// Profile colours after a configuration read or save.
    pub fn set_colors(&self, colors: Vec<[u8; 3]>) {
        if let Ok(mut shared) = self.shared.lock() {
            shared.colors = colors;
        }
    }

    /// The window has stopped after a desktop error.
    pub fn error(&self) -> Option<String> {
        let error = self.shared.lock().ok()?.error.clone();
        error.or_else(|| {
            self.thread
                .as_ref()
                .is_some_and(|thread| thread.is_finished())
                .then(|| "状态浮窗线程已退出".into())
        })
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

fn fill(image: &mut Pixmap, rect: Rect, color: [u8; 4]) {
    let mut paint = Paint::default();
    paint.set_color_rgba8(color[0], color[1], color[2], color[3]);
    image.fill_rect(rect, &paint, Transform::identity(), None);
}

fn circle(image: &mut Pixmap, center: [f32; 2], radius: f32, color: [u8; 4]) {
    let mut paint = Paint::default();
    paint.set_color_rgba8(color[0], color[1], color[2], color[3]);
    paint.anti_alias = true;
    let path = PathBuilder::from_circle(center[0], center[1], radius).unwrap();
    image.fill_path(
        &path,
        &paint,
        FillRule::Winding,
        Transform::identity(),
        None,
    );
}

/// Layout of MouseLEDEx: a dark frame, 28-pixel icon cells from (6, 6), and
/// the CPI text at the right in CPI_FontColor. The icon is drawn here, not
/// copied: an indicator light in the profile colour with the profile number.
fn draw(font: &FontArc, frame: &Frame) -> Pixmap {
    let mut image = Pixmap::new(WIDTH, HEIGHT).expect("fixed-size status image");
    let rect = |x, y, w, h| Rect::from_xywh(x, y, w, h).unwrap();
    let (w, h) = (WIDTH as f32, HEIGHT as f32);
    fill(&mut image, rect(0.0, 0.0, w, h), [96, 98, 102, 255]);
    fill(
        &mut image,
        rect(1.0, 1.0, w - 2.0, h - 2.0),
        [18, 19, 21, 255],
    );
    fill(&mut image, rect(5.0, 5.0, 30.0, 30.0), [70, 72, 76, 255]);
    fill(&mut image, rect(6.0, 6.0, 28.0, 28.0), [0, 0, 0, 255]);
    let [r, g, b] = frame.color;
    circle(&mut image, [20.0, 20.0], 12.0, [r, g, b, 70]);
    circle(&mut image, [20.0, 20.0], 9.0, [r, g, b, 255]);
    // Dark digits on bright colours, light digits on dark ones.
    let luma = 0.299 * f32::from(r) + 0.587 * f32::from(g) + 0.114 * f32::from(b);
    let ink = if luma > 110.0 { [0; 3] } else { [255; 3] };
    let number = (frame.profile + 1).to_string();
    let size = 14.0;
    let left = 20.0 - overlay::measure(font, &number, size) / 2.0;
    overlay::text(&mut image, font, &number, [left, 25.0], w, size, ink);
    let [x, y] = frame.cpi.map_or(["—".into(), "—".into()], |cpi| {
        cpi.map(|v| v.to_string())
    });
    for (label, value, baseline) in [("X", x, 17.0), ("Y", y, 33.0)] {
        overlay::text(&mut image, font, label, [44.0, baseline], w, 12.0, [254; 3]);
        let size = 13.0;
        let right = w - 8.0;
        let left = right - overlay::measure(font, &value, size);
        overlay::text(
            &mut image,
            font,
            &value,
            [left, baseline],
            w,
            size,
            [255, 50, 50],
        );
    }
    image
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_colors_come_from_metadata() {
        use crate::configuration::{Action, Profile};
        let mut profile = Profile {
            buttons: Default::default(),
            wheel: [Action::Direct(0x262c), Action::Direct(0x262b)],
            gestures: None,
            metadata: [0x01ff, 0xff00],
        };
        let mut config = Configuration {
            profiles: vec![profile.clone()],
            selected: 0,
            auxiliary: std::array::from_fn(|_| Action::Direct(0x262e)),
            parameters: [0; 6],
            flags: 0,
            header_word6: 0,
            lighting: Vec::new(),
        };
        profile.metadata = [0x0300, 0x00ff];
        config.profiles.push(profile);
        assert_eq!(colors(&config), [[255, 255, 0], [0, 0, 255]]);
    }

    #[test]
    #[ignore = "requires A90_TEST_STATUS_WINDOW=1, a desktop and the mouse; reads RAM only"]
    fn status_window_follows_the_mouse() {
        assert_eq!(std::env::var("A90_TEST_STATUS_WINDOW").as_deref(), Ok("1"));
        let seconds = std::env::var("A90_TEST_STATUS_SECONDS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(5);
        let window = Handle::start(Display::Always, Vec::new()).unwrap();
        let until = Instant::now() + Duration::from_secs(seconds);
        while Instant::now() < until {
            assert_eq!(window.error(), None);
            thread::sleep(Duration::from_millis(100));
        }
    }

    #[test]
    fn frame_draws_inside_the_window() {
        let font = overlay::font().unwrap();
        for frame in [
            Frame {
                profile: 0,
                color: [255, 0, 0],
                cpi: Some([6200, 6200]),
            },
            Frame {
                profile: 6,
                color: [255, 255, 255],
                cpi: None,
            },
        ] {
            let image = draw(&font, &frame);
            assert_eq!(image.data().len(), (WIDTH * HEIGHT * 4) as usize);
            assert!(
                image
                    .data()
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .all(|pixel| pixel[..3].iter().all(|color| *color <= pixel[3]))
            );
            // Inspect the drawing without a desktop or a mouse.
            image
                .save_png(std::env::temp_dir().join(format!("a90-status-{}.png", frame.profile)))
                .unwrap();
        }
    }
}
