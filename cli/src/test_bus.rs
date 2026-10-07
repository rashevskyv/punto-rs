//! Приватная session-шина D-Bus для тестов модулей KDE.
// Тестовая утилита: сбой запуска шины - сбой теста.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use dbus::blocking::Connection;

/// Приватная шина: `dbus-daemon` на время теста, убивается при drop.
pub struct Bus {
    daemon: std::process::Child,
    pub address: String,
}
impl Bus {
    pub fn start() -> Self {
        use std::io::BufRead;
        let mut daemon = std::process::Command::new("dbus-daemon")
            .args(["--session", "--nofork", "--print-address=1"])
            .stdout(std::process::Stdio::piped())
            .spawn()
            .expect("нужен dbus-daemon");
        let mut address = String::new();
        std::io::BufReader::new(daemon.stdout.take().unwrap())
            .read_line(&mut address)
            .unwrap();
        Self {
            daemon,
            address: address.trim().to_string(),
        }
    }
    pub fn connect(address: &str) -> Result<Connection, dbus::Error> {
        let mut channel = dbus::channel::Channel::open_private(address)?;
        channel.register()?;
        Ok(Connection::from(channel))
    }
}
impl Drop for Bus {
    fn drop(&mut self) {
        let _ = self.daemon.kill();
        let _ = self.daemon.wait();
    }
}
