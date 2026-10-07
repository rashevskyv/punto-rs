//! Активная программа в KDE Plasma: скрипт `KWin` сообщает класс окна по
//! session D-Bus, демон передаёт его движку как `DeviceEvent::App`.
//! Другого способа узнать активное окно в Wayland нет.

use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

use dbus::{blocking::Connection, channel::MatchingReceiver, message::MatchRule};

use crate::{engine::DeviceEvent, tray::Sender};

const NAME: &str = "io.github.rashevskyv.PuntoRs";
const PLUGIN: &str = "punto-rs-active-window";
const KWIN: &str = "org.kde.KWin";
const SCRIPTING: &str = "org.kde.kwin.Scripting";
const TIMEOUT: Duration = Duration::from_millis(1000);

/// Скрипт `KWin` 6 (`windowActivated`) и `KWin` 5 (`clientActivated`).
fn script() -> String {
    format!(
        r#"function report(window) {{
    callDBus("{NAME}", "/", "{NAME}", "ActiveWindow", window ? String(window.resourceClass) : "");
}}
if (workspace.windowActivated) {{
    workspace.windowActivated.connect(report);
    report(workspace.activeWindow);
}} else {{
    workspace.clientActivated.connect(report);
    report(workspace.activeClient);
}}
"#
    )
}

/// Окна самого рабочего стола: щелчок по трею не должен менять программу,
/// которую предлагает меню.
pub fn is_shell(app: &str) -> bool {
    matches!(
        app,
        "" | "plasmashell" | "org.kde.plasmashell" | "krunner" | "org.kde.krunner" | "kwin_wayland"
    )
}

fn load(connection: &Connection, file: &Path) -> Result<(), dbus::Error> {
    let proxy = connection.with_proxy(KWIN, "/Scripting", TIMEOUT);
    // Скрипт от прошлого запуска: иначе loadScript вернёт -1.
    let _: Result<(bool,), _> = proxy.method_call(SCRIPTING, "unloadScript", (PLUGIN,));
    let path = file.to_string_lossy().to_string();
    let (id,): (i32,) = proxy.method_call(SCRIPTING, "loadScript", (path, PLUGIN))?;
    if id < 0 {
        return Err(dbus::Error::new_failed("loadScript"));
    }
    for object in [format!("/Scripting/Script{id}"), format!("/{id}")] {
        let script = connection.with_proxy(KWIN, object, TIMEOUT);
        if script
            .method_call::<(), _, _, _>("org.kde.kwin.Script", "run", ())
            .is_ok()
        {
            return Ok(());
        }
    }
    Err(dbus::Error::new_failed("run"))
}

fn follow(
    connection: &Connection,
    file: &Path,
    send: &Sender,
    stopped: &AtomicBool,
) -> Result<(), dbus::Error> {
    connection.request_name(NAME, false, true, true)?;
    let forward = send.clone();
    connection.start_receive(
        MatchRule::new_method_call(),
        Box::new(move |call, connection| {
            if call.member().as_deref() == Some("ActiveWindow") {
                let app: Option<String> = call.read1().ok();
                forward(DeviceEvent::App(app.filter(|app| !app.is_empty())));
            }
            let _ = dbus::channel::Sender::send(connection, call.method_return());
            true
        }),
    );
    std::fs::write(file, script()).map_err(|err| dbus::Error::new_failed(&err.to_string()))?;
    load(connection, file)?;
    while !stopped.load(Ordering::Relaxed) {
        connection.process(Duration::from_millis(100))?;
    }
    let proxy = connection.with_proxy(KWIN, "/Scripting", TIMEOUT);
    let _: Result<(bool,), _> = proxy.method_call(SCRIPTING, "unloadScript", (PLUGIN,));
    Ok(())
}

