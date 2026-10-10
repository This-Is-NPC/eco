//! Stand-in programs for the adapters that run one: a shell script in a test's
//! own directory, handed to the adapter as its program path.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

/// The executable `dir/name` that runs the shell `body`.
pub fn fake_program(dir: &Path, name: &str, body: &str) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    path
}
