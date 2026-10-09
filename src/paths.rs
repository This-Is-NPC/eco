//! Every filesystem path eco builds, in one place (docs/design.md §15).
//! On Linux they sit under the home, the XDG base directories and
//! `/run/user/<uid>`.

use std::ffi::OsString;
use std::path::PathBuf;
use std::{env, fs};

/// The paths, built from the environment variables `var` reads.
struct Layout<V> {
    var: V,
}

impl<V: Fn(&str) -> Option<OsString>> Layout<V> {
    fn home(&self) -> PathBuf {
        (self.var)("HOME").map(PathBuf::from).unwrap_or_default()
    }

    /// The directory `variable` names, or `fallback` under the home when it is unset or empty.
    fn base(&self, variable: &str, fallback: &str) -> PathBuf {
        (self.var)(variable)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| self.home().join(fallback))
    }

    fn config_home(&self) -> PathBuf {
        self.base("XDG_CONFIG_HOME", ".config")
    }

    fn data_dir(&self) -> PathBuf {
        self.base("XDG_DATA_HOME", ".local/share").join("eco")
    }

    fn runtime_dir(&self) -> PathBuf {
        (self.var)("XDG_RUNTIME_DIR")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(format!("/run/user/{}", users_uid())))
    }

    fn config_file(&self) -> PathBuf {
        self.config_home().join("eco").join("config.toml")
    }

    fn hypr_bindings(&self) -> PathBuf {
        self.config_home().join("hypr/bindings.lua")
    }

    fn vad_model(&self) -> PathBuf {
        self.data_dir().join("models").join("silero_vad.onnx")
    }

    fn speaker_model(&self) -> PathBuf {
        self.data_dir()
            .join("models")
            .join("wespeaker_campplus.onnx")
    }

    fn people_dir(&self) -> PathBuf {
        self.data_dir().join("people")
    }

    fn sessions_dir(&self) -> PathBuf {
        self.data_dir().join("sessions")
    }

    fn socket_path(&self) -> PathBuf {
        self.runtime_dir().join("eco.sock")
    }

    fn token_path(&self) -> PathBuf {
        self.runtime_dir().join("eco.token")
    }

    fn omapass_plugin(&self) -> PathBuf {
        self.home()
            .join(".config/omarchy/plugins/io.github.this-is-npc.omapass/bin/omapass")
    }

    fn expand(&self, path: &str) -> PathBuf {
        match path.strip_prefix("~/") {
            Some(rest) => self.home().join(rest),
            None => PathBuf::from(path),
        }
    }
}

fn layout() -> Layout<impl Fn(&str) -> Option<OsString>> {
    Layout {
        var: |name: &str| env::var_os(name),
    }
}

fn users_uid() -> u32 {
    fs::metadata("/proc/self")
        .map(|m| std::os::unix::fs::MetadataExt::uid(&m))
        .unwrap_or(0)
}

pub fn home() -> PathBuf {
    layout().home()
}

/// `<prefix>/<directory>/<path>` for the running `<prefix>/bin/eco`, when it is there.
fn beside_binary(directory: &str, path: &str) -> Option<PathBuf> {
    let exe = env::current_exe().ok()?;
    Some(exe.parent()?.parent()?.join(directory).join(path)).filter(|file| file.exists())
}

/// A file eco ships beside its binary, `<prefix>/share/eco/<path>` for
/// `<prefix>/bin/eco`, when it is there.
pub fn shipped(path: &str) -> Option<PathBuf> {
    beside_binary("share/eco", path)
}

/// A program eco ships for itself alone, `<prefix>/lib/eco/<name>` for
/// `<prefix>/bin/eco`, when it is there.
pub fn shipped_program(name: &str) -> Option<PathBuf> {
    beside_binary("lib/eco", name)
}

pub fn config_file() -> PathBuf {
    layout().config_file()
}

/// The Hyprland file that holds the user's key bindings.
pub fn hypr_bindings() -> PathBuf {
    layout().hypr_bindings()
}

pub fn vad_model() -> PathBuf {
    layout().vad_model()
}

pub fn speaker_model() -> PathBuf {
    layout().speaker_model()
}