/// Фоновый поток слежения; без `KWin` программа остаётся неизвестной.
pub fn watch(
    runtime: &Path,
    connect: impl Fn() -> Result<Connection, dbus::Error> + Send + 'static,
    send: Sender,
    stopped: Arc<AtomicBool>,
) {
    let file = runtime.join("kwin-active-window.js");
    thread::spawn(move || {
        let result = connect().and_then(|connection| follow(&connection, &file, &send, &stopped));
        if let Err(err) = result {
            tr!(
                log!(
                    "punto-rs: активное окно KWin недоступно, исключения программ не работают: {err}"
                ),
                log!("punto-rs: активне вікно KWin недоступне, винятки програм не працюють: {err}")
            );
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_bus::Bus;
    use dbus::channel::Sender as _;
    use std::sync::mpsc;

    /// Поддельный `KWin`: принимает скрипт и сообщает о вызовах в `calls`.
    fn fake_kwin(address: &str, calls: mpsc::Sender<String>, stopped: Arc<AtomicBool>) {
        let connection = Bus::connect(address).unwrap();
        connection.request_name(KWIN, false, true, false).unwrap();
        connection.start_receive(
            MatchRule::new_method_call(),
            Box::new(move |call, connection| {
                let member = call.member().map(|m| m.to_string()).unwrap_or_default();
                let reply = match member.as_str() {
                    "loadScript" => call.method_return().append1(7_i32),
                    "unloadScript" => call.method_return().append1(true),
                    _ => call.method_return(),
                };
                let path = call.path().map(|p| p.to_string()).unwrap_or_default();
                let _ = calls.send(format!("{member} {path}"));
                connection.send(reply).unwrap();
                true
            }),
        );
        thread::spawn(move || {
            while !stopped.load(Ordering::Relaxed) {
                connection.process(Duration::from_millis(10)).unwrap();
            }
        });
    }

    #[test]
    fn test_watch_loads_script_and_forwards_active_window() {
        let bus = Bus::start();
        let stopped = Arc::new(AtomicBool::new(false));
        let kwin_stopped = Arc::new(AtomicBool::new(false));
        let (calls_tx, calls) = mpsc::channel();
        fake_kwin(&bus.address, calls_tx, kwin_stopped.clone());
        let dir = std::env::temp_dir().join(format!("punto-rs-kwin-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (events_tx, events) = mpsc::channel();
        let events_tx = std::sync::Mutex::new(events_tx);
        let send: Sender = Arc::new(move |event| events_tx.lock().unwrap().send(event).is_ok());
        let address = bus.address.clone();
        watch(&dir, move || Bus::connect(&address), send, stopped.clone());
        let next_call = || calls.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(next_call(), "unloadScript /Scripting");
        assert_eq!(next_call(), "loadScript /Scripting");
        assert_eq!(next_call(), "run /Scripting/Script7");
        assert!(
            std::fs::read_to_string(dir.join("kwin-active-window.js"))
                .unwrap()
                .contains("ActiveWindow")
        );
        let client = Bus::connect(&bus.address).unwrap();
        let proxy = client.with_proxy(NAME, "/", TIMEOUT);
        let next_app = || match events.recv_timeout(Duration::from_secs(5)).unwrap() {
            DeviceEvent::App(app) => app,
            other => panic!("{other:?}"),
        };
        proxy
            .method_call::<(), _, _, _>(NAME, "ActiveWindow", ("org.kde.konsole",))
            .unwrap();
        assert_eq!(next_app().as_deref(), Some("org.kde.konsole"));
        proxy
            .method_call::<(), _, _, _>(NAME, "ActiveWindow", ("",))
            .unwrap();
        assert_eq!(next_app(), None);
        stopped.store(true, Ordering::Relaxed);
        assert_eq!(next_call(), "unloadScript /Scripting");
        kwin_stopped.store(true, Ordering::Relaxed);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_watch_without_kwin_logs_and_gives_up() {
        let stopped = Arc::new(AtomicBool::new(false));
        let send: Sender = Arc::new(|_| true);
        watch(
            &std::env::temp_dir(),
            || Err(dbus::Error::new_failed("нет шины")),
            send,
            stopped,
        );
    }

    #[test]
    fn test_script_reports_class_for_kwin_5_and_6() {
        let script = script();
        assert!(script.contains(&format!(
            "callDBus(\"{NAME}\", \"/\", \"{NAME}\", \"ActiveWindow\""
        )));
        assert!(script.contains("workspace.windowActivated.connect(report)"));
        assert!(script.contains("workspace.clientActivated.connect(report)"));
    }

    #[test]
    fn test_shell_windows_not_offered_as_apps() {
        assert!(is_shell("plasmashell"));
        assert!(is_shell(""));
        assert!(!is_shell("org.kde.konsole"));
    }
}
