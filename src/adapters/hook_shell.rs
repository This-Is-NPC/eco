//! Hooks as shell commands: the session reaches them in `ECO_SESSION` and `ECO_TITLE`.

use std::process::Stdio;
use std::time::Duration;

use futures::FutureExt;
use futures::future::BoxFuture;
use tokio::process::Command;

use crate::ports::{HookError, Hooks};

/// How long a hook may run before it counts as failed and is stopped.
const LIMIT: Duration = Duration::from_secs(60);

pub struct ShellHooks;

impl Hooks for ShellHooks {
    fn run(
        &self,
        command: String,
        session_id: String,
        title: String,
    ) -> BoxFuture<'static, Result<(), HookError>> {
        async move {
            let child = Command::new("sh")
                .arg("-c")
                .arg(&command)
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
}
