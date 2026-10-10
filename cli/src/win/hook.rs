//! Низкоуровневые хуки клавиатуры и мыши: аналог чтения evdev. На время
//! коррекции (`Grab`) нажатия не доходят до программ и переигрываются после,
//! как при `EVIOCGRAB`. Свои нажатия (`SendInput`) хук пропускает по метке
//! `OWN_INPUT`; нажатия других программ (переназначение клавиш в `PowerToys`,
//! `AutoHotkey`) обрабатываются как настоящие.

use std::{
    collections::HashSet,
    io,
    ptr::null_mut,
    sync::{
        Arc, Mutex, MutexGuard, OnceLock, PoisonError,
        atomic::{AtomicBool, Ordering},
        mpsc::SyncSender,
    },
    thread,
    time::SystemTime,
};

use windows_sys::Win32::{
    Foundation::{LPARAM, LRESULT, WPARAM},
    System::LibraryLoader::GetModuleHandleW,
    UI::WindowsAndMessaging::{
        CallNextHookEx, DispatchMessageW, GetMessageW, HC_ACTION, KBDLLHOOKSTRUCT, LLKHF_EXTENDED,
        MSG, SetWindowsHookExW, TranslateMessage, WH_KEYBOARD_LL, WH_MOUSE_LL, WM_KEYDOWN,
        WM_LBUTTONDOWN, WM_MBUTTONDOWN, WM_RBUTTONDOWN, WM_SYSKEYDOWN, WM_XBUTTONDOWN,
    },
};

use super::{keymap::evdev_code, output::OWN_INPUT};
use crate::{
    daemon::Message,
    engine::{DeviceEvent, KeyEvent},
    keys,
};

/// Захват ввода на время коррекции и разрешение придержать Enter.
#[derive(Clone, Default)]
pub struct Grabs {
    capture: Arc<AtomicBool>,
    hold_enter: Arc<AtomicBool>,
}

pub struct Grab<'a>(&'a Grabs);

impl Grabs {
    // Тот же интерфейс, что у захвата evdev, который может не удаться.
    #[allow(clippy::unnecessary_wraps)]
    pub fn grab(&self) -> io::Result<Grab<'_>> {
        self.capture.store(true, Ordering::SeqCst);
        Ok(Grab(self))
    }

    /// Слово перед курсором в чужой раскладке: Enter придерживается, пока
    /// оно не исправлено.
    pub fn hold_enter(&self, hold: bool) {
        self.hold_enter.store(hold, Ordering::SeqCst);
    }
}

impl Drop for Grab<'_> {
    fn drop(&mut self) {
        self.0.capture.store(false, Ordering::SeqCst);
    }
}

struct Hook {
    tx: SyncSender<Message>,
    capture: Arc<AtomicBool>,
    hold_enter: Arc<AtomicBool>,
    held: Mutex<HashSet<u16>>,
    /// Горячие клавиши punto-rs: программам их нажатие не нужно (Insert
    /// иначе включает режим замены). Первая - исправление слова.
    hotkeys: Mutex<Vec<Vec<u16>>>,
    /// Клавиши, нажатие которых проглочено: проглатывается и отпускание.
    swallowed: Mutex<HashSet<u16>>,
    recording: Mutex<Recording>,
}

/// Запись комбинации из меню.
#[derive(Default)]
enum Recording {
    #[default]
    Off,
    Waiting,
    Done(Vec<u16>),
}

static HOOK: OnceLock<Hook> = OnceLock::new();

fn send(hook: &Hook, event: DeviceEvent) {
    let _ = hook.tx.try_send(Message {
        generation: 0,
        event,
        at: SystemTime::now(),
    });
}

fn recording(hook: &Hook) -> MutexGuard<'_, Recording> {
    hook.recording
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

/// Начинает запись комбинации: следующее нажатие не-модификатора вместе с
/// зажатыми модификаторами вернёт `recorded`.
pub fn start_recording() {
    if let Some(hook) = HOOK.get() {
        *recording(hook) = Recording::Waiting;
    }
}

/// Записанная комбинация; запись при этом заканчивается.
pub fn recorded() -> Option<Vec<u16>> {
    let mut state = recording(HOOK.get()?);
    if !matches!(*state, Recording::Done(_)) {
        return None;
    }
    match std::mem::take(&mut *state) {
        Recording::Done(combo) => Some(combo),
        _ => None,
    }
}

pub fn stop_recording() {
    if let Some(hook) = HOOK.get() {
        *recording(hook) = Recording::Off;
    }
}

/// Записывает комбинацию, если запись ждёт нажатия: точные коды клавиш,
/// правые модификаторы тоже. `true` - нажатие записано.
fn record(hook: &Hook, held: &HashSet<u16>, key: u16) -> bool {
    let mut state = recording(hook);
    if !matches!(*state, Recording::Waiting)
        || keys::is_shift(key)
        || keys::is_command_modifier(key)
    {
        return false;
    }
    let mut combo: Vec<u16> = held.iter().copied().filter(|&code| code != key).collect();
    combo.sort_unstable();
    combo.push(key);
    *state = Recording::Done(combo);
    true
}

/// Enter (и на цифровом блоке) без модификаторов. Последняя буква слова
/// при быстром наборе ещё зажата, когда нажат Enter: она не мешает.
fn enter_alone(held: &HashSet<u16>, key: u16) -> bool {
    keys::is_phrase_end(key)
        && !held
            .iter()
            .any(|&code| keys::is_shift(code) || keys::is_command_modifier(code))
}

