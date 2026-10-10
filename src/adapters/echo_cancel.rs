//! PipeWire's echo cancellation (WebRTC) on one microphone, loaded into a
//! `pw-cli` that lives exactly as long as this value: dropping it removes the
//! module and its nodes. The reference is what the default output plays
//! (`monitor.mode`), so no application has to play through a new device.

use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use tokio::io::AsyncWriteExt;
use tokio::process::{Child, ChildStdin, Command};

use crate::adapters::pipewire_devices::DEFAULT_INPUT;
use crate::ports::AudioError;

/// Nodes eco creates for itself; never offered as devices.
pub const NODE_PREFIX: &str = "eco.aec.";

/// The echo-cancelled microphone this process captures from.
pub fn source_node() -> String {
    format!("{NODE_PREFIX}source.{}", std::process::id())
}

/// The module running; its `pw-cli` (and the stdin that keeps it reading) go with it.
pub struct EchoCancel {
    _process: Child,
    _input: ChildStdin,
}

fn arguments(mic: &str) -> String {
    let pid = std::process::id();
    let target = if mic == DEFAULT_INPUT {
        String::new()
    } else {
        format!(" target.object = \"{mic}\"")
    };
    format!(
        "{{ library.name = aec/libspa-aec-webrtc monitor.mode = true \
         capture.props = {{ node.name = \"{NODE_PREFIX}capture.{pid}\" node.passive = true{target} }} \
         source.props = {{ node.name = \"{}\" node.description = \"eco: echo cancelled\" }} \
         playback.props = {{ node.name = \"{NODE_PREFIX}playback.{pid}\" }} }}",
        source_node()
    )
}

async fn source_exists(pw_cli: &Path) -> bool {
    let listed = Command::new(pw_cli)
        .args(["ls", "Node"])
        .stderr(Stdio::null())
        .output()
        .await;
    listed.is_ok_and(|out| String::from_utf8_lossy(&out.stdout).contains(&source_node()))
}

/// Cancel, on `mic` (a node name or the default input), what the default
/// output plays; ready once its source exists.
pub async fn start(mic: &str) -> Result<EchoCancel, AudioError> {
    start_with(Path::new("pw-cli"), mic).await
}

/// `start`, through the program `pw_cli`.
async fn start_with(pw_cli: &Path, mic: &str) -> Result<EchoCancel, AudioError> {
    let fail = |detail: String| AudioError(format!("echo cancellation: {detail}"));
    let mut process = Command::new(pw_cli)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| fail(e.to_string()))?;
    let mut input = process.stdin.take().expect("piped");
    let line = format!(
        "load-module libpipewire-module-echo-cancel {}\n",
        arguments(mic)
    );
    input
        .write_all(line.as_bytes())
        .await
        .map_err(|e| fail(e.to_string()))?;
    for _ in 0..30 {
        if source_exists(pw_cli).await {
            return Ok(EchoCancel {
                _process: process,
                _input: input,
            });
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    Err(fail("PipeWire did not create its source".into()))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::time::Instant;

    use super::*;
    use crate::adapters::fake_program::fake_program;

    #[test]
    fn the_module_is_aimed_at_the_microphone() {
        let pid = std::process::id();
        let chosen = arguments("alsa_input.usb-mic");
        assert!(chosen.contains("monitor.mode = true"));
        assert!(chosen.contains("target.object = \"alsa_input.usb-mic\""));
        assert!(chosen.contains(&format!("node.name = \"eco.aec.source.{pid}\"")));
        assert!(!arguments(DEFAULT_INPUT).contains("target.object"));
    }

    /// A pw-cli that, run alone, notes its pid in `dir/pid` and the command it
    /// reads in `dir/loaded`, then stays; asked `ls Node`, it lists this
    /// process's source once `loaded` is there, when `creates`.
    fn pw_cli(dir: &Path, creates: bool) -> PathBuf {
        let listed = if creates {
            source_node()
        } else {
            String::new()
        };
        fake_program(
            dir,
            "pw-cli",
            &format!(
                r#"cd '{}'
if [ "$1 $2" = "ls Node" ]; then
  [ -s loaded ] && echo 'node.name = "{listed}"'
  exit 0
fi
echo $$ > pid
read -r line && echo "$line" > loaded
exec sleep 30"#,
                dir.display()
            ),
        )
    }

    /// Whether the process `pid` is gone, or a zombie, within five seconds.
    fn ends(pid: &str) -> bool {
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(5) {
            match fs::read_to_string(format!("/proc/{pid}/stat")) {
                Err(_) => return true,
                Ok(stat) if stat.contains(") Z ") => return true,
                Ok(_) => std::thread::sleep(Duration::from_millis(20)),
            }
        }
        false
    }

    #[tokio::test]
    async fn the_module_is_loaded_and_unloaded_with_its_value() {
        let dir = tempfile::tempdir().unwrap();
        let program = pw_cli(dir.path(), true);
        let module = start_with(&program, "alsa_input.usb-mic").await.unwrap();
        assert_eq!(
            fs::read_to_string(dir.path().join("loaded")).unwrap(),
            format!(
                "load-module libpipewire-module-echo-cancel {}\n",
                arguments("alsa_input.usb-mic")
            )
        );
        let pid = fs::read_to_string(dir.path().join("pid")).unwrap();
        drop(module);
        assert!(ends(pid.trim()), "pw-cli {pid} still runs");
    }

    #[tokio::test(start_paused = true)]
    async fn a_source_that_never_shows_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let program = pw_cli(dir.path(), false);
        let error = start_with(&program, DEFAULT_INPUT)
            .await
            .err()
            .expect("no source");
        assert_eq!(
            error.0,
            "echo cancellation: PipeWire did not create its source"
        );
    }

    #[tokio::test]
    async fn a_missing_pw_cli_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let error = start_with(&dir.path().join("pw-cli"), DEFAULT_INPUT)
            .await
            .err()
            .expect("no pw-cli");
        assert!(
            error.0.starts_with("echo cancellation: No such file"),
            "{}",
            error.0
        );
    }
}
