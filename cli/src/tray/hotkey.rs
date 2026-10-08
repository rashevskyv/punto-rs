//! Выбор клавиши исправления в меню трея.

use super::{Action, Item};
use crate::keys;

/// Клавиши на выбор: имя в конфиге и подпись. Модификаторы не годятся:
/// хоткей срабатывает на нажатие, а с ними набирают сочетания.
const CHOICES: [(&str, &str); 4] = [
    ("insert", "Insert"),
    ("pause", "Pause"),
    ("scrolllock", "Scroll Lock"),
    ("menu", "Menu"),
];

/// Подменю: клавиши на выбор и правка конфига для своей комбинации.
pub(super) fn menu(current: &[u16]) -> Item {
    let mut chosen = None;
    let mut children: Vec<Item> = CHOICES
        .iter()
        .map(|&(name, label)| {
            let checked = keys::parse_combo(name).as_deref() == Some(current);
            if checked {
                chosen = Some(label);
            }
            Item::check(label.into(), checked, Action::SetHotkey(name))
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
    let chosen = chosen.unwrap_or_else(|| tr!("своя", "своя"));
    Item::submenu(
        tr!(
            format!("Клавиша исправления: {chosen}"),
            format!("Клавіша виправлення: {chosen}")
        ),
        children,
    )
}
