//! Вывод клавиш через `SendInput` по скан-кодам: символ даёт активная
//! раскладка программы, как и при наборе с настоящей клавиатуры.

use std::io;

use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP,
    KEYEVENTF_SCANCODE, SendInput,
};

use super::keymap::{VK_PAUSE, scan_code};
use crate::injector::KeyOutput;

pub struct SendInputOutput;

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
        let input = INPUT {
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
        };
        let size = i32::try_from(std::mem::size_of::<INPUT>()).unwrap_or(i32::MAX);
        // SAFETY: один корректно заполненный INPUT.
        if unsafe { SendInput(1, &raw const input, size) } == 1 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }
}
