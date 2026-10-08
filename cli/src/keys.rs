//! Классификация скан-кодов клавиатуры (Linux input-event-codes.h).

use crate::layout::{self, Lang};

pub const KEY_BACKSPACE: u16 = 14;
pub const KEY_TAB: u16 = 15;
pub const KEY_ENTER: u16 = 28;
pub const KEY_LEFTCTRL: u16 = 29;
pub const KEY_LEFTSHIFT: u16 = 42;
pub const KEY_RIGHTSHIFT: u16 = 54;
pub const KEY_LEFTALT: u16 = 56;
pub const KEY_SPACE: u16 = 57;
pub const KEY_KPENTER: u16 = 96;
pub const KEY_RIGHTCTRL: u16 = 97;
pub const KEY_RIGHTALT: u16 = 100;
pub const KEY_INSERT: u16 = 110;
pub const KEY_LEFTMETA: u16 = 125;
pub const KEY_RIGHTMETA: u16 = 126;

/// Кнопки указателя: `BTN_LEFT` (0x110) … `BTN_TASK` (0x117).
#[cfg(target_os = "linux")]
pub const BTN_LEFT: u16 = 0x110;
#[cfg(target_os = "linux")]
pub const BTN_TASK: u16 = 0x117;

/// Клик мышью переставляет курсор ввода, поэтому сбрасывает буфер.
///
/// Тач-события тачпада (`BTN_TOUCH`, `BTN_TOOL_FINGER`) сюда намеренно не
/// входят: они приходят от любого касания, даже от случайного, и сбрасывали бы
/// буфер посреди набора.
#[cfg(target_os = "linux")]
pub fn is_pointer_button(code: u16) -> bool {
    (BTN_LEFT..=BTN_TASK).contains(&code)
}

/// Клавиши, дающие печатный символ в обеих раскладках.
///
/// Диапазоны соответствуют основному блоку: цифровой ряд, три буквенных ряда
/// вместе с пунктуацией, которая в русской раскладке тоже отдаёт буквы
/// (`[` → `х`, `]` → `ъ`, `;` → `ж`, `'` → `э`, `` ` `` → `ё`, `,` → `б`, `.` → `ю`).
pub fn is_char(code: u16) -> bool {
    matches!(code, 2..=13 | 16..=27 | 30..=41 | 43 | 44..=53)
}

/// Разделители слов: их скан-коды одинаковы в любой раскладке,
/// поэтому при переигрывании они воспроизводятся как есть.
pub fn is_separator(code: u16) -> bool {
    code == KEY_SPACE
}

/// Конец фразы — дальше буфер начинается заново.
pub fn is_phrase_end(code: u16) -> bool {
    matches!(code, KEY_ENTER | KEY_KPENTER)
}

pub fn is_shift(code: u16) -> bool {
    matches!(code, KEY_LEFTSHIFT | KEY_RIGHTSHIFT)
}

/// Ctrl/Alt/Meta — при зажатом любом из них нажатие считается сочетанием,
/// а не набором текста.
pub fn is_command_modifier(code: u16) -> bool {
    matches!(
        code,
        KEY_LEFTCTRL | KEY_RIGHTCTRL | KEY_LEFTALT | KEY_RIGHTALT | KEY_LEFTMETA | KEY_RIGHTMETA
    )
}

