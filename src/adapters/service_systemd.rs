//! systemd: eco as the user service `eco.service`, run through `systemctl --user`.

use std::ffi::{OsStr, OsString};
use std::path::Path;

use anyhow::{Context, Result, bail};
use futures::future::BoxFuture;
use tokio::process::Command;

use crate::ports::ServiceManager;

/// The variables the service needs to reach the graphical session.
const SESSION: [&str; 3] = [
    "WAYLAND_DISPLAY",
    "HYPRLAND_INSTANCE_SIGNATURE",
    "XDG_CURRENT_DESKTOP",
];

pub struct SystemdUser;

impl ServiceManager for SystemdUser {
    fn start(&self) -> BoxFuture<'_, Result<()>> {
        Box::pin(start(
            Path::new("systemctl"),
            session(|name| std::env::var_os(name)),
        ))
    }
}

/// This shell's graphical session as `NAME=value` assignments: each variable
/// of `SESSION` that `var` holds.
fn session(var: impl Fn(&str) -> Option<OsString>) -> Vec<OsString> {
    SESSION
        .into_iter()
        .filter_map(|name| {
            let mut assignment = OsString::from(name);
            assignment.push("=");
            assignment.push(var(name)?);
            Some(assignment)
        })
        .collect()
}

/// Hand the user manager `session`, when there is one, then start eco.service,
/// both through the program `systemctl`.
async fn start(systemctl: &Path, session: Vec<OsString>) -> Result<()> {
    if !session.is_empty() {
        let mut import = vec![OsString::from("--user"), "set-environment".into()];
        import.extend(session);
        run(
            systemctl,
            &import,
            "cannot update the user service environment",
        )
        .await?;
    }
    run(
        systemctl,
        &["--user", "start", "eco.service"],
        "cannot start eco.service",
    )
    .await
}

/// Run `systemctl` with `args`; on failure, `failed` and what it said.
async fn run(systemctl: &Path, args: &[impl AsRef<OsStr>], failed: &str) -> Result<()> {
    let output = Command::new(systemctl)
        .args(args)
        .output()
        .await
        .with_context(|| format!("{failed}: cannot run {}", systemctl.display()))?;
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

    #[test]
    fn the_session_is_each_variable_the_shell_holds() {
        let assignments = session(|name| {
            (name != "HYPRLAND_INSTANCE_SIGNATURE").then(|| OsString::from(format!("{name}-v")))
        });
        assert_eq!(
            assignments,
            [
                "WAYLAND_DISPLAY=WAYLAND_DISPLAY-v",
                "XDG_CURRENT_DESKTOP=XDG_CURRENT_DESKTOP-v"
            ]
        );
        assert!(session(|_| None).is_empty());
    }

    /// A systemctl that notes each call in `dir/calls` and fails the one whose
    /// second argument is `fails`.
    fn systemctl(dir: &Path, fails: &str) -> std::path::PathBuf {
        let calls = dir.join("calls");
        fake_program(
            dir,
            "systemctl",
            &format!(
                r#"echo "$*" >> '{}'
[ "$2" = "{fails}" ] && echo "  Unit eco.service not found.  " >&2 && exit 5
exit 0"#,
                calls.display()
            ),
        )
    }

    fn calls(dir: &Path) -> String {
        fs::read_to_string(dir.join("calls")).unwrap_or_default()
    }

    #[tokio::test]
    async fn the_session_is_imported_before_the_service_starts() {
        let dir = tempfile::tempdir().unwrap();
        let program = systemctl(dir.path(), "none");
        start(&program, vec!["WAYLAND_DISPLAY=wayland-1".into()])
            .await
            .unwrap();
        assert_eq!(
            calls(dir.path()),
            "--user set-environment WAYLAND_DISPLAY=wayland-1\n--user start eco.service\n"
        );
    }

    #[tokio::test]
    async fn without_a_session_only_the_service_starts() {
        let dir = tempfile::tempdir().unwrap();
        let program = systemctl(dir.path(), "none");
        start(&program, Vec::new()).await.unwrap();
        assert_eq!(calls(dir.path()), "--user start eco.service\n");
    }

    #[tokio::test]
    async fn a_failure_says_what_systemctl_said() {
        let dir = tempfile::tempdir().unwrap();
        let program = systemctl(dir.path(), "start");
        let error = start(&program, Vec::new()).await.unwrap_err();
        assert_eq!(
            error.to_string(),
            "cannot start eco.service: Unit eco.service not found."
        );

        let dir = tempfile::tempdir().unwrap();
        let program = systemctl(dir.path(), "set-environment");
        let error = start(&program, vec!["WAYLAND_DISPLAY=w".into()])
            .await
            .unwrap_err();
        assert_eq!(
            error.to_string(),
            "cannot update the user service environment: Unit eco.service not found."
        );
        assert_eq!(
            calls(dir.path()),
            "--user set-environment WAYLAND_DISPLAY=w\n"
        );
    }

    #[tokio::test]
    async fn a_missing_systemctl_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("systemctl");
        let error = start(&missing, Vec::new()).await.unwrap_err();
        assert_eq!(
            error.to_string(),
            format!("cannot start eco.service: cannot run {}", missing.display())
        );
    }
}
