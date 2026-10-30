//! Windows: notifications as toasts, through the WinRT toast notifier of the
//! `windows` crate's safe projection, under eco's own app id: a key in the
//! user's registry names it "eco" with eco's icon. Each toast registers it
//! first; a toast that cannot goes out under Windows PowerShell's app id
//! (docs/design.md §15).

use std::path::Path;
use std::sync::{Mutex, PoisonError};
use std::thread::JoinHandle;

use windows::Data::Xml::Dom::XmlDocument;
use windows::UI::Notifications::{ToastNotification, ToastNotificationManager};
use windows::core::HSTRING;
use windows_registry::CURRENT_USER;

use crate::paths;
use crate::ports::{Notifier, Urgency};

/// eco's app id, the name Windows files its toasts under.
pub const APP_ID: &str = "This-Is-NPC.eco";

/// Windows PowerShell's app id, which Windows always has registered.
const POWERSHELL: &str =
    "{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\\WindowsPowerShell\\v1.0\\powershell.exe";

/// The key, under the user's hive, that registers the app id `id`.
fn key(id: &str) -> String {
    format!(r"Software\Classes\AppUserModelId\{id}")
}

/// The icon eco ships for its toasts, when this eco is installed.
pub fn icon() -> Option<std::path::PathBuf> {
    paths::shipped("eco.ico")
}

/// The values the key of an app id holds: the name its toasts show, and the
/// icon at `icon` when there is one.
fn values(icon: Option<&Path>) -> Vec<(&'static str, String)> {
    let mut values = vec![("DisplayName", "eco".to_owned())];
    if let Some(icon) = icon {
        values.push(("IconUri", icon.display().to_string()));
    }
    values
}

/// What registering an app id did.
#[derive(Debug, PartialEq, Eq)]
pub enum Registered {
    Added,
    Updated,
    Current,
}

/// Register the app id `id` for the user, named "eco" with the icon at
/// `icon`; without one, an icon already registered stays. Writes nothing
/// when the key already holds these values. `Err` says why it could not.
pub fn register(id: &str, icon: Option<&Path>) -> Result<Registered, String> {
    write(id, icon).map_err(|error| {
        format!(
            "could not register eco's notifications, so they show as Windows PowerShell's: {error}"
        )
    })
}

/// [`register`], with the registry's error.
fn write(id: &str, icon: Option<&Path>) -> windows_registry::Result<Registered> {
    let path = key(id);
    let found = CURRENT_USER.open(&path).ok();
    let wanted = values(icon);
    let current = found.as_ref().is_some_and(|key| {
        wanted
            .iter()
            .all(|(name, value)| key.get_string(name).is_ok_and(|held| held == *value))
    });
    if current {
        return Ok(Registered::Current);
    }
    let key = CURRENT_USER.create(&path)?;
    for (name, value) in &wanted {
        key.set_string(name, value)?;
    }
    Ok(if found.is_some() {
        Registered::Updated
    } else {
        Registered::Added
    })
}

/// Toasts shown on threads of their own; dropping it waits for them, so that
/// a program that exits right after a toast still shows it.
pub struct Toasts {
    shown: Mutex<Vec<JoinHandle<()>>>,
    /// Whether each toast registers eco's app id first, which writes it under
    /// the user's hive; off once eco is removed, which leaves nothing behind.
    register: bool,
}

impl Default for Toasts {
    fn default() -> Self {
        Self {
            shown: Mutex::default(),
            register: true,
        }
    }
}

impl Toasts {
    /// Toasts that never register eco's app id, shown under PowerShell's:
    /// those of the step that removes eco.
    pub fn unregistered() -> Self {
        Self {
            register: false,
            ..Self::default()
        }
    }
}

/// Remove the registration of the app id `id` from the user's hive, if any.
pub fn unregister(id: &str) {
    let _ = CURRENT_USER.remove_tree(key(id));
}

impl Drop for Toasts {
    fn drop(&mut self) {
        let shown = self.shown.get_mut().unwrap_or_else(PoisonError::into_inner);
        for thread in shown.drain(..) {
            let _ = thread.join();
        }
    }
}

/// `text` safe inside an XML element or attribute.
fn escape(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&apos;"),
            c => escaped.push(c),
        }
    }
    escaped
}

/// The toast's XML: `title` and `body`; a critical one stays on screen longer.
fn content(title: &str, body: &str, urgency: Urgency) -> String {
    let duration = match urgency {
        Urgency::Normal => "short",
        Urgency::Critical => "long",
    };
    format!(
        "<toast duration=\"{duration}\"><visual><binding template=\"ToastGeneric\">\
         <text>{}</text><text>{}</text></binding></visual></toast>",
        escape(title),
        escape(body)
    )
}

/// The app id to show a toast under: eco's, registered now; PowerShell's,
/// with why, when eco's cannot be.
fn app_id() -> (&'static str, Option<String>) {
    match register(APP_ID, icon().as_deref()) {
        Ok(_) => (APP_ID, None),
        Err(why) => (POWERSHELL, Some(why)),
    }
}

