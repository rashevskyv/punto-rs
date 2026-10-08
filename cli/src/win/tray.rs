//! Значок в области уведомлений Windows (`tray-icon`). Очередь сообщений
//! потока обслуживает и значок, и меню.

use std::{
    collections::HashMap,
    ptr::null_mut,
    sync::{Arc, Mutex, MutexGuard, PoisonError, atomic::Ordering},
    thread,
    time::Duration,
};

use tray_icon::{
    Icon, TrayIconBuilder,
    menu::{
        CheckMenuItem, IsMenuItem, Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem, Submenu,
    },
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, MSG, PM_REMOVE, PeekMessageW, TranslateMessage,
};

use crate::tray::{Action, Item, Tray, icon};

fn lock(tray: &Mutex<Tray>) -> MutexGuard<'_, Tray> {
    tray.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Добавляет пункты в меню через `append`, запоминая действия по id.
fn fill(items: &[Item], append: &dyn Fn(&dyn IsMenuItem), actions: &mut HashMap<MenuId, Action>) {
    for item in items {
        if item.is_separator() {
            append(&PredefinedMenuItem::separator());
        } else if !item.children.is_empty() {
            let submenu = Submenu::new(&item.label, true);
            fill(
                &item.children,
                &|child| {
                    let _ = submenu.append(child);
                },
                actions,
            );
            append(&submenu);
        } else if let Some(checked) = item.checked {
            let entry = CheckMenuItem::new(&item.label, true, checked, None);
            if let Some(action) = &item.action {
                actions.insert(entry.id().clone(), action.clone());
            }
            append(&entry);
        } else {
            let entry = MenuItem::new(&item.label, item.action.is_some(), None);
            if let Some(action) = &item.action {
                actions.insert(entry.id().clone(), action.clone());
            }
            append(&entry);
        }
    }
}

fn build(tray: &Tray, actions: &mut HashMap<MenuId, Action>) -> Menu {
    actions.clear();
    let menu = Menu::new();
    fill(
        &tray.menu(),
        &|item| {
            let _ = menu.append(item);
        },
        actions,
    );
    menu
}

fn icon_of(tray: &Tray) -> Option<Icon> {
    let size = u32::try_from(icon::SIZE).unwrap_or(32);
    Icon::from_rgba(tray.icon(), size, size).ok()
}

pub fn run_tray(tray: Tray) {
    let stop = tray.stop_flag();
    let tray = Arc::new(Mutex::new(tray));
    let mut actions = HashMap::new();
    let built = {
        let tray = lock(&tray);
        let mut builder = TrayIconBuilder::new()
            .with_menu(Box::new(build(&tray, &mut actions)))
            .with_tooltip(tray.tooltip());
        if let Some(icon) = icon_of(&tray) {
            builder = builder.with_icon(icon);
        }
        builder.build()
    };
    let icon = match built {
        Ok(icon) => icon,
        Err(err) => {
            tr!(
                log!("punto-rs: значок в трее недоступен: {err}"),
                log!("punto-rs: значок у треї недоступний: {err}")
            );
            return;
        }
    };
    let mut shown = lock(&tray).shared.version();
    loop {
        // SAFETY: обычная выборка очереди сообщений своего потока.
        unsafe {
            let mut message: MSG = std::mem::zeroed();
            while PeekMessageW(&raw mut message, null_mut(), 0, 0, PM_REMOVE) != 0 {
                TranslateMessage(&raw const message);
                DispatchMessageW(&raw const message);
            }
        }
        let mut dirty = false;
        while let Ok(event) = MenuEvent::receiver().try_recv() {
            if let Some(action) = actions.get(&event.id).cloned() {
                // Диалог ввода блокирует: действие - в своём потоке.
                let tray = tray.clone();
                thread::spawn(move || lock(&tray).activate(action));
                dirty = true;
            }
        }
        if stop.load(Ordering::Relaxed) {
            return;
        }
        let (changed, version) = match tray.try_lock() {
            Ok(mut tray) => (tray.poll(), tray.shared.version()),
            Err(_) => (false, shown),
        };
        if changed || dirty || version != shown {
            shown = version;
            if let Ok(tray) = tray.try_lock() {
                icon.set_menu(Some(Box::new(build(&tray, &mut actions))));
                let _ = icon.set_icon(icon_of(&tray));
                let _ = icon.set_tooltip(Some(tray.tooltip()));
            }
        }
        thread::sleep(Duration::from_millis(50));
    }
}
