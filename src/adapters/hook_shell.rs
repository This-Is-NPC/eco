//! Hooks as shell commands, run by the platform's shell: the session reaches
//! them in `ECO_SESSION` and `ECO_TITLE`.

use std::process::Stdio;
use std::time::Duration;

use futures::FutureExt;
use futures::future::BoxFuture;
use tokio::process::Command;

use crate::ports::{HookError, Hooks};

/// How long a hook may run before it counts as failed and is stopped.
const LIMIT: Duration = Duration::from_secs(60);

/// `command` run by the platform's shell: `sh -c` on Linux.
#[cfg(unix)]
pub fn shell(command: &str) -> Command {
    let mut shell = Command::new("sh");
    shell.arg("-c").arg(command);
    shell
}

/// `command` run by the platform's shell: `cmd /S /C` on Windows. cmd does not
/// read its line by the quoting rules other programs use, so `command` is
/// handed over as it is, in the quotes `/S` strips.
#[cfg(windows)]
pub fn shell(command: &str) -> Command {
    let mut shell = Command::new("cmd");
    shell.args(["/S", "/C"]).raw_arg(format!("\"{command}\""));
    shell
}

pub struct ShellHooks;

impl Hooks for ShellHooks {
    fn run(
        &self,
        command: String,
        session_id: String,
        title: String,
    ) -> BoxFuture<'static, Result<(), HookError>> {
        async move {
            let child = shell(&command)
                .env("ECO_SESSION", session_id)
                .env("ECO_TITLE", title)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .kill_on_drop(true)
                .output();
            let output = tokio::time::timeout(LIMIT, child)
                .await
                .map_err(|_| HookError(format!("no exit after {} s", LIMIT.as_secs())))?
                .map_err(|e| HookError(e.to_string()))?;
            if output.status.success() {
                return Ok(());
            }
            let stderr = String::from_utf8_lossy(&output.stderr);
            let last = stderr.lines().rfind(|line| !line.trim().is_empty());
            Err(HookError(match last {
                Some(line) => line.trim().to_string(),
                None => output.status.to_string(),
            }))
        }
        .boxed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn a_command_is_handed_to_sh() {
        let built = shell("echo 'a b' | wc");
        let args: Vec<_> = built.as_std().get_args().collect();
        assert_eq!(built.as_std().get_program(), "sh");
        assert_eq!(args, ["-c", "echo 'a b' | wc"]);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn the_command_reads_the_session_from_its_environment() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("out");
        let command = format!(
            "printf '%s|%s' \"$ECO_SESSION\" \"$ECO_TITLE\" > {}",
            out.display()
        );
        ShellHooks
            .run(command, "abc123".into(), "Daily".into())
            .await
            .unwrap();
        assert_eq!(std::fs::read_to_string(out).unwrap(), "abc123|Daily");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_failing_command_says_why() {
        let failure = ShellHooks
            .run("echo 'no token' >&2; exit 3".into(), "a".into(), "b".into())
            .await
            .unwrap_err();
        assert_eq!(failure.0, "no token");
        let silent = ShellHooks
            .run("exit 4".into(), "a".into(), "b".into())
            .await
            .unwrap_err();
        assert_eq!(silent.0, "exit status: 4");
    }

    #[cfg(windows)]
    #[test]
    fn a_command_is_handed_to_cmd_as_it_is() {
        let built = shell(r#"echo "a b" | find "a""#);
        let args: Vec<_> = built.as_std().get_args().collect();
        assert_eq!(built.as_std().get_program(), "cmd");
        assert_eq!(args, ["/S", "/C", r#""echo "a b" | find "a"""#]);
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn the_command_reads_the_session_from_its_environment() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("out file");
        let command = format!(r#"echo %ECO_SESSION%^|%ECO_TITLE%>"{}""#, out.display());
        ShellHooks
            .run(command, "abc123".into(), "Daily".into())
            .await
            .unwrap();
        assert_eq!(std::fs::read_to_string(out).unwrap(), "abc123|Daily\r\n");
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn a_failing_command_says_why() {
        let failure = ShellHooks
            .run(
                ">&2 echo no token& exit /b 3".into(),
                "a".into(),
                "b".into(),
            )
            .await
            .unwrap_err();
        assert_eq!(failure.0, "no token");
        let silent = ShellHooks
            .run("exit /b 4".into(), "a".into(), "b".into())
            .await
            .unwrap_err();
        assert_eq!(silent.0, "exit code: 4");
    }
}
