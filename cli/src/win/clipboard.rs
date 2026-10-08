//! Текст буфера обмена (`CF_UNICODETEXT`): чтение, запись, номер изменения.

use std::{ptr::null_mut, thread, time::Duration};

use windows_sys::Win32::{
    Foundation::GlobalFree,
    System::{
        DataExchange::{
            CloseClipboard, EmptyClipboard, GetClipboardData, GetClipboardSequenceNumber,
            OpenClipboard, SetClipboardData,
        },
        Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock},
        Ole::CF_UNICODETEXT,
    },
};

/// Номер изменения буфера обмена: растёт при каждой записи в него.
pub fn sequence() -> u32 {
    // SAFETY: функция без аргументов, только читает счётчик.
    unsafe { GetClipboardSequenceNumber() }
}

/// Открывает буфер обмена; его может ненадолго держать другая программа.
fn open() -> bool {
    for _ in 0..10 {
        // SAFETY: буфер открывается без окна-владельца и закрывается вызывающим.
        if unsafe { OpenClipboard(null_mut()) } != 0 {
            return true;
        }
        thread::sleep(Duration::from_millis(10));
    }
    false
}

/// Текст буфера обмена; `None` - не текст, пусто или буфер занят.
pub fn text() -> Option<String> {
    if !open() {
        return None;
    }
    // SAFETY: буфер открыт; память данных читается под GlobalLock в пределах
    // её размера, до завершающего нуля UTF-16.
    unsafe {
        let handle = GetClipboardData(u32::from(CF_UNICODETEXT));
        let pointer = GlobalLock(handle).cast::<u16>();
        let text = (!pointer.is_null()).then(|| {
            let units = std::slice::from_raw_parts(pointer, GlobalSize(handle) / 2);
            let length = units
                .iter()
                .position(|&unit| unit == 0)
                .unwrap_or(units.len());
            String::from_utf16_lossy(&units[..length])
        });
        if !pointer.is_null() {
            GlobalUnlock(handle);
        }
        CloseClipboard();
        text
    }
}

/// Кладёт `text` в буфер обмена; `false` - не удалось.
pub fn set_text(text: &str) -> bool {
    let wide: Vec<u16> = text.encode_utf16().chain([0]).collect();
    if !open() {
        return false;
    }
    // SAFETY: буфер открыт; после SetClipboardData памятью владеет система,
    // при ошибке она освобождается здесь.
    unsafe {
        EmptyClipboard();
        let memory = GlobalAlloc(GMEM_MOVEABLE, wide.len() * 2);
        let pointer = GlobalLock(memory).cast::<u16>();
        let mut stored = false;
        if !pointer.is_null() {
            std::ptr::copy_nonoverlapping(wide.as_ptr(), pointer, wide.len());
            GlobalUnlock(memory);
            stored = !SetClipboardData(u32::from(CF_UNICODETEXT), memory).is_null();
        }
        if !stored && !memory.is_null() {
            GlobalFree(memory);
        }
        CloseClipboard();
        stored
    }
}
