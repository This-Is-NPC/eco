//! macOS: eco as the launchd agent `io.github.this-is-npc.eco` in the user's GUI domain, defined by
//! `~/Library/LaunchAgents/io.github.this-is-npc.eco.plist` and run through `launchctl`, its output
//! in a log under the data directory. The agent runs when started, never at
//! login, as on Linux and Windows.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::{Context, Result, bail};
use futures::future::BoxFuture;
use tokio::process::Command;

use crate::adapters::daemon_log;
use crate::config::replace_file;
use crate::paths;
use crate::ports::ServiceManager;

pub struct LaunchAgent;

impl ServiceManager for LaunchAgent {
    fn start(&self) -> BoxFuture<'_, Result<()>> {
        Box::pin(async {
            let agent = Agent::user();
            let exe = std::env::current_exe()?;
            let path = agent_path(&std::env::var("PATH").unwrap_or_default());
            start(
                Path::new("launchctl"),
                &agent,
                &plist(&exe, &agent.log, &path),
            )
            .await
        })
    }

    /// The daemon log, which each start empties: all of it is the last start's.
    fn output(&self, _: SystemTime) -> BoxFuture<'_, Option<String>> {
        Box::pin(async { daemon_log::read(&paths::daemon_log()) })
    }
}

/// Where the agent is defined and writes, and the launchd domain it runs in.
struct Agent {
    plist: PathBuf,
    log: PathBuf,
    /// `gui/<uid>`: the user's login session.
    domain: String,
}

impl Agent {
    /// This user's agent.
    fn user() -> Self {
        Self {
            plist: paths::launch_agent(),
            log: paths::daemon_log(),
            domain: format!("gui/{}", rustix::process::getuid().as_raw()),
        }
    }

    /// The agent as launchd names it: `gui/<uid>/io.github.this-is-npc.eco`.
    fn target(&self) -> String {
        format!("{}/{}", self.domain, paths::LAUNCH_AGENT)
    }
}

/// The agent's definition: `exe daemon --headless`, its output in `log`, with
/// `path` (`agent_path`), so it finds Homebrew programs, ffmpeg among them. It
/// does not run at load, so not at login.
fn plist(exe: &Path, log: &Path, path: &str) -> String {
    let exe = escape(&exe.to_string_lossy());
    let log = escape(&log.to_string_lossy());
    let path = escape(path);
    let label = paths::LAUNCH_AGENT;
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>Label</key>
	<string>{label}</string>
	<key>ProgramArguments</key>
	<array>
		<string>{exe}</string>
		<string>daemon</string>
		<string>--headless</string>
	</array>
	<key>EnvironmentVariables</key>
	<dict>
		<key>PATH</key>
		<string>{path}</string>
	</dict>
	<key>StandardOutPath</key>
	<string>{log}</string>
	<key>StandardErrorPath</key>
	<string>{log}</string>
	<key>RunAtLoad</key>
	<false/>
	<key>ProcessType</key>
	<string>Interactive</string>
</dict>
</plist>
"#
    )
}

/// Where Homebrew puts its programs, on Apple Silicon and on Intel.
const HOMEBREW: [&str; 2] = ["/opt/homebrew/bin", "/usr/local/bin"];

/// The agent's `PATH`: `caller`'s, then Homebrew's folders it lacks. eco
/// opened from Finder or Launchpad has launchd's own, which holds neither.
fn agent_path(caller: &str) -> String {
    let mut path: Vec<&str> = caller.split(':').filter(|dir| !dir.is_empty()).collect();
    for dir in HOMEBREW {
        if !path.contains(&dir) {
            path.push(dir);
        }
    }
    path.join(":")
}

/// `text` as XML character data.
fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Empty the log, define the agent as `definition` and start it, through the
/// program `launchctl`. An agent launchd holds with another definition is
/// taken out first, so the new one is what runs.
async fn start(launchctl: &Path, agent: &Agent, definition: &str) -> Result<()> {
    daemon_log::emptied(&agent.log)?;
    let target = agent.target();
    let changed = std::fs::read_to_string(&agent.plist).ok().as_deref() != Some(definition);
    let loaded = Command::new(launchctl)
        .args(["print", &target])
        .output()
        .await
        .is_ok_and(|output| output.status.success());
    if changed {
        if loaded {
            run(
                launchctl,
                &["bootout", &target],
                "cannot replace the eco agent",
            )
            .await?;
        }
        if let Some(directory) = agent.plist.parent() {
            std::fs::create_dir_all(directory)?;
        }
        replace_file(&agent.plist, definition.as_bytes(), 0o644)
            .with_context(|| format!("cannot write {}", agent.plist.display()))?;
    }
    if changed || !loaded {
        let plist = agent.plist.to_string_lossy();
        run(
            launchctl,
            &["bootstrap", &agent.domain, &plist],
            "cannot load the eco agent",
        )
        .await?;
    }
    run(
        launchctl,
        &["kickstart", &target],
        "cannot start the eco agent",
    )
    .await
}

