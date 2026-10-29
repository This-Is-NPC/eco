//! Every filesystem path eco builds, in one place (docs/design.md §15).
//! On Linux they sit under the home, the XDG base directories and
//! `/run/user/<uid>`; on Windows under `%APPDATA%` and `%LOCALAPPDATA%`,
//! with the daemon's named pipe at `\\.\pipe\eco-<user>`.

use std::env;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// The paths, built from the environment variables `var` reads.
struct Layout<V> {
    var: V,
}

/// Paths built the same way on every platform, below the home and the data directory.
impl<V: Fn(&str) -> Option<OsString>> Layout<V> {
    /// The directory `variable` names, or `fallback` under the home when it is unset or empty.
    fn base(&self, variable: &str, fallback: &str) -> PathBuf {
        (self.var)(variable)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| self.home().join(fallback))
    }

    /// Where `eco setup` downloads the model `file` to.
    fn model(&self, file: &str) -> PathBuf {
        self.data_dir().join("models").join(file)
    }

    fn people_dir(&self) -> PathBuf {
        self.data_dir().join("people")
    }

    fn sessions_dir(&self) -> PathBuf {
        self.data_dir().join("sessions")
    }

    fn expand(&self, path: &str) -> PathBuf {
        match path.strip_prefix("~/") {
            Some(rest) => self.home().join(rest),
            None => PathBuf::from(path),
        }
    }
}

/// Linux: the home, the XDG base directories and `/run/user/<uid>`.
#[cfg(unix)]
impl<V: Fn(&str) -> Option<OsString>> Layout<V> {
    fn home(&self) -> PathBuf {
        (self.var)("HOME").map(PathBuf::from).unwrap_or_default()
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

    fn socket_path(&self) -> PathBuf {
        self.runtime_dir().join("eco.sock")
    }

    fn update_state(&self) -> PathBuf {
        self.base("XDG_STATE_HOME", ".local/state")
            .join("eco")
            .join("update.json")
    }

    fn token_path(&self) -> PathBuf {
        self.runtime_dir().join("eco.token")
    }

    fn hypr_events(&self) -> Option<PathBuf> {
        let instance =
            (self.var)("HYPRLAND_INSTANCE_SIGNATURE").filter(|value| !value.is_empty())?;
        Some(
            self.runtime_dir()
                .join("hypr")
                .join(instance)
                .join(".socket2.sock"),
        )
    }

    fn omapass_plugin(&self) -> PathBuf {
        self.home()
            .join(".config/omarchy/plugins/io.github.this-is-npc.omapass/bin/omapass")
    }
}

/// Windows: the user profile, the roaming and local application data, and a
/// named pipe per user.
#[cfg(windows)]
impl<V: Fn(&str) -> Option<OsString>> Layout<V> {
    fn home(&self) -> PathBuf {
        (self.var)("USERPROFILE")
            .map(PathBuf::from)
            .unwrap_or_default()
    }

    fn data_dir(&self) -> PathBuf {
        self.base("LOCALAPPDATA", r"AppData\Local").join("eco")
    }

    fn config_file(&self) -> PathBuf {
        self.base("APPDATA", r"AppData\Roaming")
            .join("eco")
            .join("config.toml")
    }

    fn socket_path(&self) -> PathBuf {
        let user = (self.var)("USERNAME").unwrap_or_default();
        let mut name = OsString::from(r"\\.\pipe\eco-");
        name.push(user);
        PathBuf::from(name)
    }

    fn token_path(&self) -> PathBuf {
        self.data_dir().join("eco.token")
    }

    fn update_state(&self) -> PathBuf {
        self.data_dir().join("update.json")
    }

    fn daemon_log(&self) -> PathBuf {
        self.data_dir().join("daemon.log")
    }

    fn start_menu_dir(&self) -> PathBuf {
        self.base("APPDATA", r"AppData\Roaming")
            .join(r"Microsoft\Windows\Start Menu\Programs")
    }

