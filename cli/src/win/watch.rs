//! Раскладка и программа окна переднего плана: опрос раз в 50 мс, как
//! замена сигналов KDE и скрипта `KWin`.

use std::{
    ptr::null_mut,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

use windows_sys::Win32::{
    Foundation::{CloseHandle, HWND},
    System::Threading::{
        OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
        QueryFullProcessImageNameW,
    },
    UI::{
        Input::KeyboardAndMouse::{GetKeyboardLayout, GetKeyboardLayoutList},
        WindowsAndMessaging::{GetClassNameW, GetForegroundWindow, GetWindowThreadProcessId},
    },
};

use crate::{
    engine::DeviceEvent,
    layout::{Lang, Pair},
    tray::Sender,
};

/// Панель задач и область уведомлений: щелчок по трею не меняет программу.
pub const TASKBAR: &str = "@taskbar";

/// Язык раскладки по основному языку `LANGID`.
fn lang(langid: u16) -> Option<Lang> {
    match langid & 0x3ff {
        0x09 => Some(Lang::En),
        0x19 => Some(Lang::Ru),
        0x22 => Some(Lang::Uk),
        _ => None,
    }
}

/// Пара из двух установленных раскладок EN+RU или EN+UK с активной `current`.
pub fn pair_for(installed: &[u16], current: u16) -> Option<Pair> {
    let [first, second] = installed else {
        return None;
    };
    let langs = [lang(*first)?, lang(*second)?];
    let other = match langs {
        [Lang::En, other] | [other, Lang::En] if other != Lang::En => other,
        _ => return None,
    };
    match lang(current)? {
        Lang::En => Some(Pair::new(Lang::En, other)),
        shown if shown == other => Some(Pair::new(other, Lang::En)),
        _ => None,
    }
}

fn wide_to_string(buffer: &[u16]) -> String {
    String::from_utf16_lossy(buffer)
}

/// Имя .exe процесса, `None` - нет доступа.
fn exe_name(pid: u32) -> Option<String> {
    // SAFETY: дескриптор процесса закрывается в этой же функции.
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            return None;
        }
        let mut buffer = [0_u16; 1024];
        let mut size = u32::try_from(buffer.len()).unwrap_or(0);
        let ok = QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            buffer.as_mut_ptr(),
            &raw mut size,
        );
        CloseHandle(process);
        if ok == 0 {
            return None;
        }
        let path = wide_to_string(&buffer[..usize::try_from(size).unwrap_or(0)]);
        path.rsplit('\\').next().map(str::to_string)
    }
}

fn is_taskbar(window: HWND) -> bool {
    let mut buffer = [0_u16; 64];
    // SAFETY: буфер на 64 символа, размер передан.
    let length = unsafe { GetClassNameW(window, buffer.as_mut_ptr(), 64) };
    let class = wide_to_string(&buffer[..usize::try_from(length).unwrap_or(0)]);
    matches!(
        class.as_str(),
        "Shell_TrayWnd"
            | "Shell_SecondaryTrayWnd"
            | "NotifyIconOverflowWindow"
            | "TopLevelWindowForOverflowXamlIsland"
    )
}

fn installed() -> Vec<u16> {
    // SAFETY: сначала размер списка, затем буфер нужной длины.
    unsafe {
        let count = GetKeyboardLayoutList(0, null_mut());
        let mut layouts = vec![null_mut(); usize::try_from(count).unwrap_or(0)];
        let count = GetKeyboardLayoutList(count, layouts.as_mut_ptr());
        layouts.truncate(usize::try_from(count).unwrap_or(0));
        let mut langs: Vec<u16> = layouts
            .iter()
            .map(|layout| u16::try_from(*layout as usize & 0xffff).unwrap_or(0))
            .collect();
        langs.dedup();
        langs
    }
}

pub fn start(send: Sender, stopped: Arc<AtomicBool>) {
    thread::spawn(move || {
        let mut layout = None;
        let mut app = None;
        while !stopped.load(Ordering::Relaxed) {
            // SAFETY: функции только читают состояние окна переднего плана.
            let (window, thread_id, pid) = unsafe {
                let window = GetForegroundWindow();
                let mut pid = 0;
                let thread_id = GetWindowThreadProcessId(window, &raw mut pid);
                (window, thread_id, pid)
            };
            if !window.is_null() {
                // SAFETY: см. выше.
                let current = unsafe { GetKeyboardLayout(thread_id) } as usize & 0xffff;
                let pair = pair_for(&installed(), u16::try_from(current).unwrap_or(0));
                if layout != Some(pair) {
                    layout = Some(pair);
                    send(DeviceEvent::Layout(pair));
                }
                let name = if is_taskbar(window) {
                    Some(TASKBAR.to_string())
                } else {
                    exe_name(pid)
                };
                if app.as_ref() != Some(&name) {
                    send(DeviceEvent::App(name.clone()));
                    app = Some(name);
                }
            }
            thread::sleep(Duration::from_millis(50));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pair_for_two_layouts_with_english() {
        let (en, ru, uk, de) = (0x0409, 0x0419, 0x0422, 0x0407);
        assert_eq!(pair_for(&[en, uk], uk), Some(Pair::new(Lang::Uk, Lang::En)));
        assert_eq!(pair_for(&[uk, en], en), Some(Pair::new(Lang::En, Lang::Uk)));
        assert_eq!(pair_for(&[en, ru], ru), Some(Pair::new(Lang::Ru, Lang::En)));
        assert_eq!(pair_for(&[en, de], en), None);
        assert_eq!(pair_for(&[en, uk, ru], en), None);
        assert_eq!(pair_for(&[en, uk], ru), None);
    }
}
