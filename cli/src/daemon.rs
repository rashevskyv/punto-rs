//! Цикл демона: события устройств -> движок -> коррекция под захватом клавиатур.

use std::{
    io,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, RecvTimeoutError, TryRecvError},
    },
    time::{Duration, Instant, SystemTime},
};

use crate::{
    config::Config,
    engine::{DeviceEvent, Engine},
    injector::{Injector, KeyOutput, Pause},
    keys,
    platform::{Grab, Grabs, SessionGuard},
    state::Stroke,
    tray::Shared,
};

const CONTROL_INTERVAL: Duration = Duration::from_millis(10);

/// Событие устройства с поколением сессии, в котором оно прочитано.
pub struct Message {
    pub generation: u64,
    pub event: DeviceEvent,
    /// Время события по ядру: отделяет ввод до захвата клавиатур от ввода под ним.
    pub at: SystemTime,
}

/// Ввод во время коррекции. Клавиатуры захвачены с `grabbed_at`: их нажатия
/// не дошли до композитора и копятся в `queue`, чтобы переиграть их после.
struct Capture<'a> {
    rx: &'a Receiver<Message>,
    guard: &'a SessionGuard,
    stopped: &'a AtomicBool,
    generation: u64,
    grabbed_at: SystemTime,
    queue: Vec<Message>,
    /// KDE сообщила о новой раскладке после захвата.
    switched: bool,
}

impl Capture<'_> {
    fn interrupted(&self) -> bool {
        self.stopped.load(Ordering::Relaxed) || self.guard.context().generation != self.generation
    }

    /// Переигрывает перехваченный ввод через `injector`, скармливает его движку
    /// и снимает `grab`. Захват держится, пока переигранные клавиши не отпущены:
    /// отпускание под захватом иначе не дойдёт до композитора.
    fn replay<T: KeyOutput>(
        mut self,
        grab: Grab,
        injector: &mut Injector<T>,
        engine: &mut Engine,
        cfg: &Config,
    ) -> io::Result<()> {
        let mut queued = std::mem::take(&mut self.queue).into_iter();
        loop {
            let message = match queued.next() {
                Some(message) => message,
                None if !injector.holding() || self.interrupted() => break,
                None => match self.rx.recv_timeout(CONTROL_INTERVAL) {
                    Ok(message) => message,
                    Err(RecvTimeoutError::Timeout) => continue,
                    Err(RecvTimeoutError::Disconnected) => break,
                },
            };
            let captured = message.at >= self.grabbed_at;
            self.deliver(message, captured, injector, engine, cfg)?;
        }
        drop(grab);
        let released_at = SystemTime::now();
        // Ввод под захватом, который поток устройства ещё не успел передать, и
        // отпускание переигранных из него клавиш: оно приходит уже без захвата.
        // Без него виртуальная клавиша остаётся зажатой, и композитор глотает
        // ту же клавишу с настоящей клавиатуры.
        loop {
            let message = match self.rx.recv_timeout(CONTROL_INTERVAL) {
                Ok(message) => message,
                Err(RecvTimeoutError::Timeout) if injector.holding() && !self.interrupted() => {
                    continue;
                }
                Err(_) => break,
            };
            let captured = (self.grabbed_at..released_at).contains(&message.at);
            self.deliver(message, captured, injector, engine, cfg)?;
            if !captured && !injector.holding() {
                break;
            }
        }
        injector.release_all()
    }

    /// Передаёт событие движку; `captured` - композитор его не видел, и
    /// клавишу надо переиграть. Отпускание клавиши, которую держит
    /// виртуальная клавиатура, переигрывается всегда.
    fn deliver<T: KeyOutput>(
        &self,
        message: Message,
        captured: bool,
        injector: &mut Injector<T>,
        engine: &mut Engine,
        cfg: &Config,
    ) -> io::Result<()> {
        match &message.event {
            DeviceEvent::Key(key) if captured || key.value == 0 => {
                injector.forward(key.code, key.value)?;
            }
            DeviceEvent::HeldEnter => injector.press(keys::KEY_ENTER)?,
            _ => {}
        }
        if message.generation == self.generation {
            engine.observe(message.event, cfg, Instant::now());
        } else {
            engine.discard(&message.event);
        }
        Ok(())
    }
}

