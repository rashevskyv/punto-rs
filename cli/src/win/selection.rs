//! Конвертация выделенного текста: копирование `Ctrl+Insert`, перевод
//! клавиша в клавишу и набор результата символами Unicode поверх выделения.

use std::{io, time::Duration};

use super::{clipboard, output};
use crate::{
    config::Config,
    injector::{Injector, KeyOutput, Pause},
    keys,
    layout::{self, Pair},
};

/// Конвертирует выделение; `true` - раскладка переключена на язык результата.
/// Без выделения программа не меняет буфер обмена, и текст не трогается.
pub fn convert<T: KeyOutput>(
    injector: &mut Injector<T>,
    pair: Option<Pair>,
    cfg: &Config,
    mut wait: impl FnMut(Pause) -> io::Result<()>,
) -> io::Result<bool> {
    let Some(pair) = pair else {
        return Ok(false);
    };
    let saved = clipboard::text();
    let before = clipboard::sequence();
    // Ctrl+Insert, а не Ctrl+C: в терминале Ctrl+C прерывает программу.
    injector.chord(&[keys::KEY_LEFTCTRL, keys::KEY_INSERT])?;
    // Программа кладёт выделение в буфер обмена не сразу.
    for _ in 0..30 {
        if clipboard::sequence() != before {
            break;
        }
        wait(Pause::Fixed(Duration::from_millis(10)))?;
    }
    if clipboard::sequence() == before {
        return Ok(false);
    }
    let copied = clipboard::text();
    if let Some(saved) = saved {
        clipboard::set_text(&saved);
    }
    let Some((text, target)) = copied
        .as_deref()
        .and_then(|text| layout::keymap::convert_text(text, pair.shown, pair.other))
    else {
        return Ok(false);
    };
    output::type_text(&text)?;
    if target == pair.shown {
        return Ok(false);
    }
    injector.chord(&cfg.layout_switch)?;
    Ok(true)
}