/// Обрабатывает клавишу; `true` - не передавать её программам.
fn on_key(hook: &Hook, key: u16, down: bool) -> bool {
    let mut held = hook.held.lock().unwrap_or_else(PoisonError::into_inner);
    let value = if down {
        if held.insert(key) { 1 } else { 2 }
    } else {
        held.remove(&key);
        0
    };
    // Записанная клавиша не доходит ни до программ, ни до демона.
    if value == 1 && record(hook, &held, key) {
        drop(held);
        hook.swallowed
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(key);
        return true;
    }
    // Enter без модификаторов после слова в чужой раскладке: демон нажмёт
    // его сам после исправления.
    let held_enter = value == 1
        && enter_alone(&held, key)
        && hook.hold_enter.load(Ordering::SeqCst)
        && !hook.capture.load(Ordering::SeqCst);
    let hotkey = down
        && hook
            .hotkeys
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .any(|combo| {
                combo.last() == Some(&key)
                    && combo.len() == held.len()
                    && combo.iter().all(|code| held.contains(code))
            });
    drop(held);
    send(
        hook,
        if held_enter {
            DeviceEvent::HeldEnter
        } else {
            DeviceEvent::Key(KeyEvent {
                device_id: 1,
                code: key,
                value,
            })
        },
    );
    let mut swallowed = hook
        .swallowed
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    if hotkey || held_enter {
        swallowed.insert(key);
    }
    let release = !down && swallowed.remove(&key);
    hotkey
        || held_enter
        || release
        || hook.capture.load(Ordering::SeqCst)
        || (value == 2 && swallowed.contains(&key))
}

unsafe extern "system" fn keyboard(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == i32::try_from(HC_ACTION).unwrap_or(0)
        && let Some(hook) = HOOK.get()
    {
        // SAFETY: для WH_KEYBOARD_LL lparam указывает на KBDLLHOOKSTRUCT.
        let info = unsafe { &*(lparam as *const KBDLLHOOKSTRUCT) };
        let down = matches!(
            u32::try_from(wparam).unwrap_or(0),
            WM_KEYDOWN | WM_SYSKEYDOWN
        );
        if info.dwExtraInfo != OWN_INPUT
            && let Some(key) =
                evdev_code(info.vkCode, info.scanCode, info.flags & LLKHF_EXTENDED != 0)
            && on_key(hook, key, down)
        {
            return 1;
        }
    }
    // SAFETY: передача события следующему хуку по контракту WinAPI.
    unsafe { CallNextHookEx(null_mut(), code, wparam, lparam) }
}

unsafe extern "system" fn mouse(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == i32::try_from(HC_ACTION).unwrap_or(0)
        && let Some(hook) = HOOK.get()
        && matches!(
            u32::try_from(wparam).unwrap_or(0),
            WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN | WM_XBUTTONDOWN
        )
    {
        send(hook, DeviceEvent::Click);
    }
    // SAFETY: см. выше.
    unsafe { CallNextHookEx(null_mut(), code, wparam, lparam) }
}

/// Новая комбинация исправления слова: её нажатие тоже не доходит до программ.
pub fn set_word_hotkey(combo: Vec<u16>) {
    if let Some(hook) = HOOK.get()
        && let Some(first) = hook
            .hotkeys
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .first_mut()
    {
        *first = combo;
    }
}

/// Ставит хуки в своём потоке с очередью сообщений: без неё они не работают.
pub fn start(tx: SyncSender<Message>, grabs: &Grabs, hotkeys: Vec<Vec<u16>>, track_mouse: bool) {
    let capture = grabs.capture.clone();
    let hold_enter = grabs.hold_enter.clone();
    thread::spawn(move || {
        let _ = HOOK.set(Hook {
            tx,
            capture,
            hold_enter,
            held: Mutex::default(),
            hotkeys: Mutex::new(hotkeys),
            swallowed: Mutex::default(),
            recording: Mutex::default(),
        });
        // SAFETY: процедуры хуков живут всё время процесса; модуль - текущий exe.
        unsafe {
            let module = GetModuleHandleW(std::ptr::null());
            if SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard), module, 0).is_null() {
                tr!(
                    log!(
                        "punto-rs: хук клавиатуры не установлен: {}",
                        io::Error::last_os_error()
                    ),
                    log!(
                        "punto-rs: хук клавіатури не встановлено: {}",
                        io::Error::last_os_error()
                    )
                );
                return;
            }
            if track_mouse {
                SetWindowsHookExW(WH_MOUSE_LL, Some(mouse), module, 0);
            }
            let mut message: MSG = std::mem::zeroed();
            while GetMessageW(&raw mut message, null_mut(), 0, 0) > 0 {
                TranslateMessage(&raw const message);
                DispatchMessageW(&raw const message);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_enter_alone_allows_held_letter_not_modifiers() {
        let held = |codes: &[u16]| codes.iter().copied().collect::<HashSet<u16>>();
        assert!(enter_alone(&held(&[keys::KEY_ENTER]), keys::KEY_ENTER));
        // Буква `t` ещё зажата: быстрый набор слова и Enter.
        assert!(enter_alone(&held(&[20, keys::KEY_ENTER]), keys::KEY_ENTER));
        assert!(enter_alone(&held(&[keys::KEY_KPENTER]), keys::KEY_KPENTER));
        assert!(!enter_alone(
            &held(&[keys::KEY_LEFTSHIFT, keys::KEY_ENTER]),
            keys::KEY_ENTER
        ));
        assert!(!enter_alone(
            &held(&[keys::KEY_LEFTCTRL, keys::KEY_ENTER]),
            keys::KEY_ENTER
        ));
        assert!(!enter_alone(&held(&[keys::KEY_SPACE]), keys::KEY_SPACE));
    }
}