/// Имена клавиш без учёта регистра; первое имя кода - основное, оно же
/// в меню и в конфиге. Буквы и цифры - по символу в раскладке US.
const NAMES: [(&str, u16); 48] = [
    ("Insert", KEY_INSERT),
    ("Ins", KEY_INSERT),
    ("Pause", 119),
    ("Break", 119),
    ("ScrollLock", 70),
    ("CapsLock", 58),
    ("Menu", 127),
    ("Compose", 127),
    ("Space", KEY_SPACE),
    ("Tab", KEY_TAB),
    ("Enter", KEY_ENTER),
    ("Backspace", KEY_BACKSPACE),
    ("Esc", 1),
    ("Delete", 111),
    ("Home", 102),
    ("End", 107),
    ("PageUp", 104),
    ("PageDown", 109),
    ("Up", 103),
    ("Down", 108),
    ("Left", 105),
    ("Right", 106),
    ("Ctrl", KEY_LEFTCTRL),
    ("LeftCtrl", KEY_LEFTCTRL),
    ("RightCtrl", KEY_RIGHTCTRL),
    ("Shift", KEY_LEFTSHIFT),
    ("LeftShift", KEY_LEFTSHIFT),
    ("RightShift", KEY_RIGHTSHIFT),
    ("Alt", KEY_LEFTALT),
    ("LeftAlt", KEY_LEFTALT),
    ("RightAlt", KEY_RIGHTALT),
    ("AltGr", KEY_RIGHTALT),
    ("Super", KEY_LEFTMETA),
    ("LeftMeta", KEY_LEFTMETA),
    ("Win", KEY_LEFTMETA),
    ("RightMeta", KEY_RIGHTMETA),
    ("F1", 59),
    ("F2", 60),
    ("F3", 61),
    ("F4", 62),
    ("F5", 63),
    ("F6", 64),
    ("F7", 65),
    ("F8", 66),
    ("F9", 67),
    ("F10", 68),
    ("F11", 87),
    ("F12", 88),
];

/// Клавиша из конфига: имя, символ в раскладке US (`q`, `1`) или скан-код
/// числом из двух и более цифр (`125`).
pub fn key_from_spec(spec: &str) -> Option<u16> {
    if let Ok(code) = spec.parse::<u16>()
        && spec.len() > 1
    {
        return (1..=255).contains(&code).then_some(code);
    }
    if let Some((_, code)) = NAMES
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(spec))
    {
        return Some(*code);
    }
    let mut chars = spec.chars();
    let (Some(symbol), None) = (chars.next(), chars.next()) else {
        return None;
    };
    let symbol = symbol.to_ascii_lowercase();
    (2..=53).find(|&code| layout::key_char(Lang::En, code, false) == Some(symbol))
}

/// Имя клавиши для меню и конфига: `Insert`, `Q`, `F12`, иначе скан-код.
pub fn key_name(code: u16) -> String {
    NAMES
        .iter()
        .find(|(_, known)| *known == code)
        .map(|(name, _)| (*name).to_string())
        .or_else(|| layout::key_char(Lang::En, code, false).map(|c| c.to_ascii_uppercase().into()))
        .unwrap_or_else(|| code.to_string())
}

/// Комбинация именами: `Ctrl+Shift+Q`; `parse_combo` читает её обратно.
pub fn combo_name(combo: &[u16]) -> String {
    combo
        .iter()
        .map(|&code| key_name(code))
        .collect::<Vec<_>>()
        .join("+")
}

/// Разбор комбинации вида `125+57` или `super+space` в список скан-кодов.
pub fn parse_combo(spec: &str) -> Option<Vec<u16>> {
    let codes: Vec<u16> = spec
        .split('+')
        .map(|part| key_from_spec(part.trim()))
        .collect::<Option<_>>()?;
    if codes.is_empty()
        || codes
            .iter()
            .enumerate()
            .any(|(i, code)| codes[..i].contains(code))
    {
        None
    } else {
        Some(codes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_combo_name_round_trips_names_letters_and_codes() {
        for spec in [
            "Ctrl+Shift+Q",
            "Alt+F12",
            "Insert",
            "Ctrl+1",
            "Pause",
            "Ctrl+Alt+200",
        ] {
            let combo = parse_combo(spec).unwrap();
            assert_eq!(combo_name(&combo), spec);
        }
        assert_eq!(parse_combo("ctrl+shift+q"), Some(vec![29, 42, 16]));
        assert_eq!(parse_combo("leftctrl+ins"), parse_combo("Ctrl+Insert"));
        assert_eq!(key_from_spec("qq"), None);
    }
}
