use std::sync::Mutex;

use super::*;

/// Трей с записью отправленных событий во временном каталоге.
fn tray(name: &str) -> (Tray, Arc<Mutex<Vec<DeviceEvent>>>, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("punto-rs-tray-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let events = Arc::new(Mutex::new(Vec::new()));
    let log = events.clone();
    let send: Sender = Arc::new(move |event| {
        log.lock().unwrap().push(event);
        true
    });
    let tray = Tray::new(send, Arc::default(), &dir, Arc::default());
    (tray, events, dir)
}

fn labels(items: &[Item]) -> Vec<String> {
    items
        .iter()
        .flat_map(|item| std::iter::once(item.label.clone()).chain(labels(&item.children)))
        .filter(|label| !label.is_empty())
        .collect()
}

#[test]
fn test_tray_menu_offers_last_word_and_current_app() {
    let (tray, events, dir) = tray("menu");
    // Списки уходят движку сразу при создании.
    assert_eq!(events.lock().unwrap().len(), 2);
    tray.shared.update(|status| {
        status.app = Some("Code".into());
        status.last_auto = Some("ghbdtn".into());
        status.layout = Some(Lang::Uk);
    });
    let menu = labels(&tray.menu());
    assert!(
        menu.contains(&"Не исправлять «ghbdtn»".to_string()),
        "{menu:?}"
    );
    assert!(
        menu.contains(&"Не следить в «Code»".to_string()),
        "{menu:?}"
    );
    assert!(menu.contains(&"Выйти".to_string()));
    assert_eq!(tray.icon().len(), icon::SIZE * icon::SIZE * 4);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn test_tray_actions_update_lists_pause_and_stop() {
    let (mut tray, events, dir) = tray("actions");
    tray.activate(Action::ExcludeApp("Code".into()));
    tray.activate(Action::AddWord("ghbdtn".into()));
    assert_eq!(
        std::fs::read_to_string(dir.join(APPS_FILE)).unwrap(),
        "code\n"
    );
    let pushed = events.lock().unwrap().iter().any(|event| {
        matches!(event, DeviceEvent::Control(Control::ExcludedApps(apps)) if apps.contains("code"))
    });
    assert!(pushed);
    tray.shared
        .update(|status| status.app = Some("Code".into()));
    assert!(labels(&tray.menu()).contains(&"Снова следить в «Code»".to_string()));
    tray.activate(Action::IncludeApp("code".into()));
    assert!(
        !labels(&tray.menu())
            .iter()
            .any(|label| label.contains("Снова"))
    );
    tray.activate(Action::TogglePause);
    assert!(matches!(
        events.lock().unwrap().last(),
        Some(DeviceEvent::Control(Control::ExcludedApps(_)))
    ));
    assert!(
        events
            .lock()
            .unwrap()
            .iter()
            .any(|event| matches!(event, DeviceEvent::Control(Control::Pause(true))))
    );
    assert!(!tray.stopped());
    tray.activate(Action::Quit);
    assert!(tray.stopped());
    // Файл, изменённый в редакторе, перечитывается.
    std::fs::write(dir.join(EXCEPTIONS_FILE), "dnf\n").unwrap();
    std::thread::sleep(Duration::from_millis(20));
    assert!(tray.poll());
    assert!(labels(&tray.menu()).contains(&"✕ dnf".to_string()));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn test_tray_timed_pause_resumes_unless_toggled_and_tooltips() {
    let (mut tray, events, dir) = tray("pause");
    let pauses = || -> Vec<bool> {
        events
            .lock()
            .unwrap()
            .iter()
            .filter_map(|event| match event {
                DeviceEvent::Control(Control::Pause(paused)) => Some(*paused),
                _ => None,
            })
            .collect()
    };
    tray.pause_for(Duration::from_millis(20));
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(pauses(), [true, false]);
    // Ручное снятие паузы отменяет таймер.
    tray.pause_for(Duration::from_millis(50));
    tray.activate(Action::TogglePause);
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(pauses(), [true, false, true, true]);
    tray.activate(Action::PauseFor(1));
    assert_eq!(pauses().last(), Some(&true));
    tray.shared.update(|status| status.paused = true);
    assert_eq!(tray.tooltip(), "punto-rs: пауза");
    tray.shared.update(|status| {
        status.paused = false;
        status.excluded = true;
        status.app = Some("Code".into());
    });
    assert_eq!(tray.tooltip(), "punto-rs: не слежу в «Code»");
    tray.shared.update(|status| status.excluded = false);
    assert_eq!(tray.tooltip(), "punto-rs: исправляю раскладку");
    let _ = std::fs::remove_dir_all(dir);
}
