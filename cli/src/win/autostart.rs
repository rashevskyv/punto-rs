//! Автозапуск: значение `punto-rs` в `HKCU\...\Run` с путём к exe.

use std::{io, ptr::null_mut};

use windows_sys::Win32::{
    Foundation::ERROR_SUCCESS,
    System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_SZ, RegCloseKey,
        RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
    },
};

const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE: &str = "punto-rs";

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain([0]).collect()
}

fn open(access: u32) -> io::Result<HKEY> {
    let mut key = null_mut();
    // SAFETY: строка с нулём на конце, ключ закрывает вызывающий.
    let status = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            wide(RUN).as_ptr(),
            0,
            access,
            &raw mut key,
        )
    };
    if status == ERROR_SUCCESS {
        Ok(key)
    } else {
        Err(io::Error::from_raw_os_error(
            i32::try_from(status).unwrap_or(0),
        ))
    }
}

pub fn enabled() -> bool {
    open(KEY_QUERY_VALUE).is_ok_and(|key| {
        // SAFETY: запрос только наличия значения; ключ закрывается.
        unsafe {
            let status = RegQueryValueExW(
                key,
                wide(VALUE).as_ptr(),
                null_mut(),
                null_mut(),
                null_mut(),
                null_mut(),
            );
            RegCloseKey(key);
            status == ERROR_SUCCESS
        }
    })
}

pub fn set(on: bool) -> io::Result<()> {
    let key = open(KEY_SET_VALUE)?;
    let status = if on {
        let command = wide(&format!("\"{}\"", std::env::current_exe()?.display()));
        let bytes = u32::try_from(command.len() * 2).unwrap_or(u32::MAX);
        // SAFETY: данные REG_SZ с нулём на конце, длина в байтах.
        unsafe {
            RegSetValueExW(
                key,
                wide(VALUE).as_ptr(),
                0,
                REG_SZ,
                command.as_ptr().cast(),
                bytes,
            )
        }
    } else {
        // SAFETY: имя значения с нулём на конце.
        unsafe { RegDeleteValueW(key, wide(VALUE).as_ptr()) }
    };
    // SAFETY: ключ открыт выше.
    unsafe { RegCloseKey(key) };
    if status == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(
            i32::try_from(status).unwrap_or(0),
        ))
    }
}
