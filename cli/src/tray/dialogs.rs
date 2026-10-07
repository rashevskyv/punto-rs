//! Внешние окна трея: ввод слова и правка списков в редакторе.

use std::{path::Path, process::Command};

pub(super) fn words_header() -> String {
    tr!(
        "# Слова, которые punto-rs не исправляет сам: как они выглядят на экране,\n# по одному в строке.\n",
        "# Слова, які punto-rs не виправляє сам: як вони виглядають на екрані,\n# по одному в рядку.\n"
    )
    .into()
}

pub(super) fn apps_header() -> String {
    tr!(
        "# Программы, в которых punto-rs не следит за вводом: класс окна KWin\n# (например, org.kde.konsole) или имя .exe в Windows, по одной в строке.\n",
        "# Програми, у яких punto-rs не стежить за введенням: клас вікна KWin\n# (наприклад, org.kde.konsole) або ім'я .exe у Windows, по одній у рядку.\n"
    )
    .into()
}

/// Открывает список в редакторе, создав его с пояснением.
pub(super) fn open_file(path: &Path, header: &str) -> std::io::Result<()> {
    if !path.exists() {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, header)?;
    }
    #[cfg(windows)]
    let mut command = Command::new("notepad.exe");
    #[cfg(not(windows))]
    let mut command = Command::new("xdg-open");
    command.arg(path).spawn().map(|_| ())
}

/// Спрашивает слово в диалоге: `kdialog`/`zenity` в Linux, `InputBox` в Windows.
pub(super) fn ask_word(default: &str) -> Option<String> {
    let prompt = tr!(
        "Слово, которое не нужно исправлять (как на экране):",
        "Слово, яке не треба виправляти (як на екрані):"
    );
    #[cfg(windows)]
    let output = {
        use std::os::windows::process::CommandExt;
        let quote = |text: &str| text.replace('\'', "''");
        let script = format!(
            "Add-Type -AssemblyName Microsoft.VisualBasic; [Microsoft.VisualBasic.Interaction]::InputBox('{}', 'punto-rs', '{}')",
            quote(prompt),
            quote(default)
        );
        Command::new("powershell.exe")
            .args(["-NoProfile", "-Command", &script])
            .creation_flags(0x0800_0000)
            .output()
            .ok()
    };
    #[cfg(not(windows))]
    let output = Command::new("kdialog")
        .args(["--title", "punto-rs", "--inputbox", prompt, default])
        .output()
        .or_else(|_| {
            Command::new("zenity")
                .args(["--entry", "--title=punto-rs"])
                .arg(format!("--text={prompt}"))
                .arg(format!("--entry-text={default}"))
                .output()
        })
        .ok();
    let output = output.filter(|output| output.status.success())?;
    let word = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!word.is_empty()).then_some(word)
}
