//! Иконка трея 32x32: активная раскладка буквами, серый фон - коррекция стоит.

use crate::layout::Lang;

pub const SIZE: usize = 32;

/// Буквы 5x7, строка - 5 младших бит.
fn glyph(letter: char) -> [u8; 7] {
    match letter {
        'E' => [31, 16, 16, 30, 16, 16, 31],
        'N' => [17, 25, 21, 19, 17, 17, 17],
        'U' => [17, 17, 17, 17, 17, 17, 14],
        'A' => [14, 17, 17, 31, 17, 17, 17],
        'R' => [30, 17, 17, 30, 20, 18, 17],
        _ => [30, 17, 17, 30, 16, 16, 16],
    }
}

/// RGBA-пиксели иконки. `lang` - активная раскладка, `None` - неизвестна;
/// `active` - коррекция работает (не пауза, не программа-исключение).
pub fn rgba(lang: Option<Lang>, active: bool) -> Vec<u8> {
    let (text, background, ink): (&str, [u8; 3], [u8; 3]) = match (lang, active) {
        (_, false) => (label(lang), [0x8a, 0x8a, 0x8a], [0xe6, 0xe6, 0xe6]),
        (Some(Lang::Uk), true) => ("UA", [0x00, 0x57, 0xb7], [0xff, 0xd7, 0x00]),
        _ => (label(lang), [0x2f, 0x3e, 0x4f], [0xff, 0xff, 0xff]),
    };
    let mut pixels = vec![0_u8; SIZE * SIZE * 4];
    let radius = 6.0_f32;
    for y in 0..SIZE {
        for x in 0..SIZE {
            if inside_rounded(x, y, radius) {
                put(&mut pixels, x, y, background);
            }
        }
    }
    // Две буквы по 5x7 с масштабом 2 и зазором 2: 22x14 по центру.
    let (scale, gap) = (2, 2);
    let width = text.chars().count() * (5 * scale + gap) - gap;
    let left = (SIZE - width) / 2;
    let top = (SIZE - 7 * scale) / 2;
    for (index, letter) in text.chars().enumerate() {
        let origin = left + index * (5 * scale + gap);
        for (row, bits) in glyph(letter).iter().enumerate() {
            for column in 0..5 {
                if bits & (1 << (4 - column)) != 0 {
                    for dy in 0..scale {
                        for dx in 0..scale {
                            let x = origin + column * scale + dx;
                            put(&mut pixels, x, top + row * scale + dy, ink);
                        }
                    }
                }
            }
        }
    }
    pixels
}

fn label(lang: Option<Lang>) -> &'static str {
    match lang {
        Some(Lang::En) => "EN",
        Some(Lang::Ru) => "RU",
        Some(Lang::Uk) => "UA",
        None => "P",
    }
}

fn inside_rounded(x: usize, y: usize, radius: f32) -> bool {
    #[allow(clippy::cast_precision_loss)]
    let (x, y, size) = (x as f32 + 0.5, y as f32 + 0.5, SIZE as f32);
    let cx = x.clamp(radius, size - radius);
    let cy = y.clamp(radius, size - radius);
    (x - cx).powi(2) + (y - cy).powi(2) <= radius * radius
}

fn put(pixels: &mut [u8], x: usize, y: usize, color: [u8; 3]) {
    let offset = (y * SIZE + x) * 4;
    pixels[offset..offset + 4].copy_from_slice(&[color[0], color[1], color[2], 0xff]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_icon_colors_follow_layout_and_state() {
        let center = (16 * SIZE + 1) * 4;
        let ua = rgba(Some(Lang::Uk), true);
        assert_eq!(ua.len(), SIZE * SIZE * 4);
        assert_eq!(&ua[center..center + 4], &[0x00, 0x57, 0xb7, 0xff]);
        let paused = rgba(Some(Lang::Uk), false);
        assert_eq!(&paused[center..center + 4], &[0x8a, 0x8a, 0x8a, 0xff]);
        // Угол прозрачный, буквы нарисованы.
        assert_eq!(ua[3], 0);
        assert!(ua.chunks(4).any(|pixel| pixel == [0xff, 0xd7, 0x00, 0xff]));
        assert!(rgba(None, true).chunks(4).any(|pixel| pixel == [0xff; 4]));
        assert_ne!(rgba(Some(Lang::En), true), rgba(Some(Lang::Ru), true));
    }
}
