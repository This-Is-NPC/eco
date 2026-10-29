//! `eco doctor`: whether eco is ready to use, check by check — what was found
//! and, for a check that fails, what to do. A key is checked by the name it
//! comes from, never read.

use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::{Value, json};

use crate::adapters::control_socket::Address;
use crate::adapters::{credentials, omapass};
use crate::cli::{self, Failure};
use crate::config::{self, Config, ConfigError, Key, ModelConfig};
use crate::paths;

/// This eco's version.
const CURRENT: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, PartialEq, Serialize)]
pub struct Check {
    check: String,
    ok: bool,
    detail: String,
    /// What to do when it fails; empty when it passes.
    fix: String,
}

fn pass(check: &str, detail: impl Into<String>) -> Check {
    Check {
        check: check.into(),
        ok: true,
        detail: detail.into(),
        fix: String::new(),
    }
}

fn fail(check: &str, detail: impl Into<String>, fix: impl Into<String>) -> Check {
    Check {
        check: check.into(),
        ok: false,
        detail: detail.into(),
        fix: fix.into(),
    }
}

/// Check this eco and the daemon at `address`, print the checks as one JSON
/// object, and return the exit code: 1 when any check fails.
pub async fn run(address: &Address) -> i32 {
    let (said, code) = report(&checks(address, &Places::here()).await);
    println!("{said}");
    code
}

/// `checks` as `eco doctor` prints them, and its exit code.
fn report(checks: &[Check]) -> (Value, i32) {
    let ok = checks.iter().all(|check| check.ok);
    (json!({"ok": ok, "data": checks}), i32::from(!ok))
}

