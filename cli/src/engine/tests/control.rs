//! Команды трея и активная программа.

use std::{collections::HashSet, sync::Arc};

use super::*;

fn set(items: &[&str]) -> Arc<HashSet<String>> {
    Arc::new(items.iter().map(|item| (*item).to_string()).collect())
}

fn typed_privet(h: &mut Harness) -> Option<PendingFix> {
    h.type_codes(&PRIVET_ON_EN);
    h.tap(keys::KEY_SPACE);
    h.ready()
}

#[test]
fn test_control_pause_from_tray_stops_and_resumes_tracking() {
    let mut h = Harness::on_layout(Some(EN_RU));
    h.event(DeviceEvent::Control(Control::Pause(true)));
    assert!(h.engine.paused);
    assert!(typed_privet(&mut h).is_none());
    h.event(DeviceEvent::Control(Control::Pause(false)));
    let fix = typed_privet(&mut h).unwrap();
    assert!(fix.auto);
    assert_eq!(h.engine.last_auto.as_deref(), Some("ghbdtn"));
}

#[test]
fn test_control_excluded_app_not_tracked_case_insensitive() {
    let mut h = Harness::on_layout(Some(EN_RU));
    h.event(DeviceEvent::Control(Control::ExcludedApps(set(&[
        "org.kde.konsole",
    ]))));
    h.event(DeviceEvent::App(Some("org.kde.Konsole".into())));
    assert!(h.engine.excluded());
    assert!(typed_privet(&mut h).is_none());
    assert!(h.fix().is_none());
    h.event(DeviceEvent::App(Some("firefox".into())));
    assert!(!h.engine.excluded());
    assert_eq!(h.engine.app(), Some("firefox"));
    assert!(typed_privet(&mut h).is_some());
}

#[test]
fn test_control_user_exception_word_kept_and_not_joined() {
    let mut h = Harness::on_layout(Some(EN_RU));
    h.event(DeviceEvent::Control(Control::Exceptions(set(&["ghbdtn"]))));
    assert!(typed_privet(&mut h).is_none());
    // «jy» (он) перед исправляемым словом - исключение, не цепляется.
    let mut h = Harness::on_layout(Some(EN_RU));
    h.event(DeviceEvent::Control(Control::Exceptions(set(&["jy"]))));
    h.type_codes(&[36, 21, keys::KEY_SPACE]);
    let fix = typed_privet(&mut h).unwrap();
    assert_eq!(fix.strokes.len(), PRIVET_ON_EN.len() + 1);
}

#[test]
fn test_control_applies_in_old_generation_too() {
    let mut h = Harness::on_layout(Some(EN_RU));
    h.engine
        .discard(&DeviceEvent::Control(Control::Pause(true)));
    assert!(h.engine.paused);
    h.engine.discard(&DeviceEvent::App(Some("code".into())));
    assert_eq!(h.engine.app(), Some("code"));
    assert_eq!(h.engine.shown_lang(), Some(layout::Lang::En));
}

#[test]
fn test_control_hotkey_from_tray_replaces_word_hotkey() {
    let mut h = Harness::new();
    h.event(DeviceEvent::Control(Control::Hotkey(vec![119])));
    assert_eq!(h.engine.hotkey, [119]);
    h.tap(16);
    assert!(h.fix().is_none());
    h.tap(16);
    h.tap(119);
    assert_eq!(h.ready().unwrap().strokes.len(), 1);
}

#[test]
fn test_hotkey_with_empty_buffer_converts_selection_on_windows_only() {
    let mut h = Harness::new();
    h.tap(keys::KEY_INSERT);
    let fix = h.ready();
    assert_eq!(
        fix.as_ref().map(|fix| fix.selection),
        cfg!(windows).then_some(true)
    );
    assert!(fix.is_none_or(|fix| fix.strokes.is_empty() && !fix.phrase));
    // Набранное слово исправляется как раньше, не выделение.
    h.tap(16);
    assert!(!h.fix().unwrap().selection);
}
