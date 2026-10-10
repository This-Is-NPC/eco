//! The Windows Credential Manager: a provider's key kept as the generic
//! credential `eco:<name>`, read when the provider is built, stored and removed
//! by `eco key`. Other systems have none; there keys come from omapass or the
//! environment.

/// Whether this system has the Credential Manager, so the window offers it.
pub const AVAILABLE: bool = cfg!(windows);

/// The credential that holds the key `name`: its "Internet or network address".
#[cfg(any(windows, test))]
pub fn target(name: &str) -> String {
    format!("eco:{name}")
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum CredentialError {
    /// No credential `eco:<name>`, or one without a key.
    #[cfg_attr(unix, expect(dead_code, reason = "only the Windows store finds none"))]
    #[error(
        "no key in the Windows Credential Manager under eco:{0}; run `eco key set {0}`, or add the generic credential eco:{0} with the key as its password"
    )]
    Missing(String),
    #[cfg_attr(
        windows,
        expect(dead_code, reason = "Windows has the Credential Manager")
    )]
    #[error(
        "the Windows Credential Manager is only on Windows; on Linux keep the key in omapass (api_key_omapass) or an environment variable (api_key_env)"
    )]
    Unavailable,
    #[cfg_attr(unix, expect(dead_code, reason = "only the Windows store names one"))]
    #[error("a key name is a word with no spaces, like deepgram")]
    BadName,
    #[cfg_attr(unix, expect(dead_code, reason = "only Windows reads a key"))]
    #[error("no key given")]
    NoKey,
    /// The logon session has no Credential Manager: an ssh login by key has none.
    #[cfg_attr(unix, expect(dead_code, reason = "only Windows has logon sessions"))]
    #[error(
        "the Windows Credential Manager cannot be reached from this logon session ({0}); an ssh login by key has none, so run eco from the desktop"
    )]
    NoStore(String),
    #[cfg_attr(unix, expect(dead_code, reason = "only the Windows store fails"))]
    #[error("Windows Credential Manager: {0}")]
    Failed(String),
}

/// Whether `name` can name a credential: not empty, no blanks or control characters.
pub fn valid_name(name: &str) -> bool {
    !name.is_empty() && !name.chars().any(|c| c.is_whitespace() || c.is_control())
}

/// The key `eco:<name>` holds.
pub fn read(name: &str) -> Result<String, CredentialError> {
    platform::read(name)
}

/// `eco key set <name>`: read the key, hidden, from the terminal, or from
/// stdin when it is not one, and keep it as `eco:<name>`; what was done.
pub fn set(name: &str) -> Result<String, CredentialError> {
    platform::set(name)
}

/// `eco key remove <name>`: delete `eco:<name>`; what was done.
pub fn remove(name: &str) -> Result<String, CredentialError> {
    platform::remove(name)
}

#[cfg(unix)]
mod platform {
    use super::CredentialError;

    pub fn read(_name: &str) -> Result<String, CredentialError> {
        Err(CredentialError::Unavailable)
    }

    pub fn set(_name: &str) -> Result<String, CredentialError> {
        Err(CredentialError::Unavailable)
    }

    pub fn remove(_name: &str) -> Result<String, CredentialError> {
        Err(CredentialError::Unavailable)
    }
}

#[cfg(windows)]
mod platform {
    use std::collections::HashMap;
    use std::io::{IsTerminal, Read};

    use keyring_core::api::CredentialStoreApi;
    use keyring_core::{Entry, Error};
    use windows_native_keyring_store::Store;

    use super::{CredentialError, target, valid_name};

    /// The generic credential `eco:<name>`, kept on this computer only.
    fn entry(name: &str) -> Result<Entry, CredentialError> {
        if !valid_name(name) {
            return Err(CredentialError::BadName);
        }
        let target = target(name);
        let modifiers = HashMap::from([("target", target.as_str()), ("persistence", "Local")]);
        Store::new()
            .and_then(|store| store.build("eco", name, Some(&modifiers)))
            .map_err(|error| CredentialError::Failed(error.to_string()))
    }

    /// `error` as eco says it: a credential that is not there is `Missing`, a
    /// store this logon session cannot reach is `NoStore`.
    fn failed(name: &str, error: Error) -> CredentialError {
        match error {
            Error::NoEntry => CredentialError::Missing(name.into()),
            Error::NoStorageAccess(reason) => CredentialError::NoStore(reason.to_string()),
            other => CredentialError::Failed(other.to_string()),
        }
    }

    pub fn read(name: &str) -> Result<String, CredentialError> {
        let key = entry(name)?
            .get_password()
            .map_err(|error| failed(name, error))?;
        let key = key.trim();
        if key.is_empty() {
            return Err(CredentialError::Missing(name.into()));
        }
        Ok(key.into())
    }

    pub fn set(name: &str) -> Result<String, CredentialError> {
        let entry = entry(name)?;
        let stdin = std::io::stdin();
        let key = read_key(stdin.is_terminal(), stdin.lock(), || {
            rpassword::prompt_password(format!("Key for {} (hidden): ", target(name)))
        })?;
        store(&entry, name, &key)
    }

    /// Keep `key` in `entry`, the credential of `name`.
    fn store(entry: &Entry, name: &str, key: &str) -> Result<String, CredentialError> {
        entry
            .set_password(key)
            .map_err(|error| failed(name, error))?;
        Ok(format!(
            "the key is in the Windows Credential Manager as {}",
            target(name)
        ))
    }

