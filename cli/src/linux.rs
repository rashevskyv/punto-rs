//! Запуск демона в Linux: evdev, uinput, logind, KDE Plasma.

use std::{
    path::Path,
    sync::{Arc, atomic::AtomicBool, mpsc},
    thread,
    time::SystemTime,
};

use signal_hook::consts::{SIGINT, SIGTERM};

use crate::{
    config::Config,
    daemon::{self, Message},
    devices, die,
    injector::Injector,
    instance::InstanceLock,
    kde, kwin,
    session::SessionGuard,
    tray::{self, Shared, Tray},
    xdg_dir,
};

pub const VIRTUAL_NAME: &str = "punto-rs virtual keyboard";

/// Запуск демона: блокировка экземпляра, потоки сессии, раскладки и устройств,
/// главный цикл до сигнала завершения.
pub fn serve(cfg: &Config, verbose: bool, config_dir: &Path) {
    let runtime = xdg_dir("XDG_RUNTIME_DIR").unwrap_or_else(|| {
        die(tr!(
            "XDG_RUNTIME_DIR не задан: запускайте в пользовательской сессии",
            "XDG_RUNTIME_DIR не задано: запускайте в сеансі користувача"
        ))
    });
    let runtime = runtime.join("punto-rs");
    let _instance = InstanceLock::acquire(&runtime).unwrap_or_else(|err| {
        die(&tr!(
            format!("блокировка экземпляра: {err}"),
            format!("блокування екземпляра: {err}")
        ))
    });
    // Старые версии ещё не брали файловую блокировку.
    if evdev::enumerate().any(|(_, device)| device.name() == Some(VIRTUAL_NAME)) {
        die(tr!(
            "виртуальная клавиатура punto-rs уже существует; сначала остановите предыдущий экземпляр",
            "віртуальна клавіатура punto-rs уже існує; спершу зупиніть попередній екземпляр"
        ));
    }
    let stopped = Arc::new(AtomicBool::new(false));
    for signal in [SIGINT, SIGTERM] {
        signal_hook::flag::register(signal, stopped.clone()).unwrap_or_else(|err| {
            die(&tr!(
                format!("обработчик завершения: {err}"),
                format!("обробник завершення: {err}")
            ))
        });
    }
    let injector = Injector::new(VIRTUAL_NAME).unwrap_or_else(|err| {
        tr!(
            log!("punto-rs: не удалось создать виртуальную клавиатуру (/dev/uinput): {err}"),
            log!("punto-rs: не вдалося створити віртуальну клавіатуру (/dev/uinput): {err}")
        );
        std::process::exit(1)
    });
    let guard = SessionGuard::new(cfg.session_guard);
    if cfg.session_guard {
        let guard = guard.clone();
        let stopped = stopped.clone();
        thread::spawn(move || guard.monitor(&stopped));
    } else {
        tr!(
            log!("punto-rs: session-guard=no — блокировка экрана и смена сессии не отслеживаются"),
            log!("punto-rs: session-guard=no — блокування екрана і зміна сеансу не відстежуються")
        );
    }
    let (tx, rx) = mpsc::sync_channel::<Message>(1024);
    let grabs = devices::Grabs::default();
    let session_bus = dbus::blocking::Connection::new_session;
    kde::watch(cfg, session_bus, tx.clone(), guard.clone(), stopped.clone());
    let shared = Arc::new(Shared::default());
    let sender: tray::Sender = {
        let (tx, guard) = (tx.clone(), guard.clone());
        Arc::new(move |event| {
            tx.send(Message {
                generation: guard.context().generation,
                event,
                at: SystemTime::now(),
            })
            .is_ok()
        })
    };
    kwin::watch(&runtime, session_bus, sender.clone(), stopped.clone());
    let tray = Tray::new(sender, shared.clone(), config_dir, stopped.clone());
    if cfg.tray {
        thread::spawn(move || tray::sni::run_tray(tray));
    }
    devices::watch(tx, cfg, guard.clone(), grabs.clone(), stopped.clone());
    let version = env!("CARGO_PKG_VERSION");
    let (word, phrase, pause) = (&cfg.hotkey, &cfg.phrase_hotkey, &cfg.pause_hotkey);
    tr!(
        log!("punto-rs {version} запущен: слово {word:?}, фраза {phrase:?}, пауза {pause:?}"),
        log!("punto-rs {version} запущено: слово {word:?}, фраза {phrase:?}, пауза {pause:?}")
    );
    if let Err(err) = daemon::run(
        &rx, injector, cfg, verbose, &guard, &grabs, &stopped, &shared,
    ) {
        tr!(
            log!("punto-rs: инжект остановлен после ошибки: {err}"),
            log!("punto-rs: введення зупинено після помилки: {err}")
        );
        std::process::exit(1);
    }
}
