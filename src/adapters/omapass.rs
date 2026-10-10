//! omapass, the user's password plugin over gnome-keyring: its accounts (never
//! their secrets) for the config window, and one account's secret as a
//! provider's key.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use serde_json::Value;
use tokio::process::Command;

use crate::paths;

/// One login omapass keeps.
#[derive(Debug, Clone, PartialEq)]
pub struct Account {
    pub account: String,
    pub folder: String,
}

/// The omapass CLI: on PATH, or where Omarchy installs the plugin.
fn program() -> Option<PathBuf> {
    find(std::env::var_os("PATH"), paths::omapass_plugin())
}

/// `omapass` in the first directory of `path` that has it, else `plugin` if it is a file.
fn find(path: Option<OsString>, plugin: PathBuf) -> Option<PathBuf> {
    path.into_iter()
        .flat_map(|path| std::env::split_paths(&path).collect::<Vec<_>>())
        .map(|dir| dir.join("omapass"))
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
    accounts_of(program()).await
}

async fn accounts_of(program: Option<PathBuf>) -> Result<Vec<Account>, String> {
    let listed = run(&program.ok_or_else(missing)?, &["list", "--json"]).await?;
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
    secret_of(program(), account).await
}

async fn secret_of(program: Option<PathBuf>, account: &str) -> Result<String, String> {
    // Piped, so omapass prints the secret; it is never logged.
    let secret = run(&program.ok_or_else(missing)?, &["get", "--", account]).await?;
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
    use super::*;
    use crate::adapters::fake_program::fake_program;

    /// A stand-in omapass in `directory` that runs `body` for every command.
    fn scripted(directory: &Path, body: &str) -> Option<PathBuf> {
        Some(fake_program(directory, "omapass", body))
    }

    /// A stand-in omapass that knows two passwords and a Nostr key.
    fn fake(directory: &Path) -> Option<PathBuf> {
        let script = r#"case "$1" in
  list) echo '[{"account":"openrouter","folder":"AI","kind":""},{"account":"nostr:default","folder":"Nostr","kind":"nostr"},{"account":"deepgram","folder":"AI","kind":""}]' ;;
  get) [ "$3" = "deepgram" ] && printf 'dg-secret\n' && exit 0; echo "omapass: no such account: $3" >&2; exit 1 ;;
esac"#;
        scripted(directory, script)
    }

    #[tokio::test]
    async fn accounts_are_its_passwords_and_secrets_come_piped() {
        let directory = tempfile::tempdir().unwrap();
        let omapass = fake(directory.path());
        let accounts = accounts_of(omapass.clone()).await.unwrap();
        let names: Vec<&str> = accounts.iter().map(|a| a.account.as_str()).collect();
        assert_eq!(names, ["deepgram", "openrouter"]);
        assert_eq!(accounts[0].folder, "AI");
        assert_eq!(
            secret_of(omapass.clone(), "deepgram").await.unwrap(),
            "dg-secret"
        );
        let failure = secret_of(omapass, "nope").await.unwrap_err();
        assert_eq!(failure, "omapass: omapass: no such account: nope");
        assert!(!failure.contains("dg-secret"));
    }

    #[test]
    fn the_program_is_found_on_path_before_the_plugin() {
        let on_path = tempfile::tempdir().unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        let program = scripted(on_path.path(), "exit 0").unwrap();
        let plugin = scripted(elsewhere.path(), "exit 0").unwrap();
        let path = std::env::join_paths([elsewhere.path().join("bin"), on_path.path().into()]);
        assert_eq!(find(path.ok(), plugin.clone()), Some(program));
        assert_eq!(find(None, plugin.clone()), Some(plugin));
        let gone = elsewhere.path().join("none");
        assert_eq!(find(Some(on_path.path().join("x").into()), gone), None);
    }

    #[tokio::test]
    async fn without_the_program_eco_says_where_to_get_it() {
        let missing = format!("omapass is not installed ({PAGE})");
        assert_eq!(accounts_of(None).await.unwrap_err(), missing);
        assert_eq!(secret_of(None, "deepgram").await.unwrap_err(), missing);
    }

    #[tokio::test]
    async fn a_program_that_cannot_run_or_fails_silently_is_an_error() {
        let directory = tempfile::tempdir().unwrap();
        let absent = Some(directory.path().join("omapass"));
        let cannot = accounts_of(absent).await.unwrap_err();
        assert!(cannot.starts_with("omapass: "), "{cannot}");
        let silent = scripted(directory.path(), "echo '  ' >&2; exit 3");
        assert_eq!(accounts_of(silent).await.unwrap_err(), "omapass: failed");
    }

    #[tokio::test]
    async fn a_list_that_is_not_json_or_lacks_names_is_read_as_far_as_it_goes() {
        let directory = tempfile::tempdir().unwrap();
        let garbled = scripted(directory.path(), "echo 'not json'");
        let error = accounts_of(garbled).await.unwrap_err();
        assert!(error.starts_with("omapass: expected"), "{error}");
        let partial = scripted(
            directory.path(),
            r#"echo '[{"folder":"AI"},{"account":"groq"}]'"#,
        );
        let accounts = accounts_of(partial).await.unwrap();
        assert_eq!(
            accounts,
            [Account {
                account: "groq".into(),
                folder: String::new()
            }]
        );
    }

    #[tokio::test]
    async fn a_secret_that_is_blank_or_not_text_is_refused() {
        let directory = tempfile::tempdir().unwrap();
        let blank = scripted(directory.path(), r"printf '  \n'");
        assert_eq!(
            secret_of(blank, "groq").await.unwrap_err(),
            "omapass: groq is empty"
        );
        let binary = scripted(directory.path(), r"printf '\377\376'");
        assert_eq!(
            secret_of(binary, "groq").await.unwrap_err(),
            "omapass: groq is not text"
        );
    }
}
