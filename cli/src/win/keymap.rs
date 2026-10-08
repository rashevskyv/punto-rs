//! Скан-коды PC set 1 из хука Windows <-> коды Linux evdev, которыми
//! оперирует движок. Для основного блока они совпадают; клавиши с префиксом
//! E0 (`extended`) и Pause сопоставляются таблицей.

use windows_sys::Win32::UI::Input::KeyboardAndMouse::{MAPVK_VK_TO_VSC_EX, MapVirtualKeyW};

use crate::keys;

/// Комбинация из окна выбора: модификаторы `ctrl+shift+alt` и код VK клавиши.
pub fn vk_combo(modifiers: &str, vk: u32) -> Option<Vec<u16>> {
    // SAFETY: функция только переводит код клавиши в скан-код.
    let scan = unsafe { MapVirtualKeyW(vk, MAPVK_VK_TO_VSC_EX) };
    let key = evdev_code(vk, scan & 0xff, scan & 0xff00 == 0xe000)?;
    let mut combo: Vec<u16> = modifiers
        .split('+')
        .filter(|name| !name.is_empty())
        .map(keys::key_from_spec)
        .collect::<Option<_>>()?;
    combo.push(key);
    Some(combo)
}

pub const VK_PAUSE: u32 = 0x13;
pub const VK_NUMLOCK: u32 = 0x90;
const KEY_PAUSE: u16 = 119;
const KEY_NUMLOCK: u16 = 69;

/// Клавиши E0: (скан-код, код evdev).
const EXTENDED: [(u16, u16); 19] = [
    (0x45, 69),  // NUMLOCK (VK_NUMLOCK тоже приходит с флагом E0)
    (0x1c, 96),  // KPENTER
    (0x1d, 97),  // RIGHTCTRL
    (0x35, 98),  // KPSLASH
    (0x37, 99),  // SYSRQ
    (0x38, 100), // RIGHTALT
    (0x47, 102), // HOME
    (0x48, 103), // UP
    (0x49, 104), // PAGEUP
    (0x4b, 105), // LEFT
    (0x4d, 106), // RIGHT
    (0x4f, 107), // END
    (0x50, 108), // DOWN
    (0x51, 109), // PAGEDOWN
    (0x52, 110), // INSERT
    (0x53, 111), // DELETE
    (0x5b, 125), // LEFTMETA
    (0x5c, 126), // RIGHTMETA
    (0x5d, 127), // COMPOSE (Menu)
];

/// Код evdev для события хука; `None` - клавиша не нужна движку
/// (например, поддельный `Ctrl` от `AltGr` со скан-кодом `0x21D`).
pub fn evdev_code(vk: u32, scan: u32, extended: bool) -> Option<u16> {
    match vk {
        VK_PAUSE => return Some(KEY_PAUSE),
        VK_NUMLOCK => return Some(KEY_NUMLOCK),
        _ => {}
    }
    let scan = u16::try_from(scan)
        .ok()
        .filter(|scan| (1..=0x7f).contains(scan))?;
    if extended {
        EXTENDED
            .iter()
            .find(|(code, _)| *code == scan)
            .map(|(_, key)| *key)
    } else {
        Some(scan)
    }
}

/// Скан-код и признак E0 для `SendInput`; `None` - Pause (её шлют как VK).
pub fn scan_code(key: u16) -> Option<(u16, bool)> {
    match key {
        KEY_PAUSE => None,
        _ => EXTENDED
            .iter()
            .find(|(_, code)| *code == key)
            .map_or(Some((key, false)), |(scan, _)| Some((*scan, true))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keymap_round_trips_main_block_and_extended_keys() {
        for key in (1..=88).chain(EXTENDED.iter().map(|(_, key)| *key)) {
            let (scan, extended) = scan_code(key).unwrap();
            assert_eq!(evdev_code(0, u32::from(scan), extended), Some(key), "{key}");
        }
        assert_eq!(evdev_code(VK_PAUSE, 0x45, false), Some(KEY_PAUSE));
        assert_eq!(scan_code(KEY_PAUSE), None);
        assert_eq!(evdev_code(0xa2, 0x21d, false), None);
        assert_eq!(evdev_code(0x2d, 0x52, true), Some(110));
    }
}
