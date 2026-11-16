//! The overlay windows, each an `eco-window` process running the QML in
//! `overlay/`, for as long as the session holds them, and the tray icon, one
//! more such process while `[ui] tray` is on (docs/design.md §3).

use std::path::{Path, PathBuf};
use std::process::ExitStatus;

use crate::paths;

// The launcher of the window processes. Android has none: its app shows the
// windows and the daemon launches none (docs/design.md §15).
#[cfg(not(target_os = "android"))]
mod windows;
#[cfg(all(test, unix, not(target_os = "android")))]
pub(crate) use windows::programs;
#[cfg(not(target_os = "android"))]
pub use windows::{PROGRAM, Windows};

/// The overlay's entry `name` (`shell.qml`, the window; `tray.qml`, the tray
/// icon): installed beside the binary (`<prefix>/share/eco/overlay/<name>` for
/// `<prefix>/bin/eco`), or this checkout's when eco runs from it.
fn entry(name: &str) -> PathBuf {
    paths::shipped(&format!("overlay/{name}"))
        .unwrap_or_else(|| Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/overlay")).join(name))
}

/// The interface texts of the language pack `code` (`pt-BR`), by key; none
/// when the pack is not there or cannot be read.
pub fn language_pack(code: &str) -> serde_json::Map<String, serde_json::Value> {
    let path = entry(&format!("Eco/Core/i18n/{code}.json"));
    std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

/// How many starts in a row may fail before the tray is left alone.
pub const TRAY_STARTS: u32 = 5;

/// What ended the wait in `Windows::exited`.
#[cfg_attr(
    all(target_os = "android", not(test)),
    expect(
        dead_code,
        reason = "the app shows the windows, so none of the daemon's exits"
    )
)]
pub enum Exit {
    /// A window exited: its status, or `None` when it could not be read.
    Window(Option<ExitStatus>),
    /// The tray failed `TRAY_STARTS` times in a row and is not started again.
    TrayStopped,
}

#[cfg(test)]
mod packs {
    use super::*;

    #[test]
    fn a_language_pack_is_read_by_its_code_and_one_missing_is_empty() {
        let english = language_pack("en-US");
        assert_eq!(english["notify.answer"], "{action} · answer ready");
        assert_eq!(language_pack("pt-BR")["_locale"], "pt_BR");
        assert!(language_pack("xx-XX").is_empty());
    }
}
