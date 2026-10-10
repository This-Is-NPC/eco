//! Global shortcuts as `[shortcuts]` in the config writes them: keys such as
//! "Win+Alt+N", each running an `eco window` command. The daemon registers
//! them on Windows (docs/design.md §15); on Linux Hyprland binds the keys.

use std::collections::BTreeMap;

/// The shortcuts when the config has no `[shortcuts]`: those of
/// `packaging/hypr/eco.lua`, with the Windows key for SUPER. None is a key
/// Windows documents for itself: Win+Alt+<digit> opens a taskbar jump list.
pub const DEFAULT: [(&str, &str); 7] = [
    ("Win+Alt+A", "action ask"),
    ("Win+Alt+Q", "action probe"),
    ("Win+Alt+C", "config"),
    ("Win+Alt+N", "new"),
    ("Win+Alt+P", "toggle"),
    ("Win+Alt+H", "sessions"),
    ("Win+Alt+E", "focus"),
];

/// A key with the modifiers held for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Keys {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub win: bool,
    pub key: Key,
}

/// The keys a shortcut can end in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    /// A letter A–Z or a digit 0–9, upper case.
    Char(char),
    /// F1 to F24.
    F(u8),
    Space,
    Enter,
    Tab,
    Escape,
    Left,
    Up,
    Right,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
    Insert,
    Delete,
}

impl std::str::FromStr for Keys {
    type Err = String;

    /// Modifiers (Ctrl, Alt, Shift, Win or Super), at least one, then the key,
    /// joined by `+`, in any case.
    fn from_str(text: &str) -> Result<Self, String> {
        let mut parts: Vec<String> = text.split('+').map(|p| p.trim().to_uppercase()).collect();
        let key = parts.pop().unwrap_or_default();
        let mut keys = Keys {
            ctrl: false,
            alt: false,
            shift: false,
            win: false,
            key: key_named(&key).ok_or_else(|| format!("unknown key {key:?}"))?,
        };
        if parts.is_empty() {
            return Err("needs Ctrl, Alt, Shift or Win with the key".into());
        }
        for part in parts {
            let held = match part.as_str() {
                "CTRL" | "CONTROL" => &mut keys.ctrl,
                "ALT" => &mut keys.alt,
                "SHIFT" => &mut keys.shift,
                "WIN" | "SUPER" => &mut keys.win,
                _ => return Err(format!("unknown modifier {part:?}")),
            };
            if *held {
                return Err(format!("{part} twice"));
            }
            *held = true;
        }
        Ok(keys)
    }
}

/// The key upper-case `name` stands for.
fn key_named(name: &str) -> Option<Key> {
    let mut chars = name.chars();
    if let (Some(only), None) = (chars.next(), chars.next())
        && only.is_ascii_alphanumeric()
    {
        return Some(Key::Char(only));
    }
    if let Some(number) = name.strip_prefix('F')
        && let Ok(number) = number.parse::<u8>()
    {
        return (1..=24).contains(&number).then_some(Key::F(number));
    }
    Some(match name {
        "SPACE" => Key::Space,
        "ENTER" => Key::Enter,
        "TAB" => Key::Tab,
        "ESC" | "ESCAPE" => Key::Escape,
        "LEFT" => Key::Left,
        "UP" => Key::Up,
        "RIGHT" => Key::Right,
        "DOWN" => Key::Down,
        "HOME" => Key::Home,
        "END" => Key::End,
        "PAGEUP" => Key::PageUp,
        "PAGEDOWN" => Key::PageDown,
        "INSERT" => Key::Insert,
        "DELETE" => Key::Delete,
        _ => return None,
    })
}

/// A shortcut: its keys as written, what they are, and the line the daemon
/// reads when they are pressed, the one `eco window` sends.
#[derive(Debug, Clone, PartialEq)]
pub struct Binding {
    pub written: String,
    pub keys: Keys,
    pub line: String,
}

