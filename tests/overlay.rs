//! The overlay's JavaScript modules pass the QML test cases in
//! `tests/overlay/`, run offscreen by Qt's own test runner.

use std::path::{Path, PathBuf};
use std::process::Command;

const ROOT: &str = env!("CARGO_MANIFEST_DIR");

/// Qt 6's test runner as qt6-declarative installs it on Arch.
#[cfg(unix)]
fn runner() -> PathBuf {
    "/usr/lib/qt6/bin/qmltestrunner".into()
}

/// Qt 6's test runner in the Qt `mise run qt:install` put under `.qt`.
#[cfg(windows)]
fn runner() -> PathBuf {
    let found: Vec<PathBuf> = std::fs::read_dir(Path::new(ROOT).join(".qt"))
        .expect("no Qt under .qt; run `mise run qt:install`")
        .map(|version| {
            version
                .unwrap()
                .path()
                .join("msvc2022_64/bin/qmltestrunner.exe")
        })
        .filter(|runner| runner.is_file())
        .collect();
    let [runner] = <[PathBuf; 1]>::try_from(found).expect("one Qt under .qt");
    runner
}

#[test]
fn overlay_modules_pass_their_qml_tests() {
    let runner = runner();
    let runtime = tempfile::tempdir().unwrap();
    let output = Command::new(&runner)
        .arg("-input")
        .arg(Path::new(ROOT).join("tests/overlay"))
        .env("QT_QPA_PLATFORM", "offscreen")
        .env("XDG_RUNTIME_DIR", runtime.path())
        .output()
        .unwrap_or_else(|error| panic!("{}: {error}", runner.display()));
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
