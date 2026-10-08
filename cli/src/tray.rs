//! Значок в трее: пауза, временное отключение, слова и программы-исключения.
//!
//! Ядро не зависит от системы: строит меню из `Item` и выполняет `Action`.
//! Отрисовку берут `tray/sni.rs` (`StatusNotifierItem`, Linux) и `win/tray.rs`.

mod dialogs;
mod hotkey;
pub mod icon;
#[cfg(target_os = "linux")]
pub mod sni;

use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
    time::Duration,
};

use dialogs::{apps_header, ask_word, open_file, words_header};

use crate::{
    engine::{Control, DeviceEvent},
    layout::Lang,
    lists::{APPS_FILE, EXCEPTIONS_FILE, List},
};

/// Состояние демона для трея; меняет главный цикл.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Status {
    pub paused: bool,
    /// Активная программа в списке исключений.
    pub excluded: bool,
    pub layout: Option<Lang>,
    /// Последняя программа с вводом (не сам трей и не панель).
    pub app: Option<String>,
    /// Последнее автоисправленное слово, как оно было на экране.
    pub last_auto: Option<String>,
    /// Комбинация исправления слова.
    pub hotkey: Vec<u16>,
}

/// Общее состояние с номером версии: трей перерисовывается при его смене.
#[derive(Default)]
pub struct Shared {
    status: Mutex<Status>,
    version: AtomicU64,
}

impl Shared {
    pub fn update(&self, change: impl FnOnce(&mut Status)) {
        let mut status = self
            .status
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let before = status.clone();
        change(&mut status);
        if *status != before {
            self.version.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub fn status(&self) -> Status {
        self.status
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    pub fn version(&self) -> u64 {
        self.version.load(Ordering::Relaxed)
    }
}

/// Действие пункта меню.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    TogglePause,
    PauseFor(u64),
    AddWord(String),
    AskWord,
    RemoveWord(String),
    ExcludeApp(String),
    IncludeApp(String),
    OpenWords,
    OpenApps,
    #[cfg_attr(windows, allow(dead_code))]
    SetHotkey(Vec<u16>),
    #[cfg(windows)]
    RecordHotkey,
    #[cfg(not(windows))]
    OpenConfig,
    #[cfg(windows)]
    ToggleAutostart,
    Quit,
}

/// Пункт меню: `action == None` и пустые `children` - надпись или разделитель.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Item {
    pub label: String,
    pub action: Option<Action>,
    pub checked: Option<bool>,
    pub children: Vec<Item>,
}

impl Item {
    pub(crate) fn action(label: String, action: Action) -> Self {
        Self {
            label,
            action: Some(action),
            ..Self::default()
        }
    }
    pub(crate) fn check(label: String, checked: bool, action: Action) -> Self {
        Self {
            checked: Some(checked),
            ..Self::action(label, action)
        }
    }
    fn submenu(label: String, children: Vec<Item>) -> Self {
        Self {
            label,
            children,
            ..Self::default()
        }
    }
    pub fn separator() -> Self {
        Self::default()
    }
    pub fn is_separator(&self) -> bool {
        self.label.is_empty() && self.action.is_none() && self.children.is_empty()
    }
}

/// Отправка события в главный цикл; `false` - демон остановлен.
pub type Sender = Arc<dyn Fn(DeviceEvent) -> bool + Send + Sync>;

pub struct Tray {
    send: Sender,
    pub shared: Arc<Shared>,
    words: List,
    apps: List,
    /// Файл конфига: меню записывает в него выбранную клавишу.
    config: PathBuf,
    stopped: Arc<AtomicBool>,
    /// Номер паузы: таймер снимает только свою паузу.
    pause_epoch: Arc<AtomicU64>,
}

impl Tray {
    pub fn new(send: Sender, shared: Arc<Shared>, config: &Path, stopped: Arc<AtomicBool>) -> Self {
        let config_dir = config.parent().unwrap_or(config);
        let tray = Self {
            send,
            shared,
            words: List::open(config_dir, EXCEPTIONS_FILE),
            apps: List::open(config_dir, APPS_FILE),
            config: config.to_path_buf(),
            stopped,
            pause_epoch: Arc::default(),
        };
        tray.push_lists();
        tray
    }

    fn push_lists(&self) {
        (self.send)(DeviceEvent::Control(Control::Exceptions(Arc::new(
            self.words.items.clone(),
        ))));
        (self.send)(DeviceEvent::Control(Control::ExcludedApps(Arc::new(
            self.apps.items.clone(),
        ))));
    }

    /// Перечитывает списки, изменённые в редакторе; `true` - меню устарело.
    pub fn poll(&mut self) -> bool {
        let mut changed = false;
        if self.words.stale() {
            changed |= self.words.reload();
        }
        if self.apps.stale() {
            changed |= self.apps.reload();
        }
        if changed {
            self.push_lists();
        }
        changed
    }

    pub fn stopped(&self) -> bool {
        self.stopped.load(Ordering::Relaxed)
    }

    /// Подсказка к значку.
    pub fn tooltip(&self) -> String {
        let status = self.shared.status();
        if status.paused {
            tr!("punto-rs: пауза", "punto-rs: пауза").to_string()
        } else if status.excluded {
            let app = status.app.unwrap_or_default();
            tr!(
                format!("punto-rs: не слежу в «{app}»"),
                format!("punto-rs: не стежу в «{app}»")
            )
        } else {
            tr!(
                "punto-rs: исправляю раскладку",
                "punto-rs: виправляю розкладку"
            )
            .to_string()
        }
    }

