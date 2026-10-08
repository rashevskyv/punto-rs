//! Значок трея по протоколу `StatusNotifierItem` (KDE Plasma и др.) через ksni.

use std::{
    sync::{Arc, Mutex, MutexGuard, PoisonError},
    thread,
    time::Duration,
};

use ksni::{
    Icon, MenuItem, ToolTip,
    blocking::TrayMethods,
    menu::{CheckmarkItem, StandardItem, SubMenu},
};

use super::{Action, Item, Tray, icon};

struct Sni(Arc<Mutex<Tray>>);

fn lock(tray: &Mutex<Tray>) -> MutexGuard<'_, Tray> {
    tray.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Действие в своём потоке: диалог ввода не должен держать D-Bus трея.
fn run(sni: &Sni, action: Action) {
    let tray = sni.0.clone();
    thread::spawn(move || lock(&tray).activate(action));
}

/// `_` в метке ksni считает мнемоникой.
fn label(text: &str) -> String {
    text.replace('_', "__")
}

fn convert(item: &Item) -> MenuItem<Sni> {
    if item.is_separator() {
        return MenuItem::Separator;
    }
    if !item.children.is_empty() {
        return SubMenu {
            label: label(&item.label),
            submenu: item.children.iter().map(convert).collect(),
            ..SubMenu::default()
        }
        .into();
    }
    let Some(action) = item.action.clone() else {
        return StandardItem {
            label: label(&item.label),
            enabled: false,
            ..StandardItem::default()
        }
        .into();
    };
    let activate: Box<dyn Fn(&mut Sni) + Send> = Box::new(move |sni| run(sni, action.clone()));
    match item.checked {
        Some(checked) => CheckmarkItem {
            label: label(&item.label),
            checked,
            activate,
            ..CheckmarkItem::default()
        }
        .into(),
        None => StandardItem {
            label: label(&item.label),
            activate,
            ..StandardItem::default()
        }
        .into(),
    }
}

impl ksni::Tray for Sni {
    const MENU_ON_ACTIVATE: bool = true;

    fn id(&self) -> String {
        "punto-rs".into()
    }

    fn title(&self) -> String {
        "punto-rs".into()
    }

    fn icon_pixmap(&self) -> Vec<Icon> {
        // ARGB32 в сетевом порядке байт.
        let data = lock(&self.0)
            .icon()
            .chunks(4)
            .flat_map(|pixel| [pixel[3], pixel[0], pixel[1], pixel[2]])
            .collect();
        let size = i32::try_from(icon::SIZE).unwrap_or(32);
        vec![Icon {
            width: size,
            height: size,
            data,
        }]
    }

    fn tool_tip(&self) -> ToolTip {
        ToolTip {
            title: lock(&self.0).tooltip(),
            ..ToolTip::default()
        }
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        lock(&self.0).menu().iter().map(convert).collect()
    }
}

/// Держит значок, пока демон работает: перерисовывает его при смене
/// состояния и перечитывает списки, изменённые в редакторе.
pub fn run_tray(tray: Tray) {
    let tray = Arc::new(Mutex::new(tray));
    let handle = match Sni(tray.clone()).spawn() {
        Ok(handle) => handle,
        Err(err) => {
            tr!(
                log!("punto-rs: значок в трее недоступен: {err}"),
                log!("punto-rs: значок у треї недоступний: {err}")
            );
            return;
        }
    };
    let mut shown = u64::MAX;
    loop {
        thread::sleep(Duration::from_millis(250));
        let (stopped, changed, version) = {
            let mut tray = lock(&tray);
            (tray.stopped(), tray.poll(), tray.shared.version())
        };
        if stopped {
            handle.shutdown().wait();
            return;
        }
        if changed || version != shown {
            shown = version;
            handle.update(|_| {});
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ksni::Tray as _;
    use std::sync::atomic::AtomicBool;

    fn sni() -> Sni {
        let dir = std::env::temp_dir().join(format!("punto-rs-sni-{}", std::process::id()));
        let send: super::super::Sender = Arc::new(|_| true);
        let tray = Tray::new(
            send,
            Arc::default(),
            &dir.join("config.conf"),
            Arc::new(AtomicBool::new(false)),
        );
        tray.shared
            .update(|status| status.app = Some("my_app".into()));
        Sni(Arc::new(Mutex::new(tray)))
    }

    #[test]
    fn test_sni_menu_icon_and_tooltip() {
        let sni = sni();
        assert_eq!(sni.id(), "punto-rs");
        assert_eq!(sni.title(), "punto-rs");
        let icon = &sni.icon_pixmap()[0];
        assert_eq!(icon.width, 32);
        assert_eq!(icon.data.len(), 32 * 32 * 4);
        // ARGB: угол прозрачный, центр непрозрачный.
        assert_eq!(icon.data[0], 0);
        assert_eq!(icon.data[(16 * 32 + 16) * 4], 0xff);
        assert!(sni.tool_tip().title.starts_with("punto-rs"));
        let menu = sni.menu();
        assert!(matches!(&menu[0], MenuItem::Checkmark(item) if !item.checked));
        assert!(matches!(&menu[1], MenuItem::SubMenu(sub) if sub.submenu.len() == 3));
        assert!(matches!(&menu[2], MenuItem::Separator));
        let escaped = menu.iter().any(
            |item| matches!(item, MenuItem::Standard(entry) if entry.label.contains("my__app")),
        );
        assert!(escaped);
    }

    #[test]
    fn test_convert_disabled_label_and_action_runs() {
        let mut sni = sni();
        let label = Item {
            label: "info".into(),
            ..Item::default()
        };
        assert!(matches!(
            convert(&Item {
                children: vec![label.clone()],
                ..Item::default()
            }),
            MenuItem::SubMenu(_)
        ));
        assert!(matches!(convert(&label), MenuItem::Standard(entry) if !entry.enabled));
        let pause = Item::action("p".into(), Action::TogglePause);
        let MenuItem::Standard(entry) = convert(&pause) else {
            panic!("ожидался пункт");
        };
        (entry.activate)(&mut sni);
        let quit = Item::check("q".into(), false, Action::Quit);
        let MenuItem::Checkmark(entry) = convert(&quit) else {
            panic!("ожидалась галочка");
        };
        (entry.activate)(&mut sni);
        for _ in 0..100 {
            if lock(&sni.0).stopped() {
                return;
            }
            thread::sleep(Duration::from_millis(10));
        }
        panic!("Quit не выполнен");
    }
}
