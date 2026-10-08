//! Выполнение коррекции под захватом клавиатур: слово, придержанный Enter
//! или выделенный текст.

use std::io;

use super::{Capture, wait_for_input};
use crate::{
    config::Config,
    engine::Engine,
    injector::{Injector, KeyOutput},
    keys,
    platform::Grabs,
    state::Stroke,
};

/// Коррекция: нажатия слова, придержанный Enter или выделенный текст.
pub(super) struct Job {
    pub kind: &'static str,
    pub strokes: Vec<Stroke>,
    pub enter: bool,
    pub selection: bool,
}

/// Выполняет `job` под захватом клавиатур и переигрывает ввод под ним.
pub(super) fn correct<T: KeyOutput>(
    job: &Job,
    mut capture: Capture,
    grabs: &Grabs,
    injector: &mut Injector<T>,
    engine: &mut Engine,
    cfg: &Config,
) -> io::Result<()> {
    let grab = match grabs.grab() {
        Ok(grab) => grab,
        Err(err) => {
            tr!(
                log!("punto-rs: клавиатуры не захвачены, исправление пропущено: {err}"),
                log!("punto-rs: клавіатури не захоплено, виправлення пропущено: {err}")
            );
            engine.invalidate();
            if job.enter {
                injector.press(keys::KEY_ENTER)?;
            }
            return Ok(());
        }
    };
    let result = if job.selection {
        convert_selection(injector, engine, &mut capture, cfg)
    } else {
        injector
            .fix(&job.strokes, cfg, |pause| {
                wait_for_input(&mut capture, engine, cfg, pause)
            })
            .map(|()| !job.strokes.is_empty())
    };
    match result {
        Ok(switched) => {
            if switched {
                engine.switched();
            }
        }
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
    if job.enter {
        injector.press(keys::KEY_ENTER)?;
    }
    capture.replay(grab, injector, engine, cfg)?;
    grabs.hold_enter(cfg!(windows) && engine.wants_enter(cfg));
    Ok(())
}

/// Конвертирует выделенный текст; `true` - раскладка переключена.
#[cfg(windows)]
fn convert_selection<T: KeyOutput>(
    injector: &mut Injector<T>,
    engine: &mut Engine,
    capture: &mut Capture,
    cfg: &Config,
) -> io::Result<bool> {
    let pair = engine.pair();
    crate::win::selection::convert(injector, pair, cfg, |pause| {
        wait_for_input(capture, engine, cfg, pause)
    })
}

/// В Linux движок не просит конвертировать выделение.
#[cfg(not(windows))]
#[allow(clippy::unnecessary_wraps)]
fn convert_selection<T: KeyOutput>(
    _: &mut Injector<T>,
    _: &mut Engine,
    _: &mut Capture,
    _: &Config,
) -> io::Result<bool> {
    Ok(false)
}
