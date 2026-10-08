//! Состояние набора и горячих клавиш без доступа к устройствам и часам ОС.

use std::{
    collections::HashSet,
    sync::Arc,
    time::{Duration, Instant},
};

use crate::{
    config::Config,
    keys,
    layout::{self, Pair},
    state::{Buffer, Stroke},
};

#[derive(Debug)]
pub struct KeyEvent {
    pub device_id: u64,
    pub code: u16,
    pub value: i32,
}

// В Windows нет пересинхронизации и отключения устройств evdev.
#[cfg_attr(windows, allow(dead_code))]
#[derive(Debug)]
pub enum DeviceEvent {
    Resynced {
        device_id: u64,
        held_keys: Vec<u16>,
    },
    Key(KeyEvent),
    Click,
    Disconnected(u64),
    Session(Option<String>),
    LostEvents(u64),
    /// Пара раскладок с активной; `None` - неизвестна или не EN/RU, EN/UK.
    Layout(Option<Pair>),
    /// Активная программа (класс окна `KWin` или имя `.exe`); `None` - неизвестна.
    App(Option<String>),
    Control(Control),
    /// Enter, который хук не передал программе (Windows): его надо нажать
    /// заново, после исправления слова перед ним.
    #[cfg_attr(not(windows), allow(dead_code))]
    HeldEnter,
}

/// Команды из трея: пауза и пользовательские списки (в нижнем регистре).
#[derive(Debug)]
pub enum Control {
    Pause(bool),
    Exceptions(Arc<HashSet<String>>),
    ExcludedApps(Arc<HashSet<String>>),
}

#[path = "engine/held.rs"]
mod held;
use held::HeldKeys;

/// Начало исправления слова с `start`: короткие слова перед ним через один
/// пробел в той же чужой раскладке (`F jy` -> `А он`) исправляются вместе с ним.
fn short_words_before(
    phrase: &[Stroke],
    mut start: usize,
    pair: Pair,
    exceptions: &HashSet<String>,
) -> usize {
    while start >= 2 && phrase[start - 1].code == keys::KEY_SPACE {
        let end = start - 1;
        let begin = phrase[..end]
            .iter()
            .rposition(|stroke| keys::is_separator(stroke.code))
            .map_or(0, |index| index + 1);
        let letters: Vec<(u16, bool)> = phrase[begin..end]
            .iter()
            .map(|stroke| (stroke.code, stroke.shift))
            .collect();
        let excepted =
            layout::shown_word(&letters, pair.shown).is_some_and(|word| exceptions.contains(&word));
        if letters.is_empty() || excepted || !layout::short_wrong(&letters, pair) {
            break;
        }
        start = begin;
    }
    start
}

pub struct PendingFix {
    pub strokes: Vec<Stroke>,
    pub phrase: bool,
    /// Найдено детектором на пробеле, а не по хоткею.
    pub auto: bool,
    trigger: u16,
    ready_at: Option<Instant>,
}

impl PendingFix {
    /// Вид исправления для журнала.
    pub fn kind(&self) -> &'static str {
        match (self.auto, self.phrase) {
            (true, _) => tr!("авто", "авто"),
            (false, true) => tr!("фраза", "фраза"),
            (false, false) => tr!("слово", "слово"),
        }
    }
}

pub struct Engine {
    buffer: Buffer,
    held: HeldKeys,
    pending: Option<PendingFix>,
    pub paused: bool,
    session: Option<String>,
    last_input: Instant,
    unsynced: HashSet<u64>,
    layout: Option<Pair>,
    exceptions: Arc<HashSet<String>>,
    excluded_apps: Arc<HashSet<String>>,
    app: Option<String>,
    /// Слово последнего автоисправления, как оно было на экране.
    pub last_auto: Option<String>,
}

impl Engine {
    pub fn new(cfg: &Config, now: Instant) -> Self {
        Self {
            buffer: Buffer::new(cfg.max_strokes),
            held: HeldKeys::default(),
            pending: None,
            paused: false,
            session: (!cfg.session_guard).then(|| "unguarded".to_string()),
            last_input: now,
            unsynced: HashSet::new(),
            layout: None,
            exceptions: Arc::default(),
            excluded_apps: Arc::default(),
            app: None,
            last_auto: None,
        }
    }

    /// Раскладка на экране, если пара известна.
    pub fn shown_lang(&self) -> Option<layout::Lang> {
        self.layout.map(|pair| pair.shown)
    }

    pub fn app(&self) -> Option<&str> {
        self.app.as_deref()
    }