pub fn people_dir() -> PathBuf {
    layout().people_dir()
}

pub fn sessions_dir() -> PathBuf {
    layout().sessions_dir()
}

pub fn socket_path() -> PathBuf {
    layout().socket_path()
}

/// Where a running daemon keeps the windows' token, for a window it did not start.
pub fn token_path() -> PathBuf {
    layout().token_path()
}

/// Where Omarchy installs the omapass plugin's CLI.
pub fn omapass_plugin() -> PathBuf {
    layout().omapass_plugin()
}

/// `path` with a leading `~/` replaced by the home.
pub fn expand(path: &str) -> PathBuf {
    layout().expand(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with(
        variables: &[(&'static str, &'static str)],
    ) -> Layout<impl Fn(&str) -> Option<OsString>> {
        let variables = variables.to_vec();
        Layout {
            var: move |name: &str| {
                variables
                    .iter()
                    .find(|(key, _)| *key == name)
                    .map(|(_, value)| OsString::from(value))
            },
        }
    }

    fn all(layout: &Layout<impl Fn(&str) -> Option<OsString>>) -> Vec<PathBuf> {
        vec![
            layout.config_file(),
            layout.hypr_bindings(),
            layout.vad_model(),
            layout.speaker_model(),
            layout.people_dir(),
            layout.sessions_dir(),
            layout.socket_path(),
            layout.token_path(),
            layout.omapass_plugin(),
            layout.expand("~/cv.md"),
            layout.expand("/etc/cv.md"),
        ]
    }

    #[test]
    fn paths_follow_the_xdg_variables_when_set() {
        let layout = with(&[
            ("HOME", "/home/you"),
            ("XDG_CONFIG_HOME", "/cfg"),
            ("XDG_DATA_HOME", "/data"),
            ("XDG_RUNTIME_DIR", "/run/user/1000"),
        ]);
        let expected = [
            "/cfg/eco/config.toml",
            "/cfg/hypr/bindings.lua",
            "/data/eco/models/silero_vad.onnx",
            "/data/eco/models/wespeaker_campplus.onnx",
            "/data/eco/people",
            "/data/eco/sessions",
            "/run/user/1000/eco.sock",
            "/run/user/1000/eco.token",
            "/home/you/.config/omarchy/plugins/io.github.this-is-npc.omapass/bin/omapass",
            "/home/you/cv.md",
            "/etc/cv.md",
        ];
        assert_eq!(all(&layout), expected.map(PathBuf::from));
    }

    #[test]
    fn unset_or_empty_xdg_variables_fall_back_under_the_home() {
        let runtime = format!("/run/user/{}", users_uid());
        let expected = [
            "/home/you/.config/eco/config.toml".to_string(),
            "/home/you/.config/hypr/bindings.lua".into(),
            "/home/you/.local/share/eco/models/silero_vad.onnx".into(),
            "/home/you/.local/share/eco/models/wespeaker_campplus.onnx".into(),
            "/home/you/.local/share/eco/people".into(),
            "/home/you/.local/share/eco/sessions".into(),
            format!("{runtime}/eco.sock"),
            format!("{runtime}/eco.token"),
            "/home/you/.config/omarchy/plugins/io.github.this-is-npc.omapass/bin/omapass".into(),
            "/home/you/cv.md".into(),
            "/etc/cv.md".into(),
        ]
        .map(PathBuf::from);
        let unset = with(&[("HOME", "/home/you")]);
        let empty = with(&[
            ("HOME", "/home/you"),
            ("XDG_CONFIG_HOME", ""),
            ("XDG_DATA_HOME", ""),
            ("XDG_RUNTIME_DIR", ""),
        ]);
        assert_eq!(all(&unset), expected);
        assert_eq!(all(&empty), expected);
    }

    #[test]
    fn without_a_home_paths_are_relative() {
        let layout = with(&[]);
        assert_eq!(
            layout.config_file(),
            PathBuf::from(".config/eco/config.toml")
        );
        assert_eq!(layout.expand("~/cv.md"), PathBuf::from("cv.md"));
    }
}
