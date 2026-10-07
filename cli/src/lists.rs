//! Пользовательские списки рядом с конфигом: слова-исключения и программы,
//! в которых ввод не отслеживается. По записи в строке, `#` - комментарий,
//! регистр не важен.

use std::{
    collections::HashSet,
    fs, io,
    path::{Path, PathBuf},
    time::SystemTime,
};

pub const EXCEPTIONS_FILE: &str = "exceptions.txt";
pub const APPS_FILE: &str = "excluded-apps.txt";

/// Файл списка и его содержимое на момент последнего чтения.
pub struct List {
    pub path: PathBuf,
    pub items: HashSet<String>,
    modified: Option<SystemTime>,
}

fn normalize(item: &str) -> Option<String> {
    let item = item.split('#').next().unwrap_or("").trim().to_lowercase();
    (!item.is_empty()).then_some(item)
}

fn modified(path: &Path) -> Option<SystemTime> {
    fs::metadata(path).and_then(|meta| meta.modified()).ok()
}

impl List {
    /// Список из файла `name` в каталоге конфига; нет файла - пустой список.
    pub fn open(config_dir: &Path, name: &str) -> Self {
        let mut list = Self {
            path: config_dir.join(name),
            items: HashSet::new(),
            modified: None,
        };
        list.reload();
        list
    }

    /// Перечитывает файл; `true` - содержимое изменилось.
    pub fn reload(&mut self) -> bool {
        self.modified = modified(&self.path);
        let items: HashSet<String> = fs::read_to_string(&self.path)
            .unwrap_or_default()
            .lines()
            .filter_map(normalize)
            .collect();
        let changed = items != self.items;
        self.items = items;
        changed
    }

    /// Файл изменён снаружи (например, в редакторе) с прошлого чтения.
    pub fn stale(&self) -> bool {
        modified(&self.path) != self.modified
    }

    /// Добавляет запись в конец файла; `Ok(false)` - она уже была.
    pub fn add(&mut self, item: &str) -> io::Result<bool> {
        let Some(item) = normalize(item) else {
            return Ok(false);
        };
        if self.items.contains(&item) {
            return Ok(false);
        }
        let mut text = fs::read_to_string(&self.path).unwrap_or_default();
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str(&item);
        text.push('\n');
        self.write(&text)?;
        Ok(true)
    }

    /// Убирает запись из файла, сохраняя остальные строки и комментарии.
    pub fn remove(&mut self, item: &str) -> io::Result<()> {
        let item = item.to_lowercase();
        let mut text = String::new();
        for line in fs::read_to_string(&self.path).unwrap_or_default().lines() {
            if normalize(line).as_deref() != Some(item.as_str()) {
                text.push_str(line);
                text.push('\n');
            }
        }
        self.write(&text)
    }

    fn write(&mut self, text: &str) -> io::Result<()> {
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir)?;
        }
        fs::write(&self.path, text)?;
        self.reload();
        Ok(())
    }

    /// Записи по алфавиту, для меню.
    pub fn sorted(&self) -> Vec<String> {
        let mut items: Vec<String> = self.items.iter().cloned().collect();
        items.sort();
        items
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_list_add_remove_reload_keeps_comments() {
        let dir = std::env::temp_dir().join(format!("punto-rs-lists-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let mut list = List::open(&dir, EXCEPTIONS_FILE);
        assert!(list.items.is_empty());
        assert!(list.add("Ghbdsn").unwrap());
        assert!(!list.add("ghbdsn").unwrap());
        assert!(!list.add("  # только комментарий").unwrap());
        fs::write(&list.path, "# мої слова\nghbdsn\nDNF # пакетный менеджер\n").unwrap();
        assert!(list.stale());
        assert!(list.reload());
        assert_eq!(list.sorted(), ["dnf", "ghbdsn"]);
        list.remove("GHBDSN").unwrap();
        assert_eq!(
            fs::read_to_string(&list.path).unwrap(),
            "# мої слова\nDNF # пакетный менеджер\n"
        );
        assert_eq!(list.sorted(), ["dnf"]);
        fs::remove_dir_all(&dir).unwrap();
    }
}
