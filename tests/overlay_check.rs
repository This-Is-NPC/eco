//! `scripts/overlay-check` enforces the overlay's module rules (docs/design.md
//! §3). These tests run it on a temp copy of `overlay/` so a bad import or a
//! Kit component missing from the control lab can be injected without
//! touching the checkout, and prove it passes on the real tree untouched.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const ROOT: &str = env!("CARGO_MANIFEST_DIR");

fn script() -> PathBuf {
    Path::new(ROOT).join("scripts/overlay-check")
}

/// A temp copy of the checkout's `overlay/`, to mutate without touching it.
fn copied_overlay() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("overlay");
    let status = Command::new("cp")
        .args([
            "-r",
            &Path::new(ROOT).join("overlay").to_string_lossy(),
            &dest.to_string_lossy(),
        ])
        .status()
        .unwrap();
    assert!(status.success(), "cp -r overlay to a temp dir");
    (dir, dest)
}

fn run(overlay: &Path) -> Output {
    Command::new(script())
        .arg(overlay)
        .output()
        .unwrap_or_else(|error| panic!("{}: {error}", script().display()))
}

#[test]
fn the_real_overlay_passes() {
    let (_dir, overlay) = copied_overlay();
    let output = run(&overlay);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn a_disallowed_import_fails_the_check() {
    let (_dir, overlay) = copied_overlay();
    // Kit may only import Core; Eco.Live is not allowed there.
    let chip = overlay.join("Eco/Kit/Chip.qml");
    let text = fs::read_to_string(&chip).unwrap();
    fs::write(&chip, format!("import Eco.Live\n{text}")).unwrap();

    let output = run(&overlay);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Chip.qml"), "{stderr}");
    assert!(stderr.contains("Eco.Live"), "{stderr}");
    assert!(stderr.contains("Kit may not import"), "{stderr}");
}

#[test]
fn a_relative_import_fails_the_check() {
    let (_dir, overlay) = copied_overlay();
    let chip = overlay.join("Eco/Kit/Chip.qml");
    let text = fs::read_to_string(&chip).unwrap();
    fs::write(&chip, format!("import \"../Core\"\n{text}")).unwrap();

    let output = run(&overlay);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Chip.qml"), "{stderr}");
    assert!(stderr.contains("relative import"), "{stderr}");
}

#[test]
fn a_file_missing_from_qmldir_fails_the_check() {
    let (_dir, overlay) = copied_overlay();
    let qmldir = overlay.join("Eco/Kit/qmldir");
    let text = fs::read_to_string(&qmldir).unwrap();
    let edited = text.replacen("Chip 1.0 Chip.qml\n", "", 1);
    assert_ne!(text, edited, "Chip.qml should have been declared in qmldir");
    fs::write(&qmldir, edited).unwrap();

    let output = run(&overlay);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Chip.qml"), "{stderr}");
    assert!(stderr.contains("not declared in qmldir"), "{stderr}");
}

#[test]
fn a_kit_component_missing_from_the_lab_fails_the_check() {
    let (_dir, overlay) = copied_overlay();
    // NewPill is shown only in LabFeedbackSamples.qml; removing that use
    // leaves it with no path from the control lab.
    let samples = overlay.join("Eco/Lab/LabFeedbackSamples.qml");
    let text = fs::read_to_string(&samples).unwrap();
    assert!(
        text.contains("NewPill {"),
        "fixture no longer shows NewPill"
    );
    fs::write(&samples, text.replace("NewPill {", "Item {")).unwrap();

    let output = run(&overlay);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("NewPill"), "{stderr}");
    assert!(stderr.contains("not shown in the control lab"), "{stderr}");
}
