//! Enter, придержанный хуком (Windows).

use super::*;

#[test]
fn test_held_enter_fixes_wrong_word_without_space_and_ends_phrase() {
    let mut h = Harness::on_layout(Some(EN_UK));
    h.type_codes(&PRYVIT_ON_EN);
    assert!(h.engine.wants_enter(&h.cfg));
    let strokes = h.engine.held_enter(&h.cfg, h.now);
    let codes: Vec<u16> = strokes.iter().map(|stroke| stroke.code).collect();
    assert_eq!(codes, PRYVIT_ON_EN);
    assert!(!h.engine.wants_enter(&h.cfg));
    assert!(h.engine.held_enter(&h.cfg, h.now).is_empty());
    // После пробела слово уже исправлено на пробеле: Enter не придерживается.
    h.type_codes(&PRYVIT_ON_EN);
    h.tap(keys::KEY_SPACE);
    assert!(!h.engine.wants_enter(&h.cfg));
}