/// What the checks look at on disk.
struct Places {
    /// The executable that runs.
    exe: PathBuf,
    /// What it ships beside it, each with whether it is there.
    shipped: Vec<(String, bool)>,
    /// The models `eco setup` downloads, by name.
    models: Vec<(&'static str, PathBuf)>,
    config: PathBuf,
    /// The Hyprland config that loads eco's rules.
    #[cfg(unix)]
    bindings: PathBuf,
}

impl Places {
    /// This eco's own.
    fn here() -> Self {
        let window = format!("eco-window{}", std::env::consts::EXE_SUFFIX);
        Self {
            exe: std::env::current_exe().unwrap_or_default(),
            shipped: vec![
                (
                    format!("lib/eco/{window}"),
                    paths::shipped_program(&window).is_some(),
                ),
                (
                    "share/eco/overlay/shell.qml".into(),
                    paths::shipped("overlay/shell.qml").is_some(),
                ),
            ],
            models: vec![
                ("Silero VAD", paths::vad_model()),
                ("WeSpeaker CAM++", paths::speaker_model()),
            ],
            config: paths::config_file(),
            #[cfg(unix)]
            bindings: paths::hypr_bindings(),
        }
    }
}

async fn checks(address: &Address, places: &Places) -> Vec<Check> {
    let greeting = cli::greeting(address).await;
    let config = config::load(&places.config);
    let mut checks = vec![
        installed(&places.exe, &places.shipped),
        models(&places.models),
        config_check(&places.config, &config),
    ];
    if let Ok(config) = &config {
        let sources = sources(config, address).await;
        checks.extend(config.models.iter().map(|model| key(model, &sources)));
    }
    checks.push(daemon(&greeting));
    checks.push(shortcuts(&greeting, places));
    checks
}

/// The executable that runs, and what it ships beside it, each with whether
/// it is there.
fn installed(exe: &Path, shipped: &[(String, bool)]) -> Check {
    let missing: Vec<&str> = shipped
        .iter()
        .filter(|(_, found)| !found)
        .map(|(file, _)| file.as_str())
        .collect();
    match missing.is_empty() {
        true => pass(
            "installed",
            format!("{}, with its window program and overlay", exe.display()),
        ),
        false => fail(
            "installed",
            format!(
                "{} runs without {} beside it",
                exe.display(),
                missing.join(", ")
            ),
            "install eco again (docs/how-to-install-and-remove.md)",
        ),
    }
}

/// The models `eco setup` downloads, by name and where each goes.
fn models(models: &[(&str, PathBuf)]) -> Check {
    let missing: Vec<String> = models
        .iter()
        .filter(|(_, path)| !path.is_file())
        .map(|(name, path)| format!("{name} ({})", path.display()))
        .collect();
    match missing.is_empty() {
        true => pass("models", "the voice and speaker models are downloaded"),
        false => fail(
            "models",
            format!("missing: {}", missing.join(", ")),
            "run eco setup",
        ),
    }
}

fn config_check(path: &Path, config: &Result<Config, ConfigError>) -> Check {
    match config {
        Ok(_) => pass("config", format!("{} loads", path.display())),
        Err(error) => fail(
            "config",
            error.0.clone(),
            format!("correct what it names in {}", path.display()),
        ),
    }
}

/// Where keys can come from, by name.
struct Sources {
    /// The variables set, here or where the daemon runs.
    env: Vec<String>,
    /// omapass's accounts, or why there are none.
    omapass: Result<Vec<String>, String>,
    /// The Credential Manager's `eco:<name>` names; `None` where there is none.
    credentials: Option<Result<Vec<String>, String>>,
}

/// The sources `config`'s models name: each variable set in this shell or
/// listed by the daemon at `address`; omapass and the Credential Manager
/// only asked when a model names them.
async fn sources(config: &Config, address: &Address) -> Sources {
    let keys: Vec<Key> = config.models.iter().map(ModelConfig::key).collect();
    let listed = cli::key_names(address).await.ok().flatten();
    let mut env: Vec<String> = listed
        .as_ref()
        .and_then(|listed| listed["env"].as_array())
        .into_iter()
        .flatten()
        .filter_map(|name| name.as_str().map(String::from))
        .collect();
    for key in &keys {
        if let Key::Env(name) = key
            && std::env::var_os(name).is_some_and(|value| !value.is_empty())
        {
            env.push((*name).into());
        }
    }
    let omapass = match keys.iter().any(|key| matches!(key, Key::Omapass(_))) {
        true => omapass::accounts()
            .await
            .map(|accounts| accounts.into_iter().map(|a| a.account).collect()),
        false => Ok(Vec::new()),
    };
    let credentials = match keys.iter().any(|key| matches!(key, Key::Credential(_))) {
        true => credentials::names().map(|names| names.map_err(|error| error.to_string())),
        false => Some(Ok(Vec::new())),
    };
    Sources {
        env,
        omapass,
        credentials,
    }
}

/// Whether the key `model` names can be had, by its name alone.
fn key(model: &ModelConfig, sources: &Sources) -> Check {
    let check = format!("key:{}", model.name);
    let has = |names: &[String], name: &str| names.iter().any(|found| found == name);
    let another = format!("or choose another key for {} in eco's settings", model.name);
    match model.key() {
        Key::None => pass(&check, "needs no key"),
        Key::Env(name) if has(&sources.env, name) => pass(&check, format!("{name} is set")),
        Key::Env(name) => fail(
            &check,
            format!("{name} is not set"),
            format!(
                "set {name} where eco runs (docs/how-to-register-models.md), then run eco restart"
            ),
        ),
        Key::Omapass(account) => match &sources.omapass {
            Ok(accounts) if has(accounts, account) => {
                pass(&check, format!("omapass has the account {account}"))
            }
            Ok(_) => fail(
                &check,
                format!("omapass has no account {account}"),
                format!("add {account} to omapass, {another}"),
            ),
            Err(why) => fail(&check, why.clone(), format!("install omapass, {another}")),
        },
        Key::Credential(name) => match &sources.credentials {
            Some(Ok(names)) if has(names, name) => pass(
                &check,
                format!("the Windows Credential Manager has eco:{name}"),
            ),
            Some(Ok(_)) => fail(
                &check,
                format!("the Windows Credential Manager has no eco:{name}"),
                format!("run eco key set {name}"),
            ),
            Some(Err(why)) => fail(&check, why.clone(), "run eco from the desktop"),
            None => fail(
                &check,
                "this system has no Windows Credential Manager",
                another,
            ),
        },
    }
}

/// Whether the daemon runs, and is this eco.
fn daemon(greeting: &Result<Option<Vec<Value>>, Failure>) -> Check {
    let events = match greeting {
        Ok(Some(events)) => events,
        Ok(None) => return fail("daemon", "not running", "run eco start"),
        Err(failure) => return fail("daemon", failure.message(), "run eco restart"),
    };
    let version = events
        .iter()
        .find(|event| event["type"] == "daemon")
        .and_then(|event| event["version"].as_str())
        .unwrap_or("unknown");
    match version == CURRENT {
        true => pass("daemon", format!("running, version {version}")),
        false => fail(
            "daemon",
            format!("running version {version}, not this eco's {CURRENT}"),
            "run eco restart",
        ),
    }
}

/// The global shortcuts: on Linux, eco's rules loaded by the Hyprland config.
#[cfg(unix)]
fn shortcuts(_: &Result<Option<Vec<Value>>, Failure>, places: &Places) -> Check {
    hyprland(&places.bindings)
}

/// Whether the Hyprland config at `bindings` loads eco's rules.
#[cfg(unix)]
fn hyprland(bindings: &Path) -> Check {
    let shown = bindings.display();
    match std::fs::read_to_string(bindings) {
        Ok(text) if crate::adapters::desktop_hyprland::loads_rules(&text) => {
            pass("shortcuts", format!("{shown} loads eco's rules"))
        }
        Ok(_) => fail(
            "shortcuts",
            format!("{shown} does not load eco's rules"),
            "run eco setup, then hyprctl reload",
        ),
        Err(error) => fail(
            "shortcuts",
            format!("cannot read {shown}: {error}"),
            "run eco setup",
        ),
    }
}

/// The global shortcuts: on Windows, those the daemon that greeted with
/// `greeting` registered; each another program holds is named as off, which
/// is no failure: the shortcuts are optional.
#[cfg(windows)]
fn shortcuts(greeting: &Result<Option<Vec<Value>>, Failure>, _: &Places) -> Check {
    let Ok(Some(events)) = greeting else {
        return fail(
            "shortcuts",
            "the daemon registers them while it runs, and it does not",
            "run eco start",
        );
    };
    let unavailable: Vec<&str> = events
        .iter()
        .filter(|event| event["type"] == "shortcuts")
        .filter_map(|event| event["unavailable"].as_array())
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    match unavailable.is_empty() {
        true => pass("shortcuts", "registered"),
        false => pass(
            "shortcuts",
            format!(
                "registered; taken by another program, so off: {}",
                unavailable.join(", ")
            ),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model(toml: &str) -> ModelConfig {
        toml::from_str(&format!(
            "name = \"m\"\ntype = \"chat\"\nbase_url = \"https://x/v1\"\nmodel = \"x\"\n{toml}"
        ))
        .unwrap()
    }

    fn sources() -> Sources {
        Sources {
            env: vec!["OPENROUTER_API_KEY".into()],
            omapass: Ok(vec!["openrouter".into()]),
            credentials: Some(Ok(vec!["deepgram".into()])),
        }
    }

    /// Places under `dir`: the overlay shipped, the VAD model and the config
    /// only once a test writes them.
    fn places(dir: &Path) -> Places {
        Places {
            exe: dir.join("eco"),
            shipped: vec![("share/eco/overlay/shell.qml".into(), true)],
            models: vec![("Silero VAD", dir.join("vad.onnx"))],
            config: dir.join("config.toml"),
            #[cfg(unix)]
            bindings: dir.join("bindings.lua"),
        }
    }

    const CONFIG: &str = r#"
[stt]
model = "lan"

[llm]
model = "chat"

[[models]]
name = "lan"
type = "transcription"
base_url = "http://lan:8081/v1"
model = "whisper-1"

[[models]]
name = "chat"
type = "chat"
base_url = "https://openrouter.ai/api/v1"
model = "x"
api_key_env = "ECO_DOCTOR_NEVER_SET"
"#;

    fn names(checks: &[Check]) -> Vec<(&str, bool)> {
        checks
            .iter()
            .map(|check| (check.check.as_str(), check.ok))
            .collect()
    }

    #[tokio::test]
    async fn every_check_runs_and_a_key_is_checked_once_the_config_loads() {
        let directory = tempfile::tempdir().unwrap();
        let places = places(directory.path());
        let nowhere = Address::scratch(directory.path());
        std::fs::write(&places.config, "[nothing_eco_knows]\n").unwrap();
        let broken = checks(&nowhere, &places).await;
        assert_eq!(
            names(&broken),
            [
                ("installed", true),
                ("models", false),
                ("config", false),
                ("daemon", false),
                ("shortcuts", false)
            ]
        );
        std::fs::write(&places.config, CONFIG).unwrap();
        std::fs::write(directory.path().join("vad.onnx"), b"onnx").unwrap();
        let loaded = checks(&nowhere, &places).await;
        assert_eq!(
            names(&loaded),
            [
                ("installed", true),
                ("models", true),
                ("config", true),
                ("key:lan", true),
                ("key:chat", false),
                ("daemon", false),
                ("shortcuts", false)
            ]
        );
        assert_eq!(loaded[4].detail, "ECO_DOCTOR_NEVER_SET is not set");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_credential_key_fails_where_there_is_no_credential_manager() {
        let directory = tempfile::tempdir().unwrap();
        let places = places(directory.path());
        let credential = CONFIG.replace(
            "api_key_env = \"ECO_DOCTOR_NEVER_SET\"",
            "api_key_credential = \"openrouter\"",
        );
        std::fs::write(&places.config, credential).unwrap();
        let checks = checks(&Address::scratch(directory.path()), &places).await;
        let key = checks
            .iter()
            .find(|check| check.check == "key:chat")
            .unwrap();
        assert_eq!(key.detail, "this system has no Windows Credential Manager");
    }

    #[test]
    fn doctor_fails_when_any_check_fails() {
        let good = [pass("config", "loads"), pass("daemon", "running")];
        let (said, code) = report(&good);
        assert_eq!(code, 0);
        assert_eq!(
            said,
            json!({"ok": true, "data": [
                {"check": "config", "ok": true, "detail": "loads", "fix": ""},
                {"check": "daemon", "ok": true, "detail": "running", "fix": ""}]})
        );
        let bad = [
            pass("config", "loads"),
            fail("daemon", "not running", "run eco start"),
        ];
        let (said, code) = report(&bad);
        assert_eq!(code, 1);
        assert_eq!(said["ok"], false);
        assert_eq!(said["data"][1]["fix"], "run eco start");
    }

    #[test]
    fn a_key_is_found_by_the_name_of_its_source() {
        let sources = sources();
        for found in [
            "",
            "api_key_env = \"OPENROUTER_API_KEY\"",
            "api_key_omapass = \"openrouter\"",
            "api_key_credential = \"deepgram\"",
        ] {
            let check = key(&model(found), &sources);
            assert!(check.ok, "{found}: {check:?}");
            assert_eq!(check.check, "key:m");
        }
        let missing = key(&model("api_key_env = \"GROQ_API_KEY\""), &sources);
        assert_eq!(
            missing,
            fail(
                "key:m",
                "GROQ_API_KEY is not set",
                "set GROQ_API_KEY where eco runs (docs/how-to-register-models.md), then run eco restart"
            )
        );
        let account = key(&model("api_key_omapass = \"groq\""), &sources);
        assert_eq!(account.detail, "omapass has no account groq");
        let credential = key(&model("api_key_credential = \"groq\""), &sources);
        assert_eq!(credential.fix, "run eco key set groq");
    }

    #[test]
    fn a_key_source_this_system_lacks_fails_with_why() {
        let lacking = Sources {
            env: Vec::new(),
            omapass: Err("omapass is not installed".into()),
            credentials: None,
        };
        let account = key(&model("api_key_omapass = \"openrouter\""), &lacking);
        assert_eq!(
            (account.ok, account.detail.as_str()),
            (false, "omapass is not installed")
        );
        let credential = key(&model("api_key_credential = \"deepgram\""), &lacking);
        assert!(!credential.ok);
        assert_eq!(
            credential.detail,
            "this system has no Windows Credential Manager"
        );
    }

    #[test]
    fn the_daemon_must_run_and_be_this_eco() {
        let this = Ok(Some(vec![json!({"type": "daemon", "version": CURRENT})]));
        assert!(daemon(&this).ok);
        let old = Ok(Some(vec![json!({"type": "daemon", "version": "0.0.1"})]));
        assert_eq!(daemon(&old).fix, "run eco restart");
        assert_eq!(
            daemon(&Ok(None)),
            fail("daemon", "not running", "run eco start")
        );
        let denied = Err(Failure::new("daemon.access_denied", "access denied"));
        assert_eq!(daemon(&denied).detail, "access denied");
    }

    #[test]
    fn what_is_installed_and_downloaded_is_named_when_missing() {
        let exe = Path::new("/usr/bin/eco");
        assert!(installed(exe, &[("share/eco/overlay/shell.qml".into(), true)]).ok);
        let missing = installed(exe, &[("lib/eco/eco-window".into(), false)]);
        assert_eq!(
            missing.detail,
            "/usr/bin/eco runs without lib/eco/eco-window beside it"
        );
        let directory = tempfile::tempdir().unwrap();
        let vad = directory.path().join("vad.onnx");
        let speaker = directory.path().join("speaker.onnx");
        std::fs::write(&vad, b"onnx").unwrap();
        assert!(models(&[("Silero VAD", vad.clone())]).ok);
        let missing = models(&[("Silero VAD", vad), ("WeSpeaker CAM++", speaker.clone())]);
        assert_eq!(
            missing,
            fail(
                "models",
                format!("missing: WeSpeaker CAM++ ({})", speaker.display()),
                "run eco setup"
            )
        );
    }

    #[test]
    fn a_config_that_does_not_load_is_named() {
        let path = Path::new("/home/me/.config/eco/config.toml");
        let broken = Err(ConfigError("invalid config: unknown field `x`".into()));
        assert_eq!(
            config_check(path, &broken),
            fail(
                "config",
                "invalid config: unknown field `x`",
                "correct what it names in /home/me/.config/eco/config.toml"
            )
        );
    }

    #[cfg(unix)]
    #[test]
    fn on_linux_the_shortcuts_are_the_rules_hyprland_loads() {
        let directory = tempfile::tempdir().unwrap();
        let bindings = directory.path().join("bindings.lua");
        assert!(!hyprland(&bindings).ok);
        std::fs::write(&bindings, "o.bind(\"SUPER + Q\", \"close\")\n").unwrap();
        assert_eq!(
            hyprland(&bindings).fix,
            "run eco setup, then hyprctl reload"
        );
        std::fs::write(&bindings, "dofile(\"/usr/share/eco/hypr/eco.lua\")\n").unwrap();
        assert!(hyprland(&bindings).ok);
    }

    #[cfg(windows)]
    #[test]
    fn on_windows_the_shortcuts_are_those_the_daemon_registered() {
        let directory = tempfile::tempdir().unwrap();
        let path = &places(directory.path());
        assert_eq!(shortcuts(&Ok(None), path).fix, "run eco start");
        let registered = Ok(Some(vec![json!({"type": "daemon"})]));
        assert_eq!(shortcuts(&registered, path).detail, "registered");
        let taken = Ok(Some(vec![
            json!({"type": "shortcuts", "unavailable": ["Win+Alt+A", "Win+Alt+Q"]}),
        ]));
        let check = shortcuts(&taken, path);
        assert!(check.ok);
        assert_eq!(
            check.detail,
            "registered; taken by another program, so off: Win+Alt+A, Win+Alt+Q"
        );
    }
}