/// Show the toast `xml` under the app id `id`.
fn show(id: &str, xml: &str) -> windows::core::Result<()> {
    let document = XmlDocument::new()?;
    document.LoadXml(&HSTRING::from(xml))?;
    let toast = ToastNotification::CreateToastNotification(&document)?;
    ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(id))?.Show(&toast)
}

impl Notifier for Toasts {
    fn notify(&self, title: &str, body: &str, urgency: Urgency) {
        let xml = content(title, body, urgency);
        let register = self.register;
        // On a thread of its own: no caller waits on Windows.
        let thread = std::thread::spawn(move || {
            let (id, why) = match register {
                true => app_id(),
                false => (POWERSHELL, None),
            };
            if let Some(why) = why {
                eprintln!("eco: {why}");
            }
            if let Err(error) = show(id, &xml) {
                eprintln!("eco: the notification could not show: {error}");
            }
        });
        let mut shown = self.shown.lock().unwrap_or_else(PoisonError::into_inner);
        shown.retain(|thread| !thread.is_finished());
        shown.push(thread);
    }
}

/// A test's own app id, registered by the test alone.
#[cfg(test)]
pub mod testing {
    use super::{CURRENT_USER, key};

    /// The app id `id`, unregistered now and again when dropped, also when
    /// the test fails.
    pub struct TestId(pub &'static str);

    impl TestId {
        pub fn new(id: &'static str) -> Self {
            let _ = CURRENT_USER.remove_tree(key(id));
            Self(id)
        }
    }

    impl Drop for TestId {
        fn drop(&mut self) {
            let _ = CURRENT_USER.remove_tree(key(self.0));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::testing::TestId;
    use super::*;

    #[test]
    fn unregistering_removes_the_app_id_and_a_second_time_is_quiet() {
        let id = TestId::new("This-Is-NPC.eco-unregister-test");
        register(id.0, None).unwrap();
        assert!(CURRENT_USER.open(key(id.0)).is_ok());
        unregister(id.0);
        assert!(CURRENT_USER.open(key(id.0)).is_err());
        unregister(id.0);
    }

    /// The strings the key of the app id `id` holds, by name.
    fn held(id: &str) -> Vec<(String, String)> {
        let key = CURRENT_USER.open(key(id)).unwrap();
        let mut held: Vec<_> = key
            .values()
            .unwrap()
            .map(|(name, _)| {
                let value = key.get_string(&name).unwrap();
                (name, value)
            })
            .collect();
        held.sort();
        held
    }

    #[test]
    fn eco_s_app_id_is_named_eco_with_its_icon() {
        assert_eq!(
            key(APP_ID),
            r"Software\Classes\AppUserModelId\This-Is-NPC.eco"
        );
        assert_eq!(
            values(Some(Path::new(r"C:\eco\share\eco\eco.ico"))),
            [
                ("DisplayName", "eco".to_owned()),
                ("IconUri", r"C:\eco\share\eco\eco.ico".to_owned())
            ]
        );
        assert_eq!(values(None), [("DisplayName", "eco".to_owned())]);
    }

    #[test]
    fn registers_the_app_id_and_a_rerun_changes_nothing() {
        let test = TestId::new("This-Is-NPC.eco-test-register");
        let id = test.0;
        let icon = Path::new(r"C:\eco\share\eco\eco.ico");
        assert_eq!(register(id, Some(icon)), Ok(Registered::Added));
        let expected = [
            ("DisplayName".to_owned(), "eco".to_owned()),
            ("IconUri".to_owned(), icon.display().to_string()),
        ];
        assert_eq!(held(id), expected);
        assert_eq!(register(id, Some(icon)), Ok(Registered::Current));
        // Without an icon, as from a build that is not installed, the one
        // registered stays.
        assert_eq!(register(id, None), Ok(Registered::Current));
        assert_eq!(held(id), expected);
        let moved = Path::new(r"D:\eco\share\eco\eco.ico");
        assert_eq!(register(id, Some(moved)), Ok(Registered::Updated));
        assert_eq!(held(id)[1].1, moved.display().to_string());
    }

    #[test]
    fn a_toast_carries_its_title_and_body_escaped_and_its_urgency_as_duration() {
        assert_eq!(
            content("Eles", "a < b & \"c\"", Urgency::Critical),
            "<toast duration=\"long\"><visual><binding template=\"ToastGeneric\">\
             <text>Eles</text><text>a &lt; b &amp; &quot;c&quot;</text></binding></visual></toast>"
        );
        assert!(content("eco", "ready", Urgency::Normal).starts_with("<toast duration=\"short\">"));
    }

    #[test]
    fn a_toast_s_xml_is_one_windows_reads() {
        let document = XmlDocument::new().unwrap();
        document
            .LoadXml(&HSTRING::from(content(
                "eco",
                "it's <fine>",
                Urgency::Normal,
            )))
            .unwrap();
        ToastNotification::CreateToastNotification(&document).unwrap();
    }
}
