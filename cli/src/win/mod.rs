//! Windows: низкоуровневый хук вместо evdev, `SendInput` вместо uinput,
//! раскладка и программа - по окну переднего плана, значок в трее.
#![allow(unsafe_code)]

pub mod autostart;
mod clipboard;
mod hook;
mod keymap;
mod output;
pub mod selection;
mod tray;
mod watch;

use std::{
    fs::OpenOptions,
    os::windows::io::IntoRawHandle,
    path::Path,
    sync::{Arc, atomic::AtomicBool, mpsc},
    thread,
    time::SystemTime,
};

use windows_sys::Win32::{
    Foundation::{ERROR_ALREADY_EXISTS, GetLastError, INVALID_HANDLE_VALUE},
    Globalization::GetUserDefaultUILanguage,
    Media::timeBeginPeriod,
    System::{
        Console::{
            ATTACH_PARENT_PROCESS, AttachConsole, GetStdHandle, STD_ERROR_HANDLE,
            STD_OUTPUT_HANDLE, SetStdHandle,
        },
        Threading::CreateMutexW,
    },
};

pub use hook::{Grab, Grabs, set_word_hotkey};
pub use output::SendInputOutput;

use crate::{
    config::Config,
    daemon::{self, Message},
    die,
    injector::Injector,
    tray::{Sender, Shared, Tray},
};

/// Состояние сеанса: в Windows хук не видит ввод экрана блокировки,
/// поэтому сеанс всегда считается доступным.
#[derive(Clone)]
pub struct SessionGuard;

pub struct Context {
    pub generation: u64,
    pub session: Option<String>,
}

impl SessionGuard {
    #[allow(clippy::unused_self)]
    pub fn context(&self) -> Context {
        Context {
            generation: 0,
            session: Some("windows".into()),
        }
    }
}

/// Интерфейс Windows на украинском или установлена украинская раскладка:
/// переменных локали, как в Linux, здесь нет.
pub fn ukrainian_user() -> bool {
    // SAFETY: функция только читает настройку пользователя.
    let ui = unsafe { GetUserDefaultUILanguage() };
    std::iter::once(ui)
        .chain(watch::installed())
        .any(|langid| langid & 0x3ff == watch::UKRAINIAN)
}

pub fn is_shell(app: &str) -> bool {
    app.is_empty() || app == watch::TASKBAR
}

/// Есть ли куда писать stderr: перенаправление или консоль родителя.
/// Без унаследованных дескрипторов (запуск из терминала без перенаправления)
/// подключает консоль родителя для `--help` и журнала.
pub fn attach_console() -> bool {
    // SAFETY: только чтение и замена стандартных дескрипторов процесса.
    unsafe {
        let usable = |id| {
            let handle = GetStdHandle(id);
            !handle.is_null() && handle != INVALID_HANDLE_VALUE
        };
        if usable(STD_ERROR_HANDLE) {
            return true;
        }
        if AttachConsole(ATTACH_PARENT_PROCESS) == 0 {
            return false;
        }
        if let Ok(console) = OpenOptions::new().write(true).open("CONOUT$") {
            let handle = console.into_raw_handle();
            if !usable(STD_OUTPUT_HANDLE) {
                SetStdHandle(STD_OUTPUT_HANDLE, handle);
            }
            SetStdHandle(STD_ERROR_HANDLE, handle);
        }
    }
    true
}

/// Без консоли журнал пишется в `punto-rs.log` рядом с конфигом.
fn log_to_file(config_dir: &Path) {
    let _ = std::fs::create_dir_all(config_dir);
    if let Ok(file) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(config_dir.join("punto-rs.log"))
    {
        // SAFETY: дескриптор файла остаётся открытым до конца процесса.
        unsafe { SetStdHandle(STD_ERROR_HANDLE, file.into_raw_handle()) };
    }
}

fn single_instance() -> bool {
    let name: Vec<u16> = "Local\\punto-rs".encode_utf16().chain([0]).collect();
    // SAFETY: мьютекс живёт до конца процесса.
    unsafe {
        let mutex = CreateMutexW(std::ptr::null(), 0, name.as_ptr());
        !mutex.is_null() && GetLastError() != ERROR_ALREADY_EXISTS
    }
}

pub fn serve(cfg: &Config, verbose: bool, config: &Path, console: bool) {
    if !console {
        log_to_file(config.parent().unwrap_or(config));
    }
    if !single_instance() {
        die(tr!("punto-rs уже запущен", "punto-rs уже запущено"));
    }
    // Без этого паузы короче 15,6 мс (key-delay) растягиваются до такта
    // системного таймера, и исправление слова занимает полсекунды.
    // SAFETY: только точность системного таймера на время жизни процесса.
    unsafe { timeBeginPeriod(1) };
    let stopped = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::sync_channel::<Message>(1024);
    let grabs = Grabs::default();
    let hotkeys = vec![
        cfg.hotkey.clone(),
        cfg.phrase_hotkey.clone(),
        cfg.pause_hotkey.clone(),
    ];
    hook::start(tx.clone(), &grabs, hotkeys, cfg.track_mouse);
    let shared = Arc::new(Shared::default());
    let sender: Sender = Arc::new(move |event| {
        tx.send(Message {
            generation: 0,
            event,
            at: SystemTime::now(),
        })
        .is_ok()
    });
    watch::start(sender.clone(), stopped.clone());
    let tray = Tray::new(sender, shared.clone(), config, stopped.clone());
    if cfg.tray {
        thread::spawn(move || tray::run_tray(tray));
    }
    let version = env!("CARGO_PKG_VERSION");
    tr!(
        log!("punto-rs {version} запущен (Windows)"),
        log!("punto-rs {version} запущено (Windows)")
    );
    let injector = Injector::with_output(output::SendInputOutput);
    if let Err(err) = daemon::run(
        &rx,
        injector,
        cfg,
        verbose,
        &SessionGuard,
        &grabs,
        &stopped,
        &shared,
    ) {
        tr!(
            log!("punto-rs: ввод остановлен после ошибки: {err}"),
            log!("punto-rs: введення зупинено після помилки: {err}")
        );
        std::process::exit(1);
    }
}