/// Передаёт трею состояние движка. Окна оболочки (панель, трей) не
/// становятся программой, которую предлагает меню.
fn report(engine: &Engine, shared: &Shared, is_shell: fn(&str) -> bool) {
    shared.update(|status| {
        status.paused = engine.paused;
        status.excluded = engine.excluded();
        status.layout = engine.shown_lang();
        status.last_auto.clone_from(&engine.last_auto);
        if let Some(app) = engine.app().filter(|app| !is_shell(app)) {
            status.app = Some(app.to_string());
        }
    });
}

/// Передаёт движку событие текущего (`current`) или старого поколения.
/// Придержанный Enter возвращает нажатия слова, исправляемого до него.
fn observe(
    engine: &mut Engine,
    message: Message,
    current: bool,
    cfg: &Config,
) -> Option<Vec<Stroke>> {
    if matches!(message.event, DeviceEvent::HeldEnter) {
        return Some(engine.held_enter(cfg, Instant::now()));
    }
    let paused = engine.paused;
    if current {
        engine.observe(message.event, cfg, Instant::now());
    } else {
        engine.discard(&message.event);
    }
    if engine.paused != paused {
        log!(
            "punto-rs: {}",
            if engine.paused {
                tr!("пауза включена", "паузу ввімкнено")
            } else {
                tr!("пауза выключена", "паузу вимкнено")
            }
        );
    }
    None
}

/// Главный цикл: копит ввод в движке и выполняет готовые коррекции.
/// Возвращает `Ok` при остановке и ошибку записи в uinput.
#[allow(clippy::too_many_arguments)]
pub fn run<T: KeyOutput>(
    rx: &Receiver<Message>,
    mut injector: Injector<T>,
    cfg: &Config,
    verbose: bool,
    guard: &SessionGuard,
    grabs: &Grabs,
    stopped: &AtomicBool,
    shared: &Shared,
) -> io::Result<()> {
    let mut engine = Engine::new(cfg, Instant::now());
    let mut generation = u64::MAX;
    loop {
        if stopped.load(Ordering::Relaxed) {
            return Ok(());
        }
        let context = guard.context();
        if generation != context.generation {
            generation = context.generation;
            log!(
                "punto-rs: {}",
                if context.session.is_some() {
                    tr!("локальная сессия доступна", "локальний сеанс доступний")
                } else {
                    tr!(
                        "коррекция приостановлена: сессия недоступна или заблокирована",
                        "виправлення призупинено: сеанс недоступний або заблокований"
                    )
                }
            );
            engine.observe(DeviceEvent::Session(context.session), cfg, Instant::now());
        }
        let held_enter = match rx.recv_timeout(CONTROL_INTERVAL) {
            Ok(message) => {
                let current =
                    message.generation == generation && guard.context().generation == generation;
                let held_enter = observe(&mut engine, message, current, cfg);
                grabs.hold_enter(cfg!(windows) && engine.wants_enter(cfg));
                held_enter
            }
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => return Ok(()),
        };
        report(&engine, shared, crate::platform::is_shell);
        // Придержанный Enter нажимается после исправления, даже пустого или прерванного.
        let fix = match held_enter {
            Some(strokes) => Some(("Enter", strokes, true)),
            None => engine
                .take_ready(cfg, Instant::now())
                .map(|fix| (fix.kind(), fix.strokes, false)),
        };
        if let Some((kind, strokes, enter)) = fix {
            if verbose && !strokes.is_empty() {
                let count = strokes.len();
                tr!(
                    log!("punto-rs: исправляю {count} нажатий ({kind})"),
                    log!("punto-rs: виправляю {count} натискань ({kind})")
                );
            }
            let grabbed_at = SystemTime::now();
            let grab = match grabs.grab() {
                Ok(grab) => grab,
                Err(err) => {
                    tr!(
                        log!("punto-rs: клавиатуры не захвачены, исправление пропущено: {err}"),
                        log!("punto-rs: клавіатури не захоплено, виправлення пропущено: {err}")
                    );
                    engine.invalidate();
                    if enter {
                        injector.press(keys::KEY_ENTER)?;
                    }
                    continue;
                }
            };
            let mut capture = Capture {
                rx,
                guard,
                stopped,
                generation,
                grabbed_at,
                queue: Vec::new(),
                switched: false,
            };
            let result = injector.fix(&strokes, cfg, |pause| {
                wait_for_input(&mut capture, &mut engine, cfg, pause)
            });
            match result {
                Ok(()) if strokes.is_empty() => {}
                Ok(()) => engine.switched(),
                Err(err) if err.kind() == io::ErrorKind::Interrupted => {
                    engine.invalidate();
                    log!(
                        "punto-rs: {}",
                        tr!(
                            "коррекция прервана; буфер сброшен, текст мог быть изменён частично",
                            "виправлення перервано; буфер скинуто, текст міг змінитися частково"
                        )
                    );
                }
                Err(err) => return Err(err),
            }
            if enter {
                injector.press(keys::KEY_ENTER)?;
            }
            capture.replay(grab, &mut injector, &mut engine, cfg)?;
            grabs.hold_enter(cfg!(windows) && engine.wants_enter(cfg));
        }
    }
}

