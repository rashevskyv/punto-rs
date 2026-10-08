//! Вывод клавиш через `SendInput` по скан-кодам: символ даёт активная
//! раскладка программы, как и при наборе с настоящей клавиатуры.

use std::io;

use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP,
    KEYEVENTF_SCANCODE, KEYEVENTF_UNICODE, SendInput,
};

use super::keymap::{VK_PAUSE, scan_code};
use crate::injector::KeyOutput;

pub struct SendInputOutput;

fn keyboard_input(vk: u16, scan: u16, flags: u32) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: scan,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn send(inputs: &[INPUT]) -> io::Result<()> {
    let count = u32::try_from(inputs.len()).unwrap_or(u32::MAX);
    let size = i32::try_from(std::mem::size_of::<INPUT>()).unwrap_or(i32::MAX);
    // SAFETY: срез корректно заполненных INPUT, длина передана.
    if unsafe { SendInput(count, inputs.as_ptr(), size) } == count {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// Набирает текст символами Unicode: не зависит от раскладки программы.
/// Перевод строки - `\r`, как от клавиши Enter.
pub fn type_text(text: &str) -> io::Result<()> {
    let text = text.replace("\r\n", "\r").replace('\n', "\r");
    let inputs: Vec<INPUT> = text
        .encode_utf16()
        .flat_map(|unit| {
            [
                keyboard_input(0, unit, KEYEVENTF_UNICODE),
                keyboard_input(0, unit, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP),
            ]
        })
        .collect();
    send(&inputs)
}

impl KeyOutput for SendInputOutput {
    fn emit_key(&mut self, code: u16, value: i32) -> io::Result<()> {
        let (vk, scan, mut flags) = match scan_code(code) {
            Some((scan, extended)) => (
                0,
                scan,
                KEYEVENTF_SCANCODE | if extended { KEYEVENTF_EXTENDEDKEY } else { 0 },
            ),
            None => (u16::try_from(VK_PAUSE).unwrap_or(0), 0, 0),
        };
        if value == 0 {
            flags |= KEYEVENTF_KEYUP;
        }
        send(&[keyboard_input(vk, scan, flags)])
    }
}
