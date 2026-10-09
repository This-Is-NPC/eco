//! The overlay's JavaScript modules pass the QML test cases in
//! `tests/overlay/`, run offscreen by Qt's own test runner.

use std::path::Path;
use std::process::Command;

const ROOT: &str = env!("CARGO_MANIFEST_DIR");
/// Qt 6's test runner as qt6-declarative installs it on Arch.
const RUNNER: &str = "/usr/lib/qt6/bin/qmltestrunner";

#[test]
fn overlay_modules_pass_their_qml_tests() {
    let runtime = tempfile::tempdir().unwrap();
    let output = Command::new(RUNNER)
        .arg("-input")
        .arg(Path::new(ROOT).join("tests/overlay"))
        .env("QT_QPA_PLATFORM", "offscreen")
        .env("XDG_RUNTIME_DIR", runtime.path())
        .output()
        .unwrap_or_else(|error| panic!("{RUNNER}: {error}"));
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
