//! The language packs stay complete: same keys and placeholders everywhere, and
//! every key the interface or the daemon asks for exists.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

const ROOT: &str = env!("CARGO_MANIFEST_DIR");
/// Keys built at runtime from a fixed set of names.
const DYNAMIC: [&str; 2] = ["section.start", "section.history"];

fn files(directory: &Path, extension: &str) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for entry in fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            found.extend(files(&path, extension));
        } else if path.extension().is_some_and(|e| e == extension) {
            found.push(path);
        }
    }
    found
}

fn packs() -> BTreeMap<String, Map<String, Value>> {
    files(&Path::new(ROOT).join("overlay/Eco/Core/i18n"), "json")
        .into_iter()
        .map(|path| {
            let code = path.file_stem().unwrap().to_string_lossy().into_owned();
            (
                code,
                serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap(),
            )
        })
        .collect()
}

fn is_key(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '.'
}

/// The `{name}` placeholders in a text.
fn placeholders(text: &str) -> BTreeSet<&str> {
    text.split('{')
        .skip(1)
        .filter_map(|rest| rest.split_once('}').map(|(name, _)| name))
        .filter(|name| !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_'))
        .collect()
}

/// Every `<prefix>"<key>"` in a text, the key made of word characters and dots.
fn quoted_after<'a>(text: &'a str, prefix: &str) -> Vec<&'a str> {
    let mut found = Vec::new();
    for (start, _) in text.match_indices(prefix) {
        let rest = text[start + prefix.len()..].trim_start();
        if let Some(rest) = rest.strip_prefix('"') {
            let end = rest.find(|c: char| !is_key(c)).unwrap_or(rest.len());
            if rest[end..].starts_with('"') && end > 0 {
                found.push(&rest[..end]);
            }
        }
    }
    found
}

fn resolves(base: &Map<String, Value>, key: &str) -> bool {
    base.contains_key(key) || base.contains_key(&format!("{key}.other"))
}

#[test]
fn the_three_languages_ship() {
    let packs = packs();
    for code in ["en-US", "pt-BR", "ja-JP"] {
        assert!(packs.contains_key(code), "{code} is missing");
    }
}

#[test]
fn every_pack_has_the_base_keys_and_placeholders() {
    let packs = packs();
    let base = &packs["en-US"];
    for (code, pack) in &packs {
        let missing: Vec<_> = base.keys().filter(|k| !pack.contains_key(*k)).collect();
        let extra: Vec<_> = pack.keys().filter(|k| !base.contains_key(*k)).collect();
        assert!(
            missing.is_empty() && extra.is_empty(),
            "{code}: missing {missing:?}, extra {extra:?}"
        );
        for (key, text) in base {
            let (text, translated) = (text.as_str().unwrap(), pack[key].as_str().unwrap());
            assert_eq!(
                placeholders(translated),
                placeholders(text),
                "{code}: {key}"
            );
        }
        for key in ["_name", "_locale"] {
            assert!(!pack[key].as_str().unwrap().is_empty(), "{code}: {key}");
        }
    }
}

#[test]
fn every_key_the_interface_uses_exists() {
    let base = &packs()["en-US"];
    let mut used: BTreeSet<String> = DYNAMIC.map(String::from).into();
    for path in files(&Path::new(ROOT).join("overlay"), "qml") {
        let text = fs::read_to_string(&path).unwrap();
        // Literal keys only; prefixes completed at runtime ("error." + code) are covered apart.
        used.extend(
            quoted_after(&text, "I18n.t(")
                .into_iter()
                .filter(|key| !key.ends_with('.'))
                .map(String::from),
        );
    }
    let missing: Vec<_> = used.iter().filter(|key| !resolves(base, key)).collect();
    assert!(missing.is_empty(), "missing {missing:?}");
}

#[test]
fn every_daemon_error_code_has_a_translation() {
    let base = &packs()["en-US"];
    let mut codes = BTreeSet::new();
    for path in files(&Path::new(ROOT).join("src"), "rs") {
        codes.extend(
            quoted_after(&fs::read_to_string(&path).unwrap(), "error(")
                .into_iter()
                .map(String::from),
        );
    }
    assert!(codes.len() > 5, "no coded errors found");
    let missing: Vec<_> = codes
        .iter()
        .filter(|code| !resolves(base, &format!("error.{code}")))
        .collect();
    assert!(missing.is_empty(), "missing {missing:?}");
}