/// Take this user's agent off, as `eco uninstall` does: out of launchd and its
/// definition deleted; nothing to do when there is none.
pub async fn take_off() -> Result<()> {
    take_off_agent(Path::new("launchctl"), &Agent::user()).await
}

/// Take `agent` off through the program `launchctl`: `bootout` when launchd
/// holds it, then its definition deleted.
async fn take_off_agent(launchctl: &Path, agent: &Agent) -> Result<()> {
    let target = agent.target();
    let loaded = Command::new(launchctl)
        .args(["print", &target])
        .output()
        .await
        .is_ok_and(|output| output.status.success());
    if loaded {
        run(
            launchctl,
            &["bootout", &target],
            "cannot take the eco agent off",
        )
        .await?;
    }
    match std::fs::remove_file(&agent.plist) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
            Err(error).with_context(|| format!("cannot delete {}", agent.plist.display()))
        }
        _ => Ok(()),
    }
}

/// Run `launchctl` with `args`; on failure, `failed` and what it said.
async fn run(launchctl: &Path, args: &[&str], failed: &str) -> Result<()> {
    let output = Command::new(launchctl)
        .args(args)
        .output()
        .await
        .with_context(|| format!("{failed}: cannot run {}", launchctl.display()))?;
    if !output.status.success() {
        bail!(
            "{failed}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::adapters::fake_program::fake_program;

    /// A launchctl that notes each call in `dir/calls`, answers `print` with
    /// `printed` (0 when launchd holds the agent) and fails the call whose
    /// first argument is `fails`.
    fn launchctl(dir: &Path, printed: u8, fails: &str) -> PathBuf {
        let calls = dir.join("calls");
        fake_program(
            dir,
            "launchctl",
            &format!(
                r#"echo "$*" >> '{}'
[ "$1" = "print" ] && exit {printed}
[ "$1" = "{fails}" ] && echo "  Bootstrap failed: 5: Input/output error  " >&2 && exit 5
exit 0"#,
                calls.display()
            ),
        )
    }

    fn calls(dir: &Path) -> String {
        fs::read_to_string(dir.join("calls")).unwrap_or_default()
    }

    fn agent(dir: &Path) -> Agent {
        Agent {
            plist: dir.join("LaunchAgents/io.github.this-is-npc.eco.plist"),
            log: dir.join("data/daemon.log"),
            domain: "gui/501".into(),
        }
    }

    #[test]
    fn the_agent_runs_the_daemon_with_the_shells_path_and_not_at_login() {
        let text = plist(
            Path::new("/Apps/eco & co/eco"),
            Path::new("/data/daemon.log"),
            "/opt/homebrew/bin:/usr/bin",
        );
        let strings: Vec<&str> = text
            .lines()
            .filter_map(|line| {
                line.trim()
                    .strip_prefix("<string>")?
                    .strip_suffix("</string>")
            })
            .collect();
        assert_eq!(
            strings,
            [
                "io.github.this-is-npc.eco",
                "/Apps/eco &amp; co/eco",
                "daemon",
                "--headless",
                "/opt/homebrew/bin:/usr/bin",
                "/data/daemon.log",
                "/data/daemon.log",
                "Interactive"
            ]
        );
        assert!(text.contains("<key>RunAtLoad</key>\n\t<false/>"), "{text}");
    }

    #[test]
    fn the_agent_finds_homebrew_whatever_path_started_it() {
        assert_eq!(
            agent_path("/usr/bin:/bin:/usr/sbin:/sbin"),
            "/usr/bin:/bin:/usr/sbin:/sbin:/opt/homebrew/bin:/usr/local/bin"
        );
        assert_eq!(
            agent_path("/opt/homebrew/bin:/Users/you/.cargo/bin:/usr/local/bin:/usr/bin"),
            "/opt/homebrew/bin:/Users/you/.cargo/bin:/usr/local/bin:/usr/bin"
        );
        assert_eq!(agent_path(""), "/opt/homebrew/bin:/usr/local/bin");
    }

    #[tokio::test]
    async fn a_new_agent_is_written_loaded_and_started_with_an_empty_log() {
        let dir = tempfile::tempdir().unwrap();
        let program = launchctl(dir.path(), 113, "none");
        let agent = agent(dir.path());
        fs::create_dir_all(agent.log.parent().unwrap()).unwrap();
        fs::write(&agent.log, "an earlier start").unwrap();
        start(&program, &agent, "<plist/>").await.unwrap();
        assert_eq!(fs::read_to_string(&agent.plist).unwrap(), "<plist/>");
        assert_eq!(fs::read_to_string(&agent.log).unwrap(), "");
        assert_eq!(
            calls(dir.path()),
            format!(
                "print gui/501/io.github.this-is-npc.eco\nbootstrap gui/501 {}\nkickstart gui/501/io.github.this-is-npc.eco\n",
                agent.plist.display()
            )
        );
    }

    #[tokio::test]
    async fn an_agent_launchd_holds_as_written_is_only_started() {
        let dir = tempfile::tempdir().unwrap();
        let program = launchctl(dir.path(), 0, "none");
        let agent = agent(dir.path());
        fs::create_dir_all(agent.plist.parent().unwrap()).unwrap();
        fs::write(&agent.plist, "<plist/>").unwrap();
        start(&program, &agent, "<plist/>").await.unwrap();
        assert_eq!(
            calls(dir.path()),
            "print gui/501/io.github.this-is-npc.eco\nkickstart gui/501/io.github.this-is-npc.eco\n"
        );
    }

    #[tokio::test]
    async fn an_agent_held_with_another_definition_is_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let program = launchctl(dir.path(), 0, "none");
        let agent = agent(dir.path());
        fs::create_dir_all(agent.plist.parent().unwrap()).unwrap();
        fs::write(&agent.plist, "<plist>old</plist>").unwrap();
        start(&program, &agent, "<plist/>").await.unwrap();
        assert_eq!(fs::read_to_string(&agent.plist).unwrap(), "<plist/>");
        assert_eq!(
            calls(dir.path()),
            format!(
                "print gui/501/io.github.this-is-npc.eco\nbootout gui/501/io.github.this-is-npc.eco\nbootstrap gui/501 {}\n\
                 kickstart gui/501/io.github.this-is-npc.eco\n",
                agent.plist.display()
            )
        );
    }

    #[tokio::test]
    async fn a_failure_says_what_launchctl_said() {
        let dir = tempfile::tempdir().unwrap();
        let program = launchctl(dir.path(), 113, "bootstrap");
        let error = start(&program, &agent(dir.path()), "<plist/>")
            .await
            .unwrap_err();
        assert_eq!(
            error.to_string(),
            "cannot load the eco agent: Bootstrap failed: 5: Input/output error"
        );
        assert!(!calls(dir.path()).contains("kickstart"));
    }

    #[tokio::test]
    async fn an_agent_is_taken_out_of_launchd_and_its_definition_deleted() {
        let dir = tempfile::tempdir().unwrap();
        let program = launchctl(dir.path(), 0, "none");
        let agent = agent(dir.path());
        fs::create_dir_all(agent.plist.parent().unwrap()).unwrap();
        fs::write(&agent.plist, "<plist/>").unwrap();
        take_off_agent(&program, &agent).await.unwrap();
        assert!(!agent.plist.exists());
        assert_eq!(
            calls(dir.path()),
            "print gui/501/io.github.this-is-npc.eco\nbootout gui/501/io.github.this-is-npc.eco\n"
        );
    }

    #[tokio::test]
    async fn no_agent_is_nothing_to_take_off() {
        let dir = tempfile::tempdir().unwrap();
        let program = launchctl(dir.path(), 113, "none");
        take_off_agent(&program, &agent(dir.path())).await.unwrap();
        assert_eq!(
            calls(dir.path()),
            "print gui/501/io.github.this-is-npc.eco\n"
        );
    }

    #[tokio::test]
    async fn a_missing_launchctl_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("launchctl");
        let agent = agent(dir.path());
        let error = start(&missing, &agent, "<plist/>").await.unwrap_err();
        assert_eq!(
            error.to_string(),
            format!(
                "cannot load the eco agent: cannot run {}",
                missing.display()
            )
        );
    }
}