/// The shortcuts of `table`, or the default ones when there is none; why one
/// is wrong otherwise.
pub fn bindings(table: Option<&BTreeMap<String, String>>) -> Result<Vec<Binding>, String> {
    let pairs: Vec<(&str, &str)> = match table {
        Some(table) => table
            .iter()
            .map(|(k, c)| (k.as_str(), c.as_str()))
            .collect(),
        None => DEFAULT.to_vec(),
    };
    let mut bindings: Vec<Binding> = Vec::new();
    for (written, command) in pairs {
        let keys: Keys = written
            .parse()
            .map_err(|why| format!("shortcut {written:?}: {why}"))?;
        let window = crate::window_request(command).ok_or_else(|| {
            format!("shortcut {written:?}: {command:?} is not an `eco window` command")
        })?;
        if bindings.iter().any(|b| b.keys == keys) {
            return Err(format!("shortcut {written:?}: these keys are bound twice"));
        }
        bindings.push(Binding {
            written: written.into(),
            keys,
            line: window.line(),
        });
    }
    Ok(bindings)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(text: &str) -> Result<Keys, String> {
        text.parse()
    }

    #[test]
    fn modifiers_and_a_key_in_any_case_and_spacing() {
        assert_eq!(
            keys("win+Alt + n"),
            Ok(Keys {
                ctrl: false,
                alt: true,
                shift: false,
                win: true,
                key: Key::Char('N'),
            })
        );
        assert_eq!(keys("Super+Alt+N"), keys("Win+Alt+N"));
        assert_eq!(keys("Control+Shift+F12").unwrap().key, Key::F(12));
        let all = keys("Ctrl+Shift+Win+Alt+PageDown").unwrap();
        assert!(all.ctrl && all.shift && all.win && all.alt);
        assert_eq!(all.key, Key::PageDown);
        assert_eq!(keys("Alt+esc").unwrap().key, Key::Escape);
        assert_eq!(keys("Alt+7").unwrap().key, Key::Char('7'));
    }

    #[test]
    fn a_key_without_a_modifier_or_unknown_is_refused() {
        assert_eq!(
            keys("N"),
            Err("needs Ctrl, Alt, Shift or Win with the key".into())
        );
        assert_eq!(keys("Win+Alt+"), Err("unknown key \"\"".into()));
        assert_eq!(keys("Win+F25"), Err("unknown key \"F25\"".into()));
        assert_eq!(keys("Win+Alt+Ç"), Err("unknown key \"Ç\"".into()));
        assert_eq!(keys("Hyper+N"), Err("unknown modifier \"HYPER\"".into()));
        assert_eq!(keys("Alt+alt+N"), Err("ALT twice".into()));
    }

    #[test]
    fn without_a_table_the_keys_of_eco_lua_send_what_eco_window_sends() {
        let lines: Vec<(String, String)> = bindings(None)
            .unwrap()
            .into_iter()
            .map(|b| (b.written, b.line))
            .collect();
        let expected = [
            ("Win+Alt+A", "action ask"),
            ("Win+Alt+Q", "action probe"),
            ("Win+Alt+C", r#"window.call {"call":"config"}"#),
            ("Win+Alt+N", r#"window.call {"call":"new_session"}"#),
            ("Win+Alt+P", "session.toggle"),
            ("Win+Alt+H", r#"window.call {"call":"sessions"}"#),
            ("Win+Alt+E", r#"window.call {"call":"focus"}"#),
        ]
        .map(|(keys, line)| (keys.to_string(), line.to_string()));
        assert_eq!(lines, expected);
    }

    #[test]
    fn no_default_is_a_key_windows_keeps_for_itself() {
        // Win+Alt with a digit (taskbar jump lists), B (HDR), D (date and
        // time), K (microphone), the arrows and Enter, and the Game Bar's G,
        // M, R and T, as Microsoft lists them.
        let reserved: Vec<Keys> = [
            "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "B", "D", "K", "Up", "Down", "Enter",
            "G", "M", "R", "T",
        ]
        .iter()
        .map(|key| format!("Win+Alt+{key}").parse().unwrap())
        .collect();
        for binding in bindings(None).unwrap() {
            assert!(!reserved.contains(&binding.keys), "{}", binding.written);
        }
    }

    #[test]
    fn a_table_replaces_the_defaults_and_an_empty_one_binds_nothing() {
        let table = BTreeMap::from([("Ctrl+Alt+M".to_string(), "action minutes".to_string())]);
        let only = bindings(Some(&table)).unwrap();
        assert_eq!(only.len(), 1);
        assert_eq!(only[0].line, "action minutes");
        assert_eq!(bindings(Some(&BTreeMap::new())), Ok(vec![]));
    }

    #[test]
    fn a_wrong_shortcut_says_which_and_why() {
        let one = |keys: &str, command: &str| {
            bindings(Some(&BTreeMap::from([(
                keys.to_string(),
                command.to_string(),
            )])))
            .unwrap_err()
        };
        assert_eq!(
            one("N", "focus"),
            "shortcut \"N\": needs Ctrl, Alt, Shift or Win with the key"
        );
        assert_eq!(
            one("Win+Alt+Q", "quit"),
            "shortcut \"Win+Alt+Q\": \"quit\" is not an `eco window` command"
        );
        assert_eq!(
            one("Win+Alt+Q", "action"),
            "shortcut \"Win+Alt+Q\": \"action\" is not an `eco window` command"
        );
        let twice = BTreeMap::from([
            ("Win+Alt+N".to_string(), "new".to_string()),
            ("super+alt+n".to_string(), "focus".to_string()),
        ]);
        assert_eq!(
            bindings(Some(&twice)).unwrap_err(),
            "shortcut \"super+alt+n\": these keys are bound twice"
        );
    }
}
