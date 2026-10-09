//! systemd: eco as the user service `eco.service`, run through `systemctl --user`.

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
        Box::pin(async {
            import_session().await?;
            let output = Command::new("systemctl")
                .args(["--user", "start", "eco.service"])
                .output()
                .await
                .context("cannot run systemctl --user")?;
            if !output.status.success() {
                let reason = String::from_utf8_lossy(&output.stderr);
                bail!("cannot start eco.service: {}", reason.trim());
            }
            Ok(())
        })
    }
}

/// Hand the user manager this shell's graphical session, when it has one.
async fn import_session() -> Result<()> {
    let variables: Vec<_> = SESSION
        .into_iter()
        .filter_map(|name| std::env::var_os(name).map(|value| (name, value)))
        .collect();
    if variables.is_empty() {
        return Ok(());
    }
    let mut import = Command::new("systemctl");
    import.args(["--user", "set-environment"]);
    for (name, value) in variables {
        let mut assignment = std::ffi::OsString::from(name);
        assignment.push("=");
        assignment.push(value);
        import.arg(assignment);
    }
    let output = import
        .output()
        .await
        .context("cannot update the user service environment")?;
    if !output.status.success() {
        bail!(
            "cannot update the user service environment: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}
