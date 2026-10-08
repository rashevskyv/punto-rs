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

/// Окно «нажмите комбинацию» (Windows Forms): модификаторы `ctrl+shift+alt`
/// и код VK клавиши. Клавишу, печатающую символ, принимает только с Ctrl
/// или Alt: иначе она перестанет печатать. Esc или закрытие - `None`.
#[cfg(windows)]
pub(super) fn ask_hotkey() -> Option<(String, u32)> {
    use std::os::windows::process::CommandExt;
    let prompt = tr!(
        "Нажмите новую клавишу или комбинацию для исправления раскладки.`nEsc - отмена.",
        "Натисніть нову клавішу або комбінацію для виправлення розкладки.`nEsc - скасувати."
    );
    let printable = tr!(
        "Эта клавиша печатает символ: добавьте Ctrl или Alt.`nEsc - отмена.",
        "Ця клавіша друкує символ: додайте Ctrl або Alt.`nEsc - скасувати."
    );
    let script = format!(
        r#"Add-Type -AssemblyName System.Windows.Forms
$f = New-Object System.Windows.Forms.Form
$f.Text = 'punto-rs'; $f.Width = 460; $f.Height = 160; $f.TopMost = $true
$f.StartPosition = 'CenterScreen'; $f.FormBorderStyle = 'FixedDialog'
$f.MaximizeBox = $false; $f.MinimizeBox = $false; $f.KeyPreview = $true
$l = New-Object System.Windows.Forms.Label
$l.Dock = 'Fill'; $l.TextAlign = 'MiddleCenter'; $l.Text = "{prompt}"
$f.Controls.Add($l)
$f.Add_KeyDown({{ param($s, $e)
  $e.SuppressKeyPress = $true
  $k = [int]$e.KeyCode
  if ($k -eq 27) {{ $f.Close(); return }}
  if ($k -in 16, 17, 18, 91, 92) {{ return }}
  $printable = ($k -ge 0x30 -and $k -le 0x5A) -or $k -in 8, 9, 13, 32 -or ($k -ge 0xBA -and $k -le 0xE2)
  if ($printable -and -not ($e.Control -or $e.Alt)) {{ $l.Text = "{printable}"; return }}
  $m = @(); if ($e.Control) {{ $m += 'ctrl' }}; if ($e.Shift) {{ $m += 'shift' }}; if ($e.Alt) {{ $m += 'alt' }}
  [Console]::Out.Write(($m -join '+') + '|' + $k); $f.Close() }})
$f.Add_Shown({{ $f.Activate() }})
[void]$f.ShowDialog()"#
    );
    let output = Command::new("powershell.exe")
        .args(["-NoProfile", "-Command", &script])
        .creation_flags(0x0800_0000)
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    let (modifiers, vk) = text.trim().split_once('|')?;
    Some((modifiers.to_string(), vk.parse().ok()?))
}

/// Сообщение в окне с кнопкой OK.
#[cfg(windows)]
pub(super) fn message(text: &str) {
    use std::os::windows::process::CommandExt;
    let script = format!(
        "Add-Type -AssemblyName System.Windows.Forms; [void][System.Windows.Forms.MessageBox]::Show('{}', 'punto-rs')",
        text.replace('\'', "''")
    );
    let _ = Command::new("powershell.exe")
        .args(["-NoProfile", "-Command", &script])
        .creation_flags(0x0800_0000)
        .status();
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