    /// Активная программа в списке исключений: ввод в ней не отслеживается.
    pub fn excluded(&self) -> bool {
        self.app
            .as_ref()
            .is_some_and(|app| self.excluded_apps.contains(&app.to_lowercase()))
    }

    /// Активная программа и команды трея; `true` - событие обработано.
    fn apply_control(&mut self, event: &DeviceEvent) -> bool {
        match event {
            DeviceEvent::App(app) => self.app.clone_from(app),
            DeviceEvent::Control(Control::Pause(paused)) => self.paused = *paused,
            DeviceEvent::Control(Control::Exceptions(words)) => self.exceptions = words.clone(),
            DeviceEvent::Control(Control::ExcludedApps(apps)) => self.excluded_apps = apps.clone(),
            _ => return false,
        }
        self.invalidate();
        true
    }

    /// Слово в пользовательских исключениях: на экране оставляем.
    fn user_exception(&self, letters: &[(u16, bool)], pair: Pair) -> bool {
        layout::shown_word(letters, pair.shown).is_some_and(|word| self.exceptions.contains(&word))
    }

    /// Коррекция переключила раскладку хоткеем: при двух раскладках - на другую.
    /// Сигнал KDE о той же смене после этого не сбрасывает буфер.
    pub fn switched(&mut self) {
        self.layout = self.layout.map(Pair::swapped);
    }

    /// Последнее слово в чужой раскладке, после которого ровно `spaces`
    /// пробелов: начало исправления во фразе и слово, как оно на экране.
    fn wrong_last_word(&self, cfg: &Config, spaces: usize) -> Option<(usize, Option<String>)> {
        let pair = self.layout.filter(|_| cfg.auto_switch)?;
        let word = self.buffer.last_word();
        let letters: Vec<(u16, bool)> = word
            .iter()
            .take_while(|stroke| !keys::is_separator(stroke.code))
            .map(|stroke| (stroke.code, stroke.shift))
            .collect();
        if letters.is_empty()
            || word.len() != letters.len() + spaces
            || self.user_exception(&letters, pair)
            || !layout::wrong_layout(&letters, pair)
        {
            return None;
        }
        let phrase = self.buffer.phrase();
        let start = short_words_before(phrase, phrase.len() - word.len(), pair, &self.exceptions);
        Some((start, layout::shown_word(&letters, pair.shown)))
    }

    /// Пробел после слова в чужой раскладке -> автоматическая коррекция слова с пробелом.
    fn check_last_word(&mut self, cfg: &Config) {
        // Ровно один пробел после слова: второй пробел слово уже не трогает.
        if let Some((start, shown)) = self.wrong_last_word(cfg, 1) {
            self.last_auto = shown;
            self.pending = Some(PendingFix {
                strokes: self.buffer.phrase()[start..].to_vec(),
                phrase: false,
                auto: true,
                trigger: keys::KEY_SPACE,
                ready_at: None,
            });
        }
    }

    /// Придержать ли Enter до исправления: слово перед ним в чужой раскладке.
    pub fn wants_enter(&self, cfg: &Config) -> bool {
        self.pending.is_none() && self.wrong_last_word(cfg, 0).is_some()
    }

    /// Enter, придержанный хуком: нажатия слова перед ним, если его надо
    /// исправить до Enter. Фраза на Enter заканчивается.
    pub fn held_enter(&mut self, cfg: &Config, now: Instant) -> Vec<Stroke> {
        self.expire(cfg, now);
        let strokes = self
            .wrong_last_word(cfg, 0)
            .map(|(start, shown)| {
                self.last_auto = shown;
                self.buffer.phrase()[start..].to_vec()
            })
            .unwrap_or_default();
        self.invalidate();
        strokes
    }

    fn observe_device_state(&mut self, event: &DeviceEvent) {
        self.held.observe(event);
        match event {
            DeviceEvent::LostEvents(id) => {
                self.unsynced.insert(*id);
            }
            DeviceEvent::Resynced { device_id, .. } | DeviceEvent::Disconnected(device_id) => {
                self.unsynced.remove(device_id);
            }
            _ => {}
        }
    }

    pub fn invalidate(&mut self) {
        self.buffer.clear();
        self.pending = None;
    }

    pub fn expire(&mut self, cfg: &Config, now: Instant) {
        if now.duration_since(self.last_input) >= Duration::from_millis(cfg.buffer_timeout_ms) {
            self.invalidate();
        }
    }