/// Ждёт `pause`, копя ввод под захватом в очередь. `Interrupted` - коррекцию
/// надо прервать: ввод до захвата, клик, смена сессии или устройств, остановка.
fn wait_for_input(
    capture: &mut Capture,
    engine: &mut Engine,
    cfg: &Config,
    pause: Pause,
) -> io::Result<()> {
    let rx = capture.rx;
    let deadline = Instant::now() + pause.duration();
    loop {
        // Сигнал KDE о новой раскладке приходит после того, как композитор её
        // применил: дальше ждать срок `switch-delay` незачем.
        if capture.switched && matches!(pause, Pause::Switch(_)) {
            return Ok(());
        }
        if capture.interrupted() {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                tr!("завершение или смена сессии", "завершення або зміна сеансу"),
            ));
        }
        let message = match rx.try_recv() {
            Ok(message) => Some(message),
            Err(TryRecvError::Disconnected) => {
                return Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    tr!("поток событий закрыт", "потік подій закрито"),
                ));
            }
            Err(TryRecvError::Empty) => {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    return Ok(());
                }
                match rx.recv_timeout(remaining.min(CONTROL_INTERVAL)) {
                    Ok(message) => Some(message),
                    Err(RecvTimeoutError::Disconnected) => {
                        return Err(io::Error::new(
                            io::ErrorKind::Interrupted,
                            tr!("поток событий закрыт", "потік подій закрито"),
                        ));
                    }
                    Err(RecvTimeoutError::Timeout) => None,
                }
            }
        };
        if let Some(message) = message {
            // Придержанный Enter программа не видела, когда бы он ни пришёл.
            let captured = message.generation == capture.generation
                && match message.event {
                    DeviceEvent::HeldEnter => true,
                    DeviceEvent::Key(_) | DeviceEvent::Layout(_) => {
                        message.at >= capture.grabbed_at
                    }
                    _ => false,
                };
            if captured {
                capture.switched |= matches!(message.event, DeviceEvent::Layout(Some(_)));
                capture.queue.push(message);
                continue;
            }
            if message.generation == capture.generation {
                engine.during_fix(message.event, cfg, Instant::now());
            } else {
                engine.discard(&message.event);
            }
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                tr!("новый ввод", "нове введення"),
            ));
        }
    }
}

// Тесты на захвате и сессии Linux.
#[cfg(test)]
#[cfg(target_os = "linux")]
mod tests;
