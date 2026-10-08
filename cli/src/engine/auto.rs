//! Автоисправление последнего слова: на пробеле и перед придержанным Enter.

use std::{collections::HashSet, time::Instant};

use super::{Engine, PendingFix};
use crate::{
    config::Config,
    keys,
    layout::{self, Pair},
    state::Stroke,
};

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

impl Engine {
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
    pub(super) fn check_last_word(&mut self, cfg: &Config) {
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
}