    /// Клавиша, пока коррекция ждёт отпускания всех клавиш.
    fn observe_pending(&mut self, event: &KeyEvent, now: Instant) {
        let Some(pending) = &mut self.pending else {
            return;
        };
        // Следующее слово, начатое до отпускания пробела, уже на экране в той же
        // чужой раскладке: перенабирается вместе с исправляемым.
        let typed =
            !self.held.command() && (keys::is_char(event.code) || keys::is_separator(event.code));
        if pending.auto && event.value == 1 && (typed || keys::is_shift(event.code)) {
            if typed {
                let shift = self.held.shift();
                self.buffer.push(event.code, shift);
                pending.strokes.push(Stroke {
                    code: event.code,
                    shift,
                });
            }
            pending.ready_at = None;
            self.last_input = now;
        } else if event.value == 1
            || (event.value == 2 && (pending.auto || event.code != pending.trigger))
        {
            self.invalidate();
        } else if self.held.keys.is_empty() {
            pending.ready_at = Some(now + Duration::from_millis(30));
        }
    }

    pub fn observe(&mut self, event: DeviceEvent, cfg: &Config, now: Instant) {
        self.expire(cfg, now);
        self.observe_device_state(&event);
        if let DeviceEvent::Session(session) = event {
            self.session = session;
            self.invalidate();
            return;
        }
        if let DeviceEvent::Layout(layout) = event {
            if self.layout != layout {
                self.layout = layout;
                self.invalidate();
            }
            return;
        }
        if self.apply_control(&event) {
            return;
        }
        let DeviceEvent::Key(event) = event else {
            self.invalidate();
            return;
        };
        if event.value == 1
            && cfg.pause_hotkey.last() == Some(&event.code)
            && self.held.matches(&cfg.pause_hotkey)
        {
            self.paused = !self.paused;
            self.invalidate();
            return;
        }
        if self.paused || self.excluded() || self.session.is_none() || !self.unsynced.is_empty() {
            self.invalidate();
            return;
        }
        if self.pending.is_some() {
            self.observe_pending(&event, now);
            return;
        }
        if event.value == 0 {
            return;
        }
        self.last_input = now;
        // Повтор на уровне evdev не гарантирует столько же символов в Wayland.
        if event.value != 1 {
            self.invalidate();
            return;
        }
        let phrase = if cfg.phrase_hotkey.last() == Some(&event.code)
            && self.held.matches(&cfg.phrase_hotkey)
        {
            Some(true)
        } else if cfg.hotkey.last() == Some(&event.code) && self.held.matches(&cfg.hotkey) {
            Some(false)
        } else {
            None
        };
        if let Some(phrase) = phrase {
            let strokes = if phrase {
                self.buffer.phrase()
            } else {
                self.buffer.last_word()
            };
            if !strokes.is_empty() {
                self.pending = Some(PendingFix {
                    strokes: strokes.to_vec(),
                    phrase,
                    auto: false,
                    trigger: event.code,
                    ready_at: None,
                });
            }
            return;
        }
        if self.held.matches(&cfg.layout_switch) {
            self.invalidate();
            return;
        }
        if keys::is_shift(event.code) || keys::is_command_modifier(event.code) {
            return;
        }
        if self.held.command() {
            self.invalidate();
        } else if event.code == keys::KEY_BACKSPACE {
            self.buffer.backspace();
        } else if event.code == keys::KEY_TAB || keys::is_phrase_end(event.code) {
            self.invalidate();
        } else if keys::is_char(event.code) || keys::is_separator(event.code) {
            self.buffer.push(event.code, self.held.shift());
            if event.code == keys::KEY_SPACE {
                self.check_last_word(cfg);
            }
        } else {
            self.invalidate();
        }
    }

    pub fn take_ready(&mut self, cfg: &Config, now: Instant) -> Option<PendingFix> {
        self.expire(cfg, now);
        if self
            .pending
            .as_ref()
            .is_some_and(|fix| fix.ready_at.is_some_and(|at| at <= now))
            && self.held.keys.is_empty()
        {
            self.pending.take()
        } else {
            None
        }
    }

    pub fn discard(&mut self, event: &DeviceEvent) {
        self.observe_device_state(event);
        // Раскладка - не ввод: её смена в старом поколении сессии всё равно действует.
        if let DeviceEvent::Layout(layout) = event {
            self.layout = *layout;
        }
        // Команды трея и смена программы тоже действуют в любом поколении.
        self.apply_control(event);
        self.invalidate();
    }

    pub fn during_fix(&mut self, event: DeviceEvent, cfg: &Config, now: Instant) {
        self.invalidate();
        self.observe(event, cfg, now);
        self.invalidate();
    }
}

#[cfg(test)]
mod tests;
