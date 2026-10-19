use super::Frame;
use ab_glyph::{Font, FontArc, ScaleFont};
use tiny_skia::{Color, Paint, PathBuilder, Pixmap, Stroke, Transform};

pub(super) const WIDTH: u32 = 360;
pub(super) const HEIGHT: u32 = 252;

pub(super) struct Renderer {
    font: FontArc,
}

impl Renderer {
    pub(super) fn new() -> Result<Self, String> {
        for path in [
            "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/TTF/DejaVuSans.ttf",
            "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        ] {
            if let Ok(bytes) = std::fs::read(path)
                && let Ok(font) = FontArc::try_from_vec(bytes)
            {
                return Ok(Self { font });
            }
        }
        Err("未找到手势提示板字体".into())
    }

    pub(super) fn draw(&self, frame: &Frame) -> Pixmap {
        let mut image = Pixmap::new(WIDTH, HEIGHT).expect("fixed-size tip image");
        image.fill(Color::from_rgba8(29, 30, 34, 242));
        let mut border = Paint::default();
        border.set_color_rgba8(172, 35, 48, 255);
        image.fill_rect(
            tiny_skia::Rect::from_xywh(0.0, 0.0, WIDTH as f32, 3.0).unwrap(),
            &border,
            Transform::identity(),
            None,
        );
        self.text(&mut image, "鼠标手势", 16.0, 27.0, 19.0, [240, 240, 240]);
        let points = fit(&frame.points);
        if let Some(first) = points.first() {
            let mut path = PathBuilder::new();
            path.move_to(first[0], first[1]);
            for point in points.iter().skip(1) {
                path.line_to(point[0], point[1]);
            }
            if let Some(path) = path.finish() {
                let mut paint = Paint::default();
                paint.set_color_rgba8(235, 70, 80, 255);
                image.stroke_path(
                    &path,
                    &paint,
                    &Stroke {
                        width: 3.0,
                        line_cap: tiny_skia::LineCap::Round,
                        line_join: tiny_skia::LineJoin::Round,
                        ..Default::default()
                    },
                    Transform::identity(),
                    None,
                );
            }
            for (point, color) in [
                (first, [180, 180, 180]),
                (points.last().unwrap(), [255, 85, 95]),
            ] {
                let mut paint = Paint::default();
                paint.set_color_rgba8(color[0], color[1], color[2], 255);
                let path = PathBuilder::from_circle(point[0], point[1], 3.5).unwrap();
                image.fill_path(
                    &path,
                    &paint,
                    tiny_skia::FillRule::Winding,
                    Transform::identity(),
                    None,
                );
            }
        }
        self.text(
            &mut image,
            &frame.direction,
            16.0,
            204.0,
            20.0,
            [255, 196, 80],
        );
        self.text(
            &mut image,
            &frame.action,
            16.0,
            234.0,
            18.0,
            [235, 235, 235],
        );
        image
    }

    fn text(
        &self,
        image: &mut Pixmap,
        text: &str,
        mut x: f32,
        baseline: f32,
        size: f32,
        color: [u8; 3],
    ) {
        let font = self.font.as_scaled(size);
        for character in text.chars().take(256) {
            let character = if character.is_control() {
                ' '
            } else {
                character
            };
            let id = font.glyph_id(character);
            let width = font.h_advance(id);
            if x + width > WIDTH as f32 - 16.0 {
                break;
            }
            let glyph = id.with_scale_and_position(size, ab_glyph::point(x, baseline));
            if let Some(outline) = self.font.outline_glyph(glyph) {
                let bounds = outline.px_bounds();
                outline.draw(|gx, gy, coverage| {
                    let (px, py) = (
                        bounds.min.x as i32 + gx as i32,
                        bounds.min.y as i32 + gy as i32,
                    );
                    if px < 0 || py < 0 || px >= WIDTH as i32 || py >= HEIGHT as i32 {
                        return;
                    }
                    let at = (py as usize * WIDTH as usize + px as usize) * 4;
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
            x += width;
        }
    }
}

/// Fit raw counts inside the trajectory area. Widen before subtraction.
fn fit(points: &[[i32; 2]]) -> Vec<[f32; 2]> {
    if points.is_empty() {
        return Vec::new();
    }
    let mut minimum = [i32::MAX; 2];
    let mut maximum = [i32::MIN; 2];
    for point in points {
        for axis in 0..2 {
            minimum[axis] = minimum[axis].min(point[axis]);
            maximum[axis] = maximum[axis].max(point[axis]);
        }
    }
    let extent = [
        maximum[0] as f64 - minimum[0] as f64,
        maximum[1] as f64 - minimum[1] as f64,
    ];
    let scale = (300.0 / extent[0].max(1.0))
        .min(126.0 / extent[1].max(1.0))
        .min(1.0);
    let center = [
        (minimum[0] as f64 + maximum[0] as f64) / 2.0,
        (minimum[1] as f64 + maximum[1] as f64) / 2.0,
    ];
    points
        .iter()
        .map(|p| {
            [
                ((p[0] as f64 - center[0]) * scale + WIDTH as f64 / 2.0) as f32,
                ((p[1] as f64 - center[1]) * scale + 106.0) as f32,
            ]
        })
        .collect()
}

pub(super) fn bgra(image: &Pixmap) -> Vec<u8> {
    image
        .data()
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|pixel| [pixel[2], pixel[1], pixel[0], pixel[3]])
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn trajectory_bounds_cover_large_negative_and_stationary_points() {
        for points in [
            vec![],
            vec![[0, 0]],
            vec![[i32::MIN, i32::MAX], [i32::MAX, i32::MIN]],
        ] {
            for [x, y] in fit(&points) {
                assert!((29.0..=331.0).contains(&x));
                assert!((42.0..=170.0).contains(&y));
            }
        }
        let image = Renderer::new().unwrap().draw(&Frame {
            points: vec![[0, 0], [0, -90], [70, -90]],
            direction: "上 → 右".into(),
            action: "复制 Ctrl+C".into(),
        });
        assert_eq!(image.data().len(), (WIDTH * HEIGHT * 4) as usize);
        assert!(
            image
                .data()
                .as_chunks::<4>()
                .0
                .iter()
                .all(|pixel| pixel[..3].iter().all(|color| *color <= pixel[3]))
        );
        // A standalone artifact lets the renderer be inspected without input capture.
        image
            .save_png(std::env::temp_dir().join("a90-gesture-tip.png"))
            .unwrap();
    }
}