    /// The key typed at the terminal through `prompt`, which hides it, or else
    /// read from `input` to its end; blanks around it are dropped.
    pub(super) fn read_key(
        terminal: bool,
        mut input: impl Read,
        prompt: impl FnOnce() -> std::io::Result<String>,
    ) -> Result<String, CredentialError> {
        let read = |error: std::io::Error| CredentialError::Failed(error.to_string());
        let key = if terminal {
            prompt().map_err(read)?
        } else {
            let mut key = String::new();
            input.read_to_string(&mut key).map_err(read)?;
            key
        };
        let key = key.trim();
        if key.is_empty() {
            return Err(CredentialError::NoKey);
        }
        Ok(key.into())
    }

    pub fn remove(name: &str) -> Result<String, CredentialError> {
        entry(name)?
            .delete_credential()
            .map_err(|error| failed(name, error))?;
        Ok(format!(
            "removed {} from the Windows Credential Manager",
            target(name)
        ))
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// A credential name no user has: this test's own, removed after.
        fn test_name(what: &str) -> String {
            format!("test-{what}-{}", uuid::Uuid::new_v4().simple())
        }

        #[test]
        fn a_store_this_session_cannot_reach_is_said_so() {
            let error = keyring_core::Error::NoStorageAccess("1312".into());
            assert_eq!(failed("x", error), CredentialError::NoStore("1312".into()));
            let said = CredentialError::NoStore("1312".into()).to_string();
            assert!(said.contains("run eco from the desktop"), "{said}");
            assert_eq!(
                failed("x", Error::NoEntry),
                CredentialError::Missing("x".into())
            );
        }

        #[test]
        fn a_key_is_stored_read_and_removed_under_eco_name() {
            if super::super::testing::no_store() {
                return;
            }
            let name = test_name("roundtrip");
            assert_eq!(read(&name), Err(CredentialError::Missing(name.clone())));
            let entry = entry(&name).unwrap();
            assert_eq!(
                store(&entry, &name, "sk-test-123"),
                Ok(format!(
                    "the key is in the Windows Credential Manager as eco:{name}"
                ))
            );
            assert_eq!(read(&name), Ok("sk-test-123".into()));
            assert_eq!(
                remove(&name),
                Ok(format!(
                    "removed eco:{name} from the Windows Credential Manager"
                ))
            );
            assert_eq!(read(&name), Err(CredentialError::Missing(name.clone())));
            assert_eq!(remove(&name), Err(CredentialError::Missing(name)));
        }

        #[test]
        fn a_credential_added_by_hand_is_read_by_its_address() {
            if super::super::testing::no_store() {
                return;
            }
            // The Credential Manager's "Add a generic credential" keeps the
            // password as UTF-16 under the address typed, with any user name.
            let name = test_name("by-hand");
            let address = format!("eco:{name}");
            let modifiers = HashMap::from([("target", address.as_str())]);
            let by_hand = Store::new()
                .and_then(|store| store.build("someone", "me@example.com", Some(&modifiers)))
                .unwrap();
            by_hand.set_password(" dg-key \r\n").unwrap();
            let found = read(&name);
            by_hand.delete_credential().unwrap();
            assert_eq!(found, Ok("dg-key".into()));
        }

        #[test]
        fn a_blank_credential_is_missing_and_a_bad_name_is_refused() {
            assert_eq!(read("two words"), Err(CredentialError::BadName));
            assert_eq!(remove(""), Err(CredentialError::BadName));
            if super::super::testing::no_store() {
                return;
            }
            let name = test_name("blank");
            let entry = entry(&name).unwrap();
            entry.set_password("  ").unwrap();
            let found = read(&name);
            entry.delete_credential().unwrap();
            assert_eq!(found, Err(CredentialError::Missing(name)));
        }

        #[test]
        fn the_key_comes_from_stdin_when_it_is_not_a_terminal() {
            let never = || -> std::io::Result<String> { panic!("prompted") };
            let piped = read_key(false, "sk-piped\r\n".as_bytes(), never);
            assert_eq!(piped, Ok("sk-piped".into()));
            let typed = read_key(true, "unread".as_bytes(), || Ok("sk-typed".into()));
            assert_eq!(typed, Ok("sk-typed".into()));
            assert_eq!(
                read_key(false, " \n".as_bytes(), never),
                Err(CredentialError::NoKey)
            );
        }
    }
}

/// For the tests that need the real store.
#[cfg(all(test, windows))]
pub mod testing {
    use super::{CredentialError, read};

    /// Whether this logon session has no Credential Manager — an ssh login by
    /// key has none —, said on stderr: the tests that need it then stop there,
    /// and run from a desktop terminal.
    pub fn no_store() -> bool {
        let none = matches!(read("test-probe"), Err(CredentialError::NoStore(_)));
        if none {
            eprintln!("skipped: no Windows Credential Manager in this logon session");
        }
        none
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_name_is_one_word_and_its_credential_is_eco_name() {
        assert_eq!(target("deepgram"), "eco:deepgram");
        assert!(valid_name("open-router_2"));
        for bad in ["", "two words", "tab\there", "line\n"] {
            assert!(!valid_name(bad), "{bad:?}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn on_linux_there_is_no_credential_manager() {
        assert_eq!(read("deepgram"), Err(CredentialError::Unavailable));
        assert_eq!(set("deepgram"), Err(CredentialError::Unavailable));
        assert_eq!(remove("deepgram"), Err(CredentialError::Unavailable));
        let said = CredentialError::Unavailable.to_string();
        assert!(said.contains("omapass"), "{said}");
    }
}
