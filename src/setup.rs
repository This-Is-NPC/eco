//! `eco setup`: download the pinned models and load the Hyprland rules.

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};

use crate::adapters::http;
use crate::config;

/// A model file eco downloads, pinned by checksum.
struct Model {
    name: &'static str,
    url: &'static str,
    sha256: &'static str,
    target: fn() -> PathBuf,
}

const MODELS: [Model; 2] = [
    // The export with declared input shapes and no nested sample-rate branches:
    // tract cannot type the branches of the default `silero_vad.onnx`.
    Model {
        name: "Silero VAD",
        url: "https://github.com/snakers4/silero-vad/raw/v6.2.3/src/silero_vad/data/silero_vad_op18_ifless.onnx",
        sha256: "7671cd04b004e9076da0d4a7b1a5aec36adf161c39230c1cb94a4fd5db6bbd28",
        target: config::vad_model,
    },
    // WeSpeaker CAM++ trained on VoxCeleb (Apache-2.0), for telling speakers apart.
    Model {
        name: "WeSpeaker CAM++",
        url: "https://huggingface.co/Wespeaker/wespeaker-voxceleb-campplus/resolve/main/voxceleb_CAM%2B%2B.onnx",
        sha256: "b50810498b5bcf5773d086f6993d344476bd0c88b566a41e8d801aaf8461efad",
        target: config::speaker_model,
    },
];

fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Download each model unless the one in place already has the pinned checksum.
pub async fn run() -> Result<Vec<String>> {
    let mut done = Vec::new();
    for model in &MODELS {
        let target = (model.target)();
        if fs::read(&target).is_ok_and(|bytes| digest(&bytes) == model.sha256) {
            done.push(format!("{} is up to date", target.display()));
            continue;
        }
        let bytes = http::client(None)?
            .get(model.url)
            .send()
            .await?
            .error_for_status()?
            .bytes()
            .await?;
        let found = digest(&bytes);
        if found != model.sha256 {
            bail!("{} checksum mismatch: {found}", model.name);
        }
        fs::create_dir_all(target.parent().context("model path has a parent")?)?;
        fs::write(&target, &bytes)?;
        done.push(format!("saved {}", target.display()));
    }
    Ok(done)
}

/// Ends the line `hyprland` writes, so a later run finds its own line.
const MARKER: &str = " -- eco setup";

/// The Lua line loading `rules`, skipped when the file is gone (after
/// `pacman -R eco`) so Hyprland does not fail on it.
fn load_line(rules: &Path) -> String {
    let path = rules
        .display()
        .to_string()
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    format!(
        "do local eco = \"{path}\"; local file = io.open(eco, \"r\"); if file then file:close(); dofile(eco) end end{MARKER}"
    )
}

fn is_ours(line: &str) -> bool {
    let line = line.trim();
    line.ends_with(MARKER) && !line.starts_with("--")
}

/// `text` with the eco line set to `line`, every other byte kept: its own line
/// rewritten in place, or `line` appended in the file's line ending. `None`
/// when the file already holds `line`.
fn with_line(text: &str, line: &str) -> Option<(String, &'static str)> {
    let mut out = String::with_capacity(text.len() + line.len() + 2);
    let mut found = false;
    for piece in text.split_inclusive('\n') {
        let body = piece.trim_end_matches(['\n', '\r']);
        if body.trim() == line {
            return None;
        }
        if !found && is_ours(body) {
            found = true;
            out.push_str(line);
            out.push_str(&piece[body.len()..]);
        } else {
            out.push_str(piece);
        }
    }
    if found {
        return Some((out, "updated"));
    }
    let ending = if text.contains("\r\n") { "\r\n" } else { "\n" };
    if text.is_empty() || text.ends_with('\n') {
        out.push_str(line);
        out.push_str(ending);
    } else {
        // Without a final newline before, there is none after.
        out.push_str(ending);
        out.push_str(line);
    }
    Some((out, "added"))
}

