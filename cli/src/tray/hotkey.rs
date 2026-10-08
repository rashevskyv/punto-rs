//! Клавиша исправления: выбор в меню трея, запись в конфиг, смена на лету.

use std::{fs, io};

use super::{Action, Item, Tray};
use crate::{
    config::Config,
    engine::{Control, DeviceEvent},
    keys,
};

/// Пункт меню. В Windows - окно, которое запоминает нажатую комбинацию;
/// в Linux такого окна нет, поэтому - клавиши на выбор и правка конфига.
pub(super) fn menu(current: &[u16]) -> Item {
    let name = keys::combo_name(current);
    let label = tr!(
        format!("Клавиша исправления: {name}…"),
        format!("Клавіша виправлення: {name}…")
    );
    #[cfg(windows)]
    return Item::action(label, Action::RecordHotkey);
    #[cfg(not(windows))]
    {
        let mut children: Vec<Item> = ["Insert", "Pause", "ScrollLock", "Menu"]
            .iter()
            .filter_map(|name| keys::parse_combo(name))
            .map(|combo| {
                let checked = combo == current;
                Item::check(keys::combo_name(&combo), checked, Action::SetHotkey(combo))
            })
            .collect();
        children.push(Item::separator());
        children.push(Item::action(
            tr!(
                "Другая комбинация - в конфиге…",
                "Інша комбінація - у конфігу…"
            )
            .into(),
            Action::OpenConfig,
        ));
        Item::submenu(label, children)
    }
}

impl Tray {
    /// Окно записи комбинации; Esc - без изменений.
    #[cfg(windows)]
    pub(super) fn record_hotkey(&self) -> io::Result<()> {
        let Some(combo) = super::dialogs::ask_hotkey()
            .and_then(|(modifiers, vk)| crate::win::vk_combo(&modifiers, vk))
        else {
            return Ok(());
        };
        self.set_hotkey(combo).inspect_err(|err| {
            super::dialogs::message(&tr!(
                format!("Комбинация не сохранена:\n{err}"),
                format!("Комбінацію не збережено:\n{err}")
            ));
        })
    }

    /// Записывает комбинацию в конфиг и сразу применяет. Если конфиг стал
    /// неверным (комбинация совпала с другой), он возвращается как был.
    pub(super) fn set_hotkey(&self, combo: Vec<u16>) -> io::Result<()> {
        let before = fs::read_to_string(&self.config).ok();
        crate::config::set_value(&self.config, "hotkey", &keys::combo_name(&combo))?;
        if let Err(err) = Config::load(&self.config) {
            match before {
                Some(text) => fs::write(&self.config, text)?,
                None => fs::remove_file(&self.config)?,
            }
            return Err(io::Error::other(err));
        }
        #[cfg(windows)]
        crate::win::set_word_hotkey(combo.clone());
        (self.send)(DeviceEvent::Control(Control::Hotkey(combo)));
        Ok(())
    }
}
