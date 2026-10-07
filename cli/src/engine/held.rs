//! Нажатые клавиши всех устройств: для горячих клавиш и модификаторов.

use std::collections::HashSet;

use super::DeviceEvent;
use crate::keys;

#[derive(Default)]
pub(super) struct HeldKeys {
    pub(super) keys: HashSet<(u64, u16)>,
}

impl HeldKeys {
    pub(super) fn observe(&mut self, event: &DeviceEvent) {
        match event {
            DeviceEvent::Resynced {
                device_id,
                held_keys,
            } => {
                self.keys.retain(|(id, _)| id != device_id);
                self.keys
                    .extend(held_keys.iter().map(|code| (*device_id, *code)));
            }
            DeviceEvent::Key(event) => match event.value {
                0 => {
                    self.keys.remove(&(event.device_id, event.code));
                }
                1 => {
                    self.keys.insert((event.device_id, event.code));
                }
                _ => {}
            },
            DeviceEvent::Disconnected(device_id) => self.keys.retain(|(id, _)| id != device_id),
            _ => {}
        }
    }
    pub(super) fn matches(&self, hotkey: &[u16]) -> bool {
        hotkey
            .iter()
            .all(|required| self.keys.iter().any(|(_, code)| code == required))
            && self.keys.iter().all(|(_, code)| hotkey.contains(code))
    }
    pub(super) fn shift(&self) -> bool {
        self.keys.iter().any(|(_, code)| keys::is_shift(*code))
    }
    pub(super) fn command(&self) -> bool {
        self.keys
            .iter()
            .any(|(_, code)| keys::is_command_modifier(*code))
    }
}