/// Make `bindings` load `rules` with one marked line: add it at the end,
/// rewrite it in place when `rules` moved, and leave the file alone when a
/// line eco did not write already names an `eco.lua`. Never creates `bindings`.
/// A change first copies the file to `bindings.lua.bak.<unix seconds>` beside
/// it, then replaces the file a symlink points to atomically, in its own mode.
/// `Err` is a warning: the file could not be read or written.
pub fn hyprland(bindings: &Path, rules: Option<&Path>) -> Result<String, String> {
    use std::os::unix::fs::PermissionsExt;
    let Some(rules) = rules else {
        return Ok(
            "no Hyprland rules installed beside this eco; Hyprland config left as is".into(),
        );
    };
    let line = load_line(rules);
    let shown = bindings.display();
    let warn = |doing: &str, error: std::io::Error| {
        format!("could not {doing} {shown}, so eco's Hyprland rules are not loaded: {error}")
    };
    let text = match fs::read_to_string(bindings) {
        Ok(text) => text,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            return Ok(format!(
                "no {shown}; add this line to your Hyprland config to load eco's rules: {line}"
            ));
        }
        Err(error) => return Err(warn("read", error)),
    };
    if text
        .lines()
        .any(|other| !is_ours(other) && other.contains("eco.lua"))
    {
        return Ok(format!("{shown} already loads an eco.lua; left as is"));
    }
    let Some((changed, outcome)) = with_line(&text, &line) else {
        return Ok(format!("Hyprland rules in {shown} are up to date"));
    };
    let target = fs::canonicalize(bindings).map_err(|error| warn("resolve", error))?;
    let mode = fs::metadata(&target)
        .map_err(|error| warn("read", error))?
        .permissions()
        .mode()
        & 0o7777;
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    let mut name = bindings.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".bak.{seconds}"));
    let backup = bindings.with_file_name(name);
    config::replace_file(&backup, text.as_bytes(), mode).map_err(|error| warn("back up", error))?;
    config::replace_file(&target, changed.as_bytes(), mode)
        .map_err(|error| warn("write", error))?;
    Ok(format!(
        "Hyprland rules {outcome} in {shown}; `hyprctl reload` loads them"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const RULES: &str = "/usr/share/eco/hypr/eco.lua";

    fn bindings(text: Option<&str>) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hypr/bindings.lua");
        if let Some(text) = text {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, text).unwrap();
        }
        (dir, path)
    }

    #[test]
    fn the_line_loads_the_rules_only_while_they_exist() {
        assert_eq!(
            load_line(Path::new(RULES)),
            "do local eco = \"/usr/share/eco/hypr/eco.lua\"; local file = io.open(eco, \"r\"); if file then file:close(); dofile(eco) end end -- eco setup"
        );
        assert_eq!(
            load_line(Path::new("/a \"b\\c/eco.lua")),
            "do local eco = \"/a \\\"b\\\\c/eco.lua\"; local file = io.open(eco, \"r\"); if file then file:close(); dofile(eco) end end -- eco setup"
        );
    }

    #[test]
    fn adds_one_line_and_a_rerun_changes_nothing() {
        let (_dir, path) = bindings(Some("o.bind(\"SUPER + Q\", \"close\")\n"));
        let added = hyprland(&path, Some(Path::new(RULES))).unwrap();
        assert!(added.starts_with("Hyprland rules added in"), "{added}");
        let once = fs::read_to_string(&path).unwrap();
        assert_eq!(
            once,
            format!(
                "o.bind(\"SUPER + Q\", \"close\")\n{}\n",
                load_line(Path::new(RULES))
            )
        );
        let again = hyprland(&path, Some(Path::new(RULES))).unwrap();
        assert!(again.ends_with("are up to date"), "{again}");
        assert_eq!(fs::read_to_string(&path).unwrap(), once);
    }

    #[test]
    fn rewrites_its_own_line_when_the_rules_moved() {
        let old = load_line(Path::new("/home/you/.local/share/eco/hypr/eco.lua"));
        let (_dir, path) = bindings(Some(&format!("-- mine\n{old}\n-- after\n")));
        let updated = hyprland(&path, Some(Path::new(RULES))).unwrap();
        assert!(
            updated.starts_with("Hyprland rules updated in"),
            "{updated}"
        );
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            format!("-- mine\n{}\n-- after\n", load_line(Path::new(RULES)))
        );
    }

    #[test]
    fn leaves_a_line_it_did_not_write() {
        for theirs in [
            "dofile(\"/usr/share/eco/hypr/eco.lua\")\n".to_string(),
            format!("-- {}\n", load_line(Path::new(RULES))),
        ] {
            let (_dir, path) = bindings(Some(&theirs));
            let said = hyprland(&path, Some(Path::new(RULES))).unwrap();
            assert!(
                said.ends_with("already loads an eco.lua; left as is"),
                "{said}"
            );
            assert_eq!(fs::read_to_string(&path).unwrap(), theirs);
        }
    }

    #[test]
    fn creates_no_hyprland_config() {
        let (dir, path) = bindings(None);
        let said = hyprland(&path, Some(Path::new(RULES))).unwrap();
        assert!(said.starts_with("no "), "{said}");
        assert!(said.ends_with(&load_line(Path::new(RULES))), "{said}");
        assert!(!dir.path().join("hypr").exists());
    }

    fn backups(path: &Path) -> Vec<PathBuf> {
        fs::read_dir(path.parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|other| {
                other
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("bindings.lua.bak.")
            })
            .collect()
    }

    #[test]
    fn keeps_every_other_byte_of_the_file() {
        let line = load_line(Path::new(RULES));
        let old = load_line(Path::new("/old/eco.lua"));
        for (before, after) in [
            ("a\r\nb\r\n".to_string(), format!("a\r\nb\r\n{line}\r\n")),
            ("a\nb".to_string(), format!("a\nb\n{line}")),
            (format!("a\r\n{old}\r\nb"), format!("a\r\n{line}\r\nb")),
            (format!("a\n{old}"), format!("a\n{line}")),
        ] {
            let (_dir, path) = bindings(Some(&before));
            hyprland(&path, Some(Path::new(RULES))).unwrap();
            assert_eq!(fs::read_to_string(&path).unwrap(), after, "{before:?}");
        }
    }

    #[test]
    fn backs_up_only_a_file_it_changes() {
        let (_dir, path) = bindings(Some("-- mine\n"));
        hyprland(&path, Some(Path::new(RULES))).unwrap();
        let saved = backups(&path);
        assert_eq!(saved.len(), 1);
        assert_eq!(fs::read_to_string(&saved[0]).unwrap(), "-- mine\n");
        hyprland(&path, Some(Path::new(RULES))).unwrap();
        assert_eq!(backups(&path), saved);
        let (_dir, theirs) = bindings(Some("dofile(\"/x/eco.lua\")\n"));
        hyprland(&theirs, Some(Path::new(RULES))).unwrap();
        assert!(backups(&theirs).is_empty());
    }

    #[test]
    fn replaces_what_a_symlink_points_to_in_its_mode() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let (dir, path) = bindings(None);
        let real = dir.path().join("dotfiles/bindings.lua");
        fs::create_dir_all(real.parent().unwrap()).unwrap();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&real, "-- mine\n").unwrap();
        fs::set_permissions(&real, fs::Permissions::from_mode(0o640)).unwrap();
        symlink(&real, &path).unwrap();
        hyprland(&path, Some(Path::new(RULES))).unwrap();
        assert!(
            fs::symlink_metadata(&path)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(fs::read_link(&path).unwrap(), real);
        assert!(
            fs::read_to_string(&real)
                .unwrap()
                .ends_with("-- eco setup\n")
        );
        let mode = |file: &Path| fs::metadata(file).unwrap().permissions().mode() & 0o7777;
        assert_eq!(mode(&real), 0o640);
        assert_eq!(mode(&backups(&path)[0]), 0o640);
    }

    #[test]
    fn a_file_it_cannot_write_is_a_warning() {
        use std::os::unix::fs::PermissionsExt;
        let (_dir, path) = bindings(Some("-- mine\n"));
        let parent = path.parent().unwrap();
        fs::set_permissions(parent, fs::Permissions::from_mode(0o500)).unwrap();
        // Root writes through any mode; there is nothing to check then.
        let writable = fs::write(parent.join("probe"), "").is_ok();
        let said = hyprland(&path, Some(Path::new(RULES)));
        fs::set_permissions(parent, fs::Permissions::from_mode(0o700)).unwrap();
        if writable {
            return;
        }
        let warning = said.unwrap_err();
        assert!(
            warning.contains("eco's Hyprland rules are not loaded"),
            "{warning}"
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), "-- mine\n");
        assert!(backups(&path).is_empty());
    }

    #[test]
    fn digests_are_lowercase_hex() {
        assert_eq!(
            digest(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }
}
