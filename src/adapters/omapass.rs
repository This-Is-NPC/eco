//! omapass, the user's password plugin over gnome-keyring: its accounts (never
//! their secrets) for the config window, and one account's secret as a
//! provider's key.

use std::path::{Path, PathBuf};
use std::process::Stdio;

use serde_json::Value;
use tokio::process::Command;

use crate::config;

/// One login omapass keeps.
#[derive(Debug, Clone, PartialEq)]
pub struct Account {
    pub account: String,
    pub folder: String,
}

/// The omapass CLI: on PATH, or where Omarchy installs the plugin.
fn program() -> Option<PathBuf> {
    let on_path = std::env::var_os("PATH")
        .into_iter()
        .flat_map(|path| std::env::split_paths(&path).collect::<Vec<_>>())
        .map(|dir| dir.join("omapass"));
    let plugin =
        config::home().join(".config/omarchy/plugins/io.github.this-is-npc.omapass/bin/omapass");
    on_path
        .chain([plugin])
        .find(|candidate| candidate.is_file())
}

/// Where Omarchy's plugin directory tells how to install omapass.
pub const PAGE: &str = "https://plugins.omarchy.org/plugin.html?id=io.github.this-is-npc.omapass";

/// Whether the omapass CLI is on this machine.
pub fn installed() -> bool {
    program().is_some()
}

fn missing() -> String {
    format!("omapass is not installed ({PAGE})")
}

/// Run omapass with `args`; its stdout, or the last line it wrote to stderr.
async fn run(program: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let output = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .await
        .map_err(|e| format!("omapass: {e}"))?;
    if output.status.success() {
        return Ok(output.stdout);
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let last = stderr.lines().rfind(|line| !line.trim().is_empty());
    Err(format!("omapass: {}", last.unwrap_or("failed").trim()))
}

/// The passwords omapass keeps (not its Nostr keys), by account.
pub async fn accounts() -> Result<Vec<Account>, String> {
    accounts_of(&program().ok_or_else(missing)?).await
}

async fn accounts_of(program: &Path) -> Result<Vec<Account>, String> {
    let listed = run(program, &["list", "--json"]).await?;
    let items: Vec<Value> = serde_json::from_slice(&listed).map_err(|e| format!("omapass: {e}"))?;
    let mut accounts: Vec<Account> = items
        .iter()
        .filter(|item| item["kind"].as_str().unwrap_or("").is_empty())
        .filter_map(|item| {
            Some(Account {
                account: item["account"].as_str()?.into(),
                folder: item["folder"].as_str().unwrap_or("").into(),
            })
        })
        .collect();
    accounts.sort_by(|a, b| a.account.cmp(&b.account));
    Ok(accounts)
}

/// The secret omapass keeps for `account`.
pub async fn secret(account: &str) -> Result<String, String> {
    secret_of(&program().ok_or_else(missing)?, account).await
}

async fn secret_of(program: &Path, account: &str) -> Result<String, String> {
    // Piped, so omapass prints the secret; it is never logged.
    let secret = run(program, &["get", "--", account]).await?;
    let secret =
        String::from_utf8(secret).map_err(|_| format!("omapass: {account} is not text"))?;
    let secret = secret.trim();
    if secret.is_empty() {
        return Err(format!("omapass: {account} is empty"));
    }
    Ok(secret.into())
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    /// A stand-in omapass that knows two passwords and a Nostr key.
    fn fake(directory: &Path) -> PathBuf {
        let path = directory.join("omapass");
        let script = r#"#!/bin/sh
case "$1" in
  list) echo '[{"account":"openrouter","folder":"AI","kind":""},{"account":"nostr:default","folder":"Nostr","kind":"nostr"},{"account":"deepgram","folder":"AI","kind":""}]' ;;
  get) [ "$3" = "deepgram" ] && printf 'dg-secret\n' && exit 0; echo "omapass: no such account: $3" >&2; exit 1 ;;
esac
"#;
        std::fs::write(&path, script).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    #[tokio::test]
    async fn accounts_are_its_passwords_and_secrets_come_piped() {
        let directory = tempfile::tempdir().unwrap();
        let omapass = fake(directory.path());
        let accounts = accounts_of(&omapass).await.unwrap();
        let names: Vec<&str> = accounts.iter().map(|a| a.account.as_str()).collect();
        assert_eq!(names, ["deepgram", "openrouter"]);
        assert_eq!(accounts[0].folder, "AI");
        assert_eq!(secret_of(&omapass, "deepgram").await.unwrap(), "dg-secret");
        let failure = secret_of(&omapass, "nope").await.unwrap_err();
        assert_eq!(failure, "omapass: omapass: no such account: nope");
        assert!(!failure.contains("dg-secret"));
    }
}
