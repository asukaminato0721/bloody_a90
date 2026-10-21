//! Click-through desktop surfaces that never take focus. The gesture tip and
//! the status window draw into them. Pixels are premultiplied BGRA.
mod wayland;
mod x11;

use ab_glyph::{Font, FontArc, ScaleFont};
use tiny_skia::Pixmap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Place {
    /// Top right on Wayland. Beside the pointer on X11.
    Pointer,
    BottomRight,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Spec {
    pub width: u32,
    pub height: u32,
    /// Shown in errors, and used as the layer namespace and X11 window name.
    pub name: &'static str,
    pub id: &'static str,
    pub place: Place,
}

impl Spec {
    fn bytes(&self) -> usize {
        (self.width * self.height * 4) as usize
    }
}

pub(crate) enum Native {
    Wayland(Box<wayland::Window>),
    X11(Box<x11::Window>),
}

impl Native {
    pub(crate) fn connect(spec: Spec) -> Result<Self, String> {
        if std::env::var_os("WAYLAND_DISPLAY").is_some() {
            wayland::Window::connect(spec).map(|w| Self::Wayland(Box::new(w)))
        } else {
            x11::Window::connect(spec).map(|w| Self::X11(Box::new(w)))
        }
    }
    /// False means that the surface is not ready yet. Present again later.
    pub(crate) fn present(&mut self, pixels: &[u8]) -> Result<bool, String> {
        match self {
            Self::Wayland(w) => w.present(pixels),
            Self::X11(w) => w.present(pixels),
        }
    }
    pub(crate) fn hide(&mut self) -> Result<(), String> {
        match self {
            Self::Wayland(w) => w.hide(),
            Self::X11(w) => w.hide(),
        }
    }
    /// Dispatch events. Waits up to 16 ms.
    pub(crate) fn pump(&mut self) -> Result<(), String> {
        match self {
            Self::Wayland(w) => w.pump(),
            Self::X11(w) => w.pump(),
        }
    }
}

pub(crate) fn font() -> Result<FontArc, String> {
    for path in [
        "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/TTF/DejaVuSans.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    ] {
        if let Ok(bytes) = std::fs::read(path)
            && let Ok(font) = FontArc::try_from_vec(bytes)
        {
            return Ok(font);
        }
    }
    Err("未找到桌面提示字体".into())
}

/// Draw one line from `x` and stop before `right`.
pub(crate) fn text(
    image: &mut Pixmap,
    font: &FontArc,
    text: &str,
    [mut x, baseline]: [f32; 2],
    right: f32,
    size: f32,
    color: [u8; 3],
) {
    let (width, height) = (image.width() as i32, image.height() as i32);
    let scaled = font.as_scaled(size);
    for character in text.chars().take(256) {
        let character = if character.is_control() {
            ' '
        } else {
            character
        };
        let id = scaled.glyph_id(character);
        let advance = scaled.h_advance(id);
        if x + advance > right {
            break;
        }
        let glyph = id.with_scale_and_position(size, ab_glyph::point(x, baseline));
        if let Some(outline) = font.outline_glyph(glyph) {
            let bounds = outline.px_bounds();
            outline.draw(|gx, gy, coverage| {
                let (px, py) = (
                    bounds.min.x as i32 + gx as i32,
                    bounds.min.y as i32 + gy as i32,
                );
                if px < 0 || py < 0 || px >= width || py >= height {
                    return;
                }
                let at = (py as usize * width as usize + px as usize) * 4;
                let data = image.data_mut();
                for channel in 0..3 {
                    data[at + channel] = (color[channel] as f32 * coverage
                        + data[at + channel] as f32 * (1.0 - coverage))
                        .round() as u8;
                }
                data[at + 3] =
                    (255.0 * coverage + data[at + 3] as f32 * (1.0 - coverage)).round() as u8;
            });
        }
        x += advance;
    }
}

/// Width of one line at `size`.
pub(crate) fn measure(font: &FontArc, text: &str, size: f32) -> f32 {
    let scaled = font.as_scaled(size);
    text.chars()
        .map(|c| scaled.h_advance(scaled.glyph_id(c)))
        .sum()
}

pub(crate) fn bgra(image: &Pixmap) -> Vec<u8> {
    image
        .data()
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|pixel| [pixel[2], pixel[1], pixel[0], pixel[3]])
        .collect()
}
