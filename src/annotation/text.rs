//! System-font shaping, fallback and rasterization for multilingual annotations.
use super::Shape;
use cosmic_text::{Attrs, Buffer, Color, Family, FontSystem, Metrics, Shaping, SwashCache};
use gpui_kit::{Pixels, Point};
use std::cell::RefCell;

thread_local! {
    static FONTS: RefCell<(FontSystem, SwashCache)> = RefCell::new((FontSystem::new(), SwashCache::new()));
}

pub(crate) fn with_fonts<T>(f: impl FnOnce(&mut FontSystem) -> T) -> T {
    FONTS.with_borrow_mut(|(fonts, _)| f(fonts))
}

/// Isolate layout tests from installed fonts and platform fallback differences.
#[cfg(test)]
pub(crate) fn with_test_font<T>(f: impl FnOnce() -> T) -> T {
    struct RestoreFonts(Option<(FontSystem, SwashCache)>);
    impl Drop for RestoreFonts {
        fn drop(&mut self) {
            FONTS.with(|fonts| fonts.replace(self.0.take().unwrap()));
        }
    }
    let mut db = cosmic_text::fontdb::Database::new();
    db.load_font_data(include_bytes!("../../assets/DejaVuSans-Bold.ttf").to_vec());
    db.set_sans_serif_family("DejaVu Sans");
    let fonts = FontSystem::new_with_locale_and_db("en-US".into(), db);
    let _restore = RestoreFonts(Some(
        FONTS.with(|cell| cell.replace((fonts, SwashCache::new()))),
    ));
    f()
}
pub(crate) fn buffer(text: &str, width: f32, limit: f32, font_size: f32) -> Buffer {
    with_fonts(|fonts| {
        let mut buffer = Buffer::new(fonts, Metrics::new(font_size, font_size * 1.35));
        buffer.set_size(Some(width), Some(limit));
        buffer.set_text(
            text,
            &Attrs::new().family(Family::SansSerif),
            Shaping::Advanced,
            None,
        );
        buffer.shape_until_scroll(fonts, false);
        buffer
    })
}

pub(super) fn rasterize(
    shape: &Shape,
    rgba: &mut [u8],
    w: u32,
    h: u32,
    origin: Point<Pixels>,
    scale: f32,
) {
    let Some(text) = &shape.text else {
        return;
    };
    let left = (f32::from(shape.bounds.left() - origin.x) * scale).round() as i32;
    let top = (f32::from(shape.bounds.top() - origin.y) * scale).round() as i32;
    let width = (f32::from(shape.bounds.size.width) * scale).round() as i32;
    let height = (f32::from(shape.bounds.size.height) * scale).round() as i32;
    if width <= 0 || height <= 0 {
        return;
    }
    FONTS.with_borrow_mut(|(fonts, cache)| {
        let mut buffer = Buffer::new(
            fonts,
            Metrics::new(shape.width * scale, shape.width * scale * 1.35),
        );
        buffer.set_size(Some(width as f32), Some(height as f32));
        buffer.set_text(
            text,
            &Attrs::new().family(Family::SansSerif),
            Shaping::Advanced,
            None,
        );
        let [r, g, b, a] = shape.color.to_be_bytes();
        buffer.draw(
            fonts,
            cache,
            Color::rgba(r, g, b, a),
            |x, y, pw, ph, color| {
                for row in y.max(0)..(y + ph as i32).min(height) {
                    for col in x.max(0)..(x + pw as i32).min(width) {
                        let (dx, dy) = (left + col, top + row);
                        if dx < 0 || dy < 0 || dx >= w as i32 || dy >= h as i32 {
                            continue;
                        }
                        let p = &mut rgba[((dy as u32 * w + dx as u32) * 4) as usize..][..4];
                        if p[3] == 0 {
                            continue;
                        }
                        let alpha = color.a() as f32 / 255.;
                        for (c, ink) in [color.r(), color.g(), color.b()].into_iter().enumerate() {
                            p[c] = (p[c] as f32 * (1. - alpha) + ink as f32 * alpha).round() as u8;
                        }
                    }
                }
            },
        );
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::annotation::ShapeKind;
    use gpui_kit::{Bounds, point, px, size};

    #[test]
    fn fractional_scale_clips_text_and_preserves_transparent_gaps() {
        for scale in [1_f32, 1.25, 1.73, 2.] {
            let shape = Shape {
                kind: ShapeKind::Text,
                number: None,
                text: Some("Hello 中文\nSecond line".into()),
                bounds: Bounds::new(point(px(10.), px(10.)), size(px(140.), px(90.))),
                color: 0x000000ff,
                width: 24.,
                points: Vec::new(),
            };
            let side = (180. * scale).round() as u32;
            let gap = (65. * scale).round() as u32;
            let mut pixels = vec![255; (side * side * 4) as usize];
            for y in 0..side {
                pixels[((y * side + gap) * 4 + 3) as usize] = 0;
            }
            rasterize(
                &shape,
                &mut pixels,
                side,
                side,
                point(px(0.), px(0.)),
                scale,
            );
            let mut rows = [false; 2];
            for (i, p) in pixels.as_chunks::<4>().0.iter().enumerate() {
                let (x, y) = (i as u32 % side, i as u32 / side);
                if x == gap {
                    assert_eq!(p[3], 0);
                }
                if p[0] != 255 {
                    assert!(x >= (10. * scale).round() as u32 && x < (150. * scale).round() as u32);
                    assert!(y >= (10. * scale).round() as u32 && y < (100. * scale).round() as u32);
                    rows[usize::from(y as f32 >= 42. * scale)] = true;
                }
            }
            assert_eq!(rows, [true, true]);
        }
    }
}
