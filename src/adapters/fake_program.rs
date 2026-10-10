//! Stand-in programs for the adapters that run one: a shell script in a test's
//! own directory, handed to the adapter as its program path.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The executable `dir/name` that runs the shell `body`.
///
/// `cp` writes it, not this process: a child another test thread forks
/// inherits every open fd until it execs, and a script open for writing
/// anywhere cannot run (ETXTBSY).
pub fn fake_program(dir: &Path, name: &str, body: &str) -> PathBuf {
    let path = dir.join(name);
    let source = dir.join(format!("{name}.source"));
    fs::write(&source, format!("#!/bin/sh\n{body}\n")).unwrap();
    let copied = Command::new("cp").arg(&source).arg(&path).status().unwrap();
    assert!(copied.success(), "cp {}", source.display());
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    path
}