    fn startup_dir(&self) -> PathBuf {
        self.start_menu_dir().join("Startup")
    }
}

fn layout() -> Layout<impl Fn(&str) -> Option<OsString>> {
    Layout {
        var: |name: &str| env::var_os(name),
    }
}

#[cfg(unix)]
fn users_uid() -> u32 {
    std::fs::metadata("/proc/self")
        .map(|m| std::os::unix::fs::MetadataExt::uid(&m))
        .unwrap_or(0)
}

pub fn home() -> PathBuf {
    layout().home()
}

/// `<prefix>/<directory>/<path>` for the running `<prefix>/bin/eco`, when it is there.
fn beside_binary(directory: &str, path: &str) -> Option<PathBuf> {
    beside(&env::current_exe().ok()?, directory, path)
}

/// [`beside_binary`] for the binary `exe`.
fn beside(exe: &Path, directory: &str, path: &str) -> Option<PathBuf> {
    Some(under(exe.parent()?.parent()?, directory, path)).filter(|file| file.exists())
}

/// `<prefix>/<directory>/<path>`, each `/`-separated part its own component,
/// so the path has the platform's separator throughout.
fn under(prefix: &Path, directory: &str, path: &str) -> PathBuf {
    directory
        .split('/')
        .chain(path.split('/'))
        .fold(prefix.to_path_buf(), |found, part| found.join(part))
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
#[cfg(unix)]
pub fn hypr_bindings() -> PathBuf {
    layout().hypr_bindings()
}

/// The file name of the Silero VAD model.
pub const VAD_MODEL_FILE: &str = "silero_vad.onnx";

/// The file name of the WeSpeaker CAM++ speaker model.
pub const SPEAKER_MODEL_FILE: &str = "wespeaker_campplus.onnx";

/// The model `file` a package ships, `<prefix>/share/eco/models/<file>`.
pub fn shipped_model(file: &str) -> Option<PathBuf> {
    shipped(&format!("models/{file}"))
}

/// Where `eco setup` downloads the model `file` to.
pub fn downloaded_model(file: &str) -> PathBuf {
    layout().model(file)
}

/// The model `file` eco loads: the one its package ships, else the one `eco
/// setup` downloaded.
fn model(exe: Option<&Path>, file: &str) -> PathBuf {
    exe.and_then(|exe| beside(exe, "share/eco", &format!("models/{file}")))
        .unwrap_or_else(|| downloaded_model(file))
}

pub fn vad_model() -> PathBuf {
    model(env::current_exe().ok().as_deref(), VAD_MODEL_FILE)
}

pub fn speaker_model() -> PathBuf {
    model(env::current_exe().ok().as_deref(), SPEAKER_MODEL_FILE)
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

/// Where the daemon keeps what its last check for a newer release found.
pub fn update_state() -> PathBuf {
    layout().update_state()
}

/// Where a running daemon keeps the windows' token, for a window it did not start.
pub fn token_path() -> PathBuf {
    layout().token_path()
}

/// Where the daemon started in the background writes its output.
#[cfg(windows)]
pub fn daemon_log() -> PathBuf {
    layout().daemon_log()
}

/// The folder whose programs Windows runs at the user's login.
#[cfg(windows)]
pub fn startup_dir() -> PathBuf {
    layout().startup_dir()
}

/// The socket where the running Hyprland tells its events, when eco runs under one.
#[cfg(unix)]
pub fn hypr_events() -> Option<PathBuf> {
    layout().hypr_events()
}

/// Where Omarchy installs the omapass plugin's CLI.
#[cfg(unix)]
pub fn omapass_plugin() -> PathBuf {
    layout().omapass_plugin()
}

/// `path` with a leading `~/` replaced by the home.
pub fn expand(path: &str) -> PathBuf {
    layout().expand(path)
}

/// The real file `path` names, through every link, absolute.
#[cfg(unix)]
pub fn real(path: &Path) -> std::io::Result<PathBuf> {
    path.canonicalize()
}

/// The real file `path` names, through every link, absolute and written as
/// the user writes it: Windows names a disk `\\?\C:\…` there, the user `C:\…`.
#[cfg(windows)]
pub fn real(path: &Path) -> std::io::Result<PathBuf> {
    let real = path.canonicalize()?;
    let plain = real
        .to_str()
        .and_then(|text| text.strip_prefix(r"\\?\"))
        .filter(|rest| rest.as_bytes().get(1) == Some(&b':'))
        .map(PathBuf::from);
    Ok(plain.unwrap_or(real))
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

    #[test]
    fn the_update_state_is_eco_s_own_file() {
        assert!(update_state().ends_with(Path::new("eco").join("update.json")));
    }

    #[cfg(unix)]
    fn all(layout: &Layout<impl Fn(&str) -> Option<OsString>>) -> Vec<PathBuf> {
        vec![
            layout.config_file(),
            layout.hypr_bindings(),
            layout.model(VAD_MODEL_FILE),
            layout.model(SPEAKER_MODEL_FILE),
            layout.people_dir(),
            layout.sessions_dir(),
            layout.socket_path(),
            layout.token_path(),
            layout.update_state(),
            layout.omapass_plugin(),
            layout.expand("~/cv.md"),
            layout.expand("/etc/cv.md"),
        ]
    }

    #[cfg(unix)]
    #[test]
    fn paths_follow_the_xdg_variables_when_set() {
        let layout = with(&[
            ("HOME", "/home/you"),
            ("XDG_CONFIG_HOME", "/cfg"),
            ("XDG_DATA_HOME", "/data"),
            ("XDG_RUNTIME_DIR", "/run/user/1000"),
            ("XDG_STATE_HOME", "/state"),
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
            "/state/eco/update.json",
            "/home/you/.config/omarchy/plugins/io.github.this-is-npc.omapass/bin/omapass",
            "/home/you/cv.md",
            "/etc/cv.md",
        ];
        assert_eq!(all(&layout), expected.map(PathBuf::from));
    }

    #[cfg(unix)]
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
            "/home/you/.local/state/eco/update.json".into(),
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
            ("XDG_STATE_HOME", ""),
        ]);
        assert_eq!(all(&unset), expected);
        assert_eq!(all(&empty), expected);
    }

    #[cfg(unix)]
    #[test]
    fn without_a_home_paths_are_relative() {
        let layout = with(&[]);
        assert_eq!(
            layout.config_file(),
            PathBuf::from(".config/eco/config.toml")
        );
        assert_eq!(layout.expand("~/cv.md"), PathBuf::from("cv.md"));
    }

    #[cfg(unix)]
    #[test]
    fn hyprland_events_are_in_its_instance_directory_and_only_under_it() {
        let under = with(&[
            ("XDG_RUNTIME_DIR", "/run/user/1000"),
            ("HYPRLAND_INSTANCE_SIGNATURE", "abc"),
        ]);
        assert_eq!(
            under.hypr_events(),
            Some(PathBuf::from("/run/user/1000/hypr/abc/.socket2.sock"))
        );
        let without = with(&[("XDG_RUNTIME_DIR", "/run/user/1000")]);
        assert_eq!(without.hypr_events(), None);
        let empty = with(&[("HYPRLAND_INSTANCE_SIGNATURE", "")]);
        assert_eq!(empty.hypr_events(), None);
    }

    #[cfg(windows)]
    #[test]
    fn windows_paths_follow_the_application_data_variables() {
        let layout = with(&[
            ("USERPROFILE", r"C:\Users\you"),
            ("APPDATA", r"C:\Users\you\AppData\Roaming"),
            ("LOCALAPPDATA", r"C:\Users\you\AppData\Local"),
            ("USERNAME", "you"),
        ]);
        let found = [
            layout.config_file(),
            layout.model(VAD_MODEL_FILE),
            layout.model(SPEAKER_MODEL_FILE),
            layout.people_dir(),
            layout.sessions_dir(),
            layout.socket_path(),
            layout.token_path(),
            layout.daemon_log(),
            layout.update_state(),
            layout.startup_dir(),
            layout.start_menu_dir(),
            layout.expand("~/cv.md"),
        ];
        let expected = [
            r"C:\Users\you\AppData\Roaming\eco\config.toml",
            r"C:\Users\you\AppData\Local\eco\models\silero_vad.onnx",
            r"C:\Users\you\AppData\Local\eco\models\wespeaker_campplus.onnx",
            r"C:\Users\you\AppData\Local\eco\people",
            r"C:\Users\you\AppData\Local\eco\sessions",
            r"\\.\pipe\eco-you",
            r"C:\Users\you\AppData\Local\eco\eco.token",
            r"C:\Users\you\AppData\Local\eco\daemon.log",
            r"C:\Users\you\AppData\Local\eco\update.json",
            r"C:\Users\you\AppData\Roaming\Microsoft\Windows\Start Menu\Programs\Startup",
            r"C:\Users\you\AppData\Roaming\Microsoft\Windows\Start Menu\Programs",
            r"C:\Users\you\cv.md",
        ];
        assert_eq!(found, expected.map(PathBuf::from));
    }

    #[cfg(windows)]
    #[test]
    fn unset_application_data_falls_back_under_the_profile() {
        let layout = with(&[("USERPROFILE", r"C:\Users\you"), ("APPDATA", "")]);
        assert_eq!(
            layout.config_file(),
            PathBuf::from(r"C:\Users\you\AppData\Roaming\eco\config.toml")
        );
        assert_eq!(
            layout.sessions_dir(),
            PathBuf::from(r"C:\Users\you\AppData\Local\eco\sessions")
        );
    }

    #[test]
    fn a_shipped_model_wins_over_the_downloaded_one() {
        let prefix = tempfile::tempdir().unwrap();
        let exe = prefix.path().join("bin").join("eco");
        assert_eq!(
            model(Some(&exe), VAD_MODEL_FILE),
            downloaded_model(VAD_MODEL_FILE)
        );
        let models = prefix.path().join("share").join("eco").join("models");
        std::fs::create_dir_all(&models).unwrap();
        std::fs::write(models.join(VAD_MODEL_FILE), "").unwrap();
        assert_eq!(
            model(Some(&exe), VAD_MODEL_FILE),
            models.join(VAD_MODEL_FILE)
        );
        // Another model, and an unknown binary, use the download.
        assert_eq!(
            model(Some(&exe), SPEAKER_MODEL_FILE),
            downloaded_model(SPEAKER_MODEL_FILE)
        );
        assert_eq!(
            model(None, VAD_MODEL_FILE),
            downloaded_model(VAD_MODEL_FILE)
        );
    }

    #[test]
    fn a_shipped_path_has_the_platforms_separator() {
        let found = under(Path::new("prefix"), "lib/eco", "overlay/shell.qml");
        let expected: PathBuf = ["prefix", "lib", "eco", "overlay", "shell.qml"]
            .iter()
            .collect();
        assert_eq!(found, expected);
        assert_eq!(found.as_os_str(), expected.as_os_str());
    }

    #[test]
    fn the_real_file_is_absolute_and_plain() {
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("a.jsonl");
        std::fs::write(&file, "").unwrap();
        let relative = directory.path().join(".").join("a.jsonl");
        let found = real(&relative).unwrap();
        assert!(found.is_absolute());
        assert!(!found.to_string_lossy().starts_with(r"\\?\"), "{found:?}");
        assert_eq!(found.file_name(), file.file_name());
        assert!(real(&directory.path().join("none")).is_err());
    }
}
