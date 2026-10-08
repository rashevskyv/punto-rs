//! Исправление раскладки набранного текста: Linux (evdev/uinput) и Windows
//! (хук клавиатуры/SendInput).
// В Windows без окна консоли; из терминала консоль подключается явно.
#![cfg_attr(windows, windows_subsystem = "windows")]

/// Строка журнала в stderr (под systemd уходит в journald).
/// Ошибку записи некуда сообщить, поэтому она отбрасывается.
macro_rules! log {
    ($($arg:tt)*) => {{
        use std::io::Write as _;
        let _ = writeln!(std::io::stderr(), $($arg)*);
    }};
}

/// Сообщение на языке пользователя: `tr!(русский, украинский)`.
/// Вычисляется только выбранная ветка, поэтому годится и для `log!`/`format!`.
macro_rules! tr {
    ($ru:expr, $uk:expr $(,)?) => {
        if crate::i18n::ukrainian() { $uk } else { $ru }
    };
}

/// Ответ команды в stdout; закрытый канал (`| head`) не ошибка.
macro_rules! say {
    ($($arg:tt)*) => {{
        use std::io::Write as _;
        let _ = writeln!(std::io::stdout(), $($arg)*);
    }};
}

mod config;
mod daemon;
mod engine;
mod i18n;
mod injector;
mod keys;
mod layout;
mod lists;
mod state;
mod tray;

#[cfg(target_os = "linux")]
mod devices;
#[cfg(target_os = "linux")]
mod instance;
#[cfg(target_os = "linux")]
mod kde;
#[cfg(target_os = "linux")]
mod kwin;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
mod session;
#[cfg(all(test, target_os = "linux"))]
mod test_bus;
#[cfg(windows)]
mod win;

/// Захват клавиатур на время коррекции и сеанс: свои у каждой системы.
mod platform {
    #[cfg(windows)]
    pub use crate::win::{Grab, Grabs, SessionGuard, is_shell};
    #[cfg(target_os = "linux")]
    pub use crate::{
        devices::{Grab, Grabs},
        kwin::is_shell,
        session::SessionGuard,
    };
}

use std::path::PathBuf;

use config::Config;

/// Каталог XDG из переменной `var`; пустое значение не считается заданным.
pub fn xdg_dir(var: &str) -> Option<PathBuf> {
    std::env::var_os(var)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

/// Конфиг пользователя: `$XDG_CONFIG_HOME/punto-rs/config.conf`,
/// без переменной - `~/.config/punto-rs/config.conf`; в Windows -
/// `%APPDATA%\\punto-rs\\config.conf`.
fn default_config() -> PathBuf {
    let base = if cfg!(windows) {
        xdg_dir("APPDATA")
    } else {
        xdg_dir("XDG_CONFIG_HOME").or_else(|| xdg_dir("HOME").map(|home| home.join(".config")))
    };
    base.unwrap_or_else(|| {
        die(tr!(
            "не заданы XDG_CONFIG_HOME и HOME: укажите конфиг через --config",
            "не задано XDG_CONFIG_HOME і HOME: вкажіть конфіг через --config"
        ))
    })
    .join("punto-rs")
    .join("config.conf")
}

fn main() {
    #[cfg(windows)]
    let console = win::attach_console();
    i18n::apply(i18n::Language::Auto);
    let mut config_path = None;
    let mut explicit_config = false;
    let mut verbose = false;
    let mut check_config = false;
    let mut args = std::env::args();
    let _program = args.next();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-c" | "--config" => {
                config_path = Some(args.next().map_or_else(
                    || {
                        die(tr!(
                            "--config требует путь к файлу",
                            "--config потребує шлях до файлу"
                        ))
                    },
                    PathBuf::from,
                ));
                explicit_config = true;
            }
            "-v" | "--verbose" => verbose = true,
            "--check-config" => check_config = true,
            #[cfg(target_os = "linux")]
            "--check-session" => {
                match session::check() {
                    Ok(Some(id)) => {
                        tr!(
                            say!("punto-rs: локальная графическая сессия {id} доступна"),
                            say!("punto-rs: локальний графічний сеанс {id} доступний")
                        );
                    }
                    Ok(None) => die(tr!(
                        "локальная графическая сессия недоступна, заблокирована или не поддерживается",
                        "локальний графічний сеанс недоступний, заблокований або не підтримується"
                    )),
                    Err(err) => die(&tr!(
                        format!("проверка logind: {err}"),
                        format!("перевірка logind: {err}")
                    )),
                }
                return;
            }
            #[cfg(target_os = "linux")]
            "-l" | "--list-devices" => {
                devices::list_devices();
                return;
            }
            "-V" | "--version" => {
                say!("punto-rs {}", env!("CARGO_PKG_VERSION"));
                return;
            }
            "-h" | "--help" => {
                print_help();
                return;
            }
            other => die(&tr!(
                format!("неизвестный аргумент: {other}"),
                format!("невідомий аргумент: {other}")
            )),
        }
    }
    let config_path = config_path.unwrap_or_else(default_config);
    let cfg = if !explicit_config && !check_config && matches!(config_path.try_exists(), Ok(false))
    {
        tr!(
            log!(
                "punto-rs: {} отсутствует, используются значения по умолчанию",
                config_path.display()
            ),
            log!(
                "punto-rs: {} відсутній, використовуються типові значення",
                config_path.display()
            )
        );
        Config::default()
    } else {
        if let Some(language) = Config::declared_language(&config_path) {
            i18n::apply(language);
        }
        Config::load(&config_path).unwrap_or_else(|err| die(&err))
    };
    i18n::apply(cfg.language);
    if check_config {
        tr!(
            say!("punto-rs: конфиг корректен"),
            say!("punto-rs: конфіг коректний")
        );
        return;
    }
    #[cfg(target_os = "linux")]
    linux::serve(&cfg, verbose, &config_path);
    #[cfg(windows)]
    win::serve(&cfg, verbose, &config_path, console);
}

fn print_help() {
    if i18n::ukrainian() {
        say!(
            "punto-rs — виправлення розкладки набраного тексту\n\nВикористання: punto-rs [опції]\n\n  -c, --config <файл>  конфіг (типово ~/.config/punto-rs/config.conf)\n      --check-config   перевірити конфіг без відкриття пристроїв\n      --check-session  перевірити доступність сеансу через logind\n  -l, --list-devices   показати пристрої введення\n  -v, --verbose        докладний вивід\n  -V, --version        версія\n  -h, --help           довідка\n\nПауза/відновлення: Super+Pause (pause-hotkey).\nДля запуску потрібне членство в групі input (читання /dev/input/*, запис /dev/uinput)\nі сеанс користувача з XDG_RUNTIME_DIR."
        );
        return;
    }
    say!(
        "punto-rs — исправление раскладки набранного текста\n\nИспользование: punto-rs [опции]\n\n  -c, --config <файл>  конфиг (по умолчанию ~/.config/punto-rs/config.conf)\n      --check-config   проверить конфиг без открытия устройств\n      --check-session  проверить доступность сессии через logind\n  -l, --list-devices   показать устройства ввода\n  -v, --verbose        подробный вывод\n  -V, --version        версия\n  -h, --help           справка\n\nПауза/возобновление: Super+Pause (pause-hotkey).\nДля запуска нужно членство в группе input (чтение /dev/input/*, запись /dev/uinput)\nи пользовательская сессия с XDG_RUNTIME_DIR."
    );
}
pub fn die(message: &str) -> ! {
    log!("punto-rs: {message}");
    std::process::exit(2);
}
