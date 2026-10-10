//! Windows: the global shortcuts of `[shortcuts]`, registered with the system
//! (`RegisterHotKey`, through windows-hotkeys) on a thread of their own that
//! waits for them, each pressed one handing its line to the daemon's loop.

use std::sync::mpsc;

use tokio::sync::mpsc::UnboundedSender;
use windows_hotkeys::keys::{ModKey, VKey};
use windows_hotkeys::{HotkeyManager, HotkeyManagerImpl};

use crate::shortcuts::{Binding, Key, Keys};

/// Register `bindings`, each sending its line to `lines` when pressed, and
/// return those the system refused, another program holding their keys.
pub fn register(bindings: Vec<Binding>, lines: UnboundedSender<String>) -> Vec<Binding> {
    let (refused, told) = mpsc::channel();
    std::thread::spawn(move || {
        // The hotkeys belong to this thread, which has to wait for them.
        let mut manager = HotkeyManager::<()>::new();
        let mut held = 0;
        let mut failed = Vec::new();
        for binding in bindings {
            let (key, modifiers) = system(&binding.keys);
            let lines = lines.clone();
            let line = binding.line.clone();
            let sent = move || {
                // The loop gone, the daemon is stopping.
                let _ = lines.send(line.clone());
            };
            match manager.register(key, &modifiers, sent) {
                Ok(_) => held += 1,
                Err(_) => failed.push(binding),
            }
        }
        let _ = refused.send(failed);
        if held > 0 {
            manager.event_loop();
        }
    });
    told.recv().unwrap_or_default()
}

/// The virtual key and modifiers of `keys`.
fn system(keys: &Keys) -> (VKey, Vec<ModKey>) {
    let held = [
        (keys.ctrl, ModKey::Ctrl),
        (keys.alt, ModKey::Alt),
        (keys.shift, ModKey::Shift),
        (keys.win, ModKey::Win),
    ];
    let modifiers = held
        .into_iter()
        .filter_map(|(down, modifier)| down.then_some(modifier))
        .collect();
    (VKey::CustomKeyCode(code(keys.key)), modifiers)
}

/// The Windows virtual-key code of `key`.
fn code(key: Key) -> i32 {
    match key {
        // Letters and digits are their upper-case ASCII codes.
        Key::Char(char) => char as i32,
        Key::F(number) => 0x6F + i32::from(number),
        Key::Space => 0x20,
        Key::Enter => 0x0D,
        Key::Tab => 0x09,
        Key::Escape => 0x1B,
        Key::Left => 0x25,
        Key::Up => 0x26,
        Key::Right => 0x27,
        Key::Down => 0x28,
        Key::Home => 0x24,
        Key::End => 0x23,
        Key::PageUp => 0x21,
        Key::PageDown => 0x22,
        Key::Insert => 0x2D,
        Key::Delete => 0x2E,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::shortcuts::bindings;

    #[test]
    fn keys_become_virtual_keys_and_modifiers() {
        let keys: Keys = "Win+Alt+N".parse().unwrap();
        let (key, modifiers) = system(&keys);
        assert_eq!(key.to_vk_code(), 0x4E);
        assert_eq!(modifiers, [ModKey::Alt, ModKey::Win]);
        let codes = [
            "Ctrl+7",
            "Ctrl+F1",
            "Ctrl+F24",
            "Ctrl+PageDown",
            "Ctrl+Delete",
        ]
        .map(|text| code(text.parse::<Keys>().unwrap().key));
        assert_eq!(codes, [0x37, 0x70, 0x87, 0x22, 0x2E]);
    }

    #[test]
    fn keys_the_system_refuses_are_given_back() {
        // Keys nobody uses: on a desktop the first registration holds them and
        // the second is refused; without one (over ssh) both are.
        let table = BTreeMap::from([("Ctrl+Alt+Shift+F23".to_string(), "focus".to_string())]);
        let binding = bindings(Some(&table)).unwrap();
        let (lines, _) = tokio::sync::mpsc::unbounded_channel();
        let _held = register(binding.clone(), lines.clone());
        assert_eq!(register(binding.clone(), lines), binding);
    }
}