    pub fn icon(&self) -> Vec<u8> {
        let status = self.shared.status();
        icon::rgba(status.layout, !status.paused && !status.excluded)
    }

    pub fn menu(&self) -> Vec<Item> {
        let status = self.shared.status();
        let mut menu = vec![
            Item::check(
                tr!("Пауза", "Пауза").into(),
                status.paused,
                Action::TogglePause,
            ),
            Item::submenu(
                tr!("Выключить на…", "Вимкнути на…").into(),
                vec![
                    Item::action(tr!("15 минут", "15 хвилин").into(), Action::PauseFor(15)),
                    Item::action(tr!("1 час", "1 годину").into(), Action::PauseFor(60)),
                    Item::action(tr!("4 часа", "4 години").into(), Action::PauseFor(240)),
                ],
            ),
            Item::separator(),
        ];
        if let Some(word) = status
            .last_auto
            .filter(|word| !self.words.items.contains(word))
        {
            menu.push(Item::action(
                tr!(
                    format!("Не исправлять «{word}»"),
                    format!("Не виправляти «{word}»")
                ),
                Action::AddWord(word),
            ));
        }
        menu.push(Item::action(
            tr!("Добавить слово-исключение…", "Додати слово-виняток…").into(),
            Action::AskWord,
        ));
        menu.push(Self::list_menu(
            tr!("Слова-исключения", "Слова-винятки"),
            &self.words,
            Action::RemoveWord,
            Action::OpenWords,
        ));
        menu.push(Item::separator());
        if let Some(app) = status.app {
            let lower = app.to_lowercase();
            menu.push(if self.apps.items.contains(&lower) {
                Item::action(
                    tr!(
                        format!("Снова следить в «{app}»"),
                        format!("Знову стежити в «{app}»")
                    ),
                    Action::IncludeApp(lower),
                )
            } else {
                Item::action(
                    tr!(
                        format!("Не следить в «{app}»"),
                        format!("Не стежити в «{app}»")
                    ),
                    Action::ExcludeApp(lower),
                )
            });
        }
        menu.push(Self::list_menu(
            tr!("Программы-исключения", "Програми-винятки"),
            &self.apps,
            Action::IncludeApp,
            Action::OpenApps,
        ));
        menu.push(Item::separator());
        menu.push(hotkey::menu(&status.hotkey));
        #[cfg(windows)]
        menu.push(Item::check(
            tr!("Запускать вместе с Windows", "Запускати разом з Windows").into(),
            crate::win::autostart::enabled(),
            Action::ToggleAutostart,
        ));
        menu.push(Item::action(tr!("Выйти", "Вийти").into(), Action::Quit));
        menu
    }

    /// Подменю списка: записи (щелчок убирает) и правка файла.
    fn list_menu(label: &str, list: &List, remove: fn(String) -> Action, open: Action) -> Item {
        let mut children: Vec<Item> = list
            .sorted()
            .into_iter()
            .map(|item| Item::action(format!("✕ {item}"), remove(item)))
            .collect();
        if !children.is_empty() {
            children.push(Item::separator());
        }
        children.push(Item::action(
            tr!("Открыть файл…", "Відкрити файл…").into(),
            open,
        ));
        Item::submenu(label.into(), children)
    }

    pub fn activate(&mut self, action: Action) {
        let result = match action {
            Action::TogglePause => {
                self.pause_epoch.fetch_add(1, Ordering::Relaxed);
                let paused = !self.shared.status().paused;
                (self.send)(DeviceEvent::Control(Control::Pause(paused)));
                Ok(())
            }
            Action::PauseFor(minutes) => {
                self.pause_for(Duration::from_secs(minutes * 60));
                Ok(())
            }
            Action::AddWord(word) => self.words.add(&word).map(|_| ()),
            Action::AskWord => {
                if let Some(word) = ask_word(&self.shared.status().last_auto.unwrap_or_default()) {
                    self.words.add(&word).map(|_| ())
                } else {
                    Ok(())
                }
            }
            Action::RemoveWord(word) => self.words.remove(&word),
            Action::ExcludeApp(app) => self.apps.add(&app).map(|_| ()),
            Action::IncludeApp(app) => self.apps.remove(&app),
            Action::OpenWords => open_file(&self.words.path, &words_header()),
            Action::OpenApps => open_file(&self.apps.path, &apps_header()),
            Action::SetHotkey(combo) => self.set_hotkey(combo),
            #[cfg(windows)]
            Action::RecordHotkey => self.record_hotkey(),
            #[cfg(not(windows))]
            Action::OpenConfig => open_file(&self.config, ""),
            #[cfg(windows)]
            Action::ToggleAutostart => {
                crate::win::autostart::set(!crate::win::autostart::enabled())
            }
            Action::Quit => {
                self.stopped.store(true, Ordering::Relaxed);
                Ok(())
            }
        };
        if let Err(err) = result {
            tr!(
                log!("punto-rs: действие меню не выполнено: {err}"),
                log!("punto-rs: дію меню не виконано: {err}")
            );
        }
        self.push_lists();
    }

    /// Пауза на `duration`; ручное снятие или новая пауза отменяют таймер.
    fn pause_for(&self, duration: Duration) {
        let epoch = self.pause_epoch.fetch_add(1, Ordering::Relaxed) + 1;
        (self.send)(DeviceEvent::Control(Control::Pause(true)));
        let current = self.pause_epoch.clone();
        let send = self.send.clone();
        thread::spawn(move || {
            thread::sleep(duration);
            if current.load(Ordering::Relaxed) == epoch {
                send(DeviceEvent::Control(Control::Pause(false)));
            }
        });
    }
}

#[cfg(test)]
mod tests;
