//! The desktop notifications the daemon sends while no eco window is active:
//! what the windows would have shown that the user should not miss.

use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

use crate::domain::events::Event;
use crate::ports::{Notifier, Urgency};

/// The text of an interface key in the language pack in use, if it has one.
pub type Texts = Arc<dyn Fn(&str) -> Option<String> + Send + Sync>;

/// The title of a notice that names no source: the app's name.
const APP: &str = "eco";

#[derive(Default)]
struct State {
    /// `[ui] notifications`.
    on: bool,
    /// The windows that said they are active.
    active: BTreeSet<u32>,
    /// The newer release last told of, by version.
    told: Option<String>,
    /// The action of each answer streaming, by suggestion id.
    answering: HashMap<String, String>,
}

/// Watches every event the daemon emits and tells `notifier` of those worth a
/// notification, while the setting is on and no eco window is active.
pub struct Notifications {
    notifier: Arc<dyn Notifier>,
    texts: Mutex<Texts>,
    state: Mutex<State>,
}

/// A notification decided on.
struct Notice {
    title: String,
    body: String,
    urgency: Urgency,
}

impl Notifications {
    pub fn new(notifier: Arc<dyn Notifier>, on: bool, texts: Texts) -> Self {
        Self {
            notifier,
            texts: Mutex::new(texts),
            state: Mutex::new(State {
                on,
                ..State::default()
            }),
        }
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().expect("not poisoned")
    }

    /// Follow the config: whether to notify, and the pack the texts come from.
    pub fn configure(&self, on: bool, texts: Texts) {
        self.state().on = on;
        *self.texts.lock().expect("not poisoned") = texts;
    }

    /// The window `number` is active, or no longer.
    pub fn window_active(&self, number: u32, active: bool) {
        let mut state = self.state();
        if active {
            state.active.insert(number);
        } else {
            state.active.remove(&number);
        }
    }

    /// Only the windows `open` are still there: the others' state goes.
    pub fn windows_open(&self, open: &[u32]) {
        self.state().active.retain(|number| open.contains(number));
    }

    /// Hear one event the daemon emits; notify of it when it is one worth it.
    pub fn heard(&self, event: &Event) {
        let mut state = self.state();
        let notice = self.notice(&mut state, event);
        let quiet = !state.on || !state.active.is_empty();
        drop(state);
        if let Some(notice) = notice.filter(|_| !quiet) {
            self.notifier
                .notify(&notice.title, &notice.body, notice.urgency);
        }
    }

    /// The notice `event` makes, if any, noting what later events need.
    fn notice(&self, state: &mut State, event: &Event) -> Option<Notice> {
        let text = |field: &str| event.get(field).and_then(Value::as_str);
        match text("type")? {
            "suggestion_start" => {
                state
                    .answering
                    .insert(text("id")?.into(), text("action")?.into());
                None
            }
            "suggestion_removed" => {
                state.answering.remove(text("id")?);
                None
            }
            "suggestion_end" => {
                let action = state.answering.remove(text("id")?)?;
                Some(self.app(
                    self.t("notify.answer", &json!({"action": action})),
                    Urgency::Normal,
                ))
            }
            "transcription" => {
                let who = text("who")?.to_string();
                let (body, urgency) = match text("state")? {
                    "down" => {
                        let key = format!("error.{}", text("code").unwrap_or("stt.down"));
                        let key = if self.has(&key) {
                            key
                        } else {
                            "error.stt.down".into()
                        };
                        (self.t(&key, &Value::Null), Urgency::Critical)
                    }
                    "back" => {
                        let dropped = event["dropped_s"].as_f64().unwrap_or(0.0);
                        let body = if dropped >= 0.5 {
                            self.t(
                                "transcription.back_dropped",
                                &json!({"n": dropped.round() as u64}),
                            )
                        } else {
                            self.t("transcription.back", &Value::Null)
                        };
                        (body, Urgency::Normal)
                    }
                    _ => return None,
                };
                Some(Notice {
                    title: who,
                    body,
                    urgency,
                })
            }
            "update" if event["newer"] == true => {
                let latest = text("latest")?.to_string();
                if state.told.as_ref() == Some(&latest) {
                    return None;
                }
                state.told = Some(latest.clone());
                Some(self.app(
                    self.t("update.notice", &json!({"version": latest})),
                    Urgency::Normal,
                ))
            }
            // The windows ask instead of showing this one as an error.
            "error" if text("code") == Some("daemon.recording") => None,
            "error" => {
                let key = text("code").map(|code| format!("error.{code}"));
                let body = match key.filter(|key| self.has(key)) {
                    Some(key) => self.t(&key, &event["params"]),
                    None => text("message")?.to_string(),
                };
                Some(self.app(body, Urgency::Critical))
            }
            _ => None,
        }
    }

    /// A notice under the app's name.
    fn app(&self, body: String, urgency: Urgency) -> Notice {
        Notice {
            title: APP.into(),
            body,
            urgency,
        }
    }

    fn lookup(&self, key: &str) -> Option<String> {
        (self.texts.lock().expect("not poisoned"))(key)
    }

    /// Whether the pack has a text for `key`, or for its plural.
    fn has(&self, key: &str) -> bool {
        self.lookup(key).is_some() || self.lookup(&format!("{key}.other")).is_some()
    }

    /// The text of `key` with its `{name}` placeholders filled from `args`; with
    /// an `n` or `count` argument it also tries `key.one` / `key.other`, as the
    /// windows' `I18n.t` does; the key itself when the pack has none.
    fn t(&self, key: &str, args: &Value) -> String {
        let count = args.get("n").or_else(|| args.get("count"));
        let plural = || {
            let count = count?;
            let form = if count.as_u64() == Some(1) {
                "one"
            } else {
                "other"
            };
            self.lookup(&format!("{key}.{form}"))
        };
        let Some(text) = self.lookup(key).or_else(plural) else {
            return key.to_string();
        };
        fill(&text, args)
    }
}

/// `text` with each `{name}` that `args` has replaced by its value.
fn fill(text: &str, args: &Value) -> String {
    let mut filled = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        filled.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let name = after.find('}').map(|close| &after[..close]).filter(|name| {
            !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_')
        });
        match name.and_then(|name| Some((name, args.get(name)?))) {
            Some((name, value)) => {
                match value {
                    Value::String(value) => filled.push_str(value),
                    value => filled.push_str(&value.to_string()),
                }
                rest = &after[name.len() + 1..];
            }
            None => {
                filled.push('{');
                rest = after;
            }
        }
    }
    filled.push_str(rest);
    filled
}

/// A notifier that keeps what it is asked to show.
#[cfg(test)]
pub mod testing {
    use std::sync::Mutex;

    use crate::ports::{Notifier, Urgency};

    /// Every notification shown: title, body, urgency.
    #[derive(Default)]
    pub struct Shown(Mutex<Vec<(String, String, Urgency)>>);

    impl Notifier for Shown {
        fn notify(&self, title: &str, body: &str, urgency: Urgency) {
            let shown = (title.to_string(), body.to_string(), urgency);
            self.0.lock().unwrap().push(shown);
        }
    }

    impl Shown {
        /// What was shown since the last call.
        pub fn taken(&self) -> Vec<(String, String, Urgency)> {
            std::mem::take(&mut *self.0.lock().unwrap())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::testing::Shown;
    use super::*;

    /// A pack with the texts the notices read.
    fn pack() -> Texts {
        let texts: HashMap<&str, &str> = HashMap::from([
            ("notify.answer", "{action} · answer ready"),
            ("update.notice", "eco {version} available"),
            ("error.stt.down", "Transcription down · reconnecting"),
            ("error.capture.down", "Device not heard · opening it again"),
            ("transcription.back", "Transcription back"),
            (
                "transcription.back_dropped",
                "Transcription back · {n} s of audio lost",
            ),
            ("error.hook.failed", "{action}: the hook failed — {detail}"),
            ("error.items.one", "{count} item failed"),
            ("error.items.other", "{count} items failed"),
        ]);
        Arc::new(move |key| texts.get(key).map(|text| text.to_string()))
    }

    fn notifications(on: bool) -> (Notifications, Arc<Shown>) {
        let shown = Arc::new(Shown::default());
        let notifier: Arc<dyn Notifier> = shown.clone();
        (Notifications::new(notifier, on, pack()), shown)
    }

    fn eco(body: &str, urgency: Urgency) -> (String, String, Urgency) {
        ("eco".into(), body.into(), urgency)
    }

    fn failed() -> Value {
        json!({"type": "error", "code": "hook.failed", "params": {"action": "resumo", "detail": "exit 1"}, "message": "resumo: exit 1"})
    }

    #[test]
    fn nothing_is_notified_while_the_setting_is_off() {
        let (notifications, shown) = notifications(false);
        notifications.heard(&failed());
        assert!(shown.taken().is_empty());
        notifications.configure(true, pack());
        notifications.heard(&failed());
        assert_eq!(shown.taken().len(), 1);
        notifications.configure(false, pack());
        notifications.heard(&failed());
        assert!(shown.taken().is_empty());
    }

    #[test]
    fn nothing_is_notified_while_a_window_is_active() {
        let (notifications, shown) = notifications(true);
        notifications.window_active(1, true);
        notifications.window_active(2, true);
        notifications.heard(&failed());
        notifications.window_active(1, false);
        notifications.heard(&failed());
        assert!(shown.taken().is_empty());
        notifications.window_active(2, false);
        notifications.heard(&failed());
        assert_eq!(shown.taken().len(), 1);
    }

    #[test]
    fn a_window_gone_no_longer_counts_as_active() {
        let (notifications, shown) = notifications(true);
        notifications.window_active(1, true);
        notifications.window_active(3, true);
        notifications.windows_open(&[1, 2]);
        notifications.heard(&failed());
        assert!(shown.taken().is_empty());
        notifications.windows_open(&[2]);
        notifications.heard(&failed());
        assert_eq!(shown.taken().len(), 1);
    }

    #[test]
    fn an_error_reads_its_code_s_text_or_its_message_and_is_critical() {
        let (notifications, shown) = notifications(true);
        notifications.heard(&failed());
        let unknown = json!({"type": "error", "code": "new.thing", "params": {}, "message": "something broke"});
        notifications.heard(&unknown);
        let plural =
            json!({"type": "error", "code": "items", "params": {"count": 3}, "message": "3 items"});
        notifications.heard(&plural);
        let one =
            json!({"type": "error", "code": "items", "params": {"count": 1}, "message": "1 item"});
        notifications.heard(&one);
        assert_eq!(
            shown.taken(),
            [
                eco("resumo: the hook failed — exit 1", Urgency::Critical),
                eco("something broke", Urgency::Critical),
                eco("3 items failed", Urgency::Critical),
                eco("1 item failed", Urgency::Critical),
            ]
        );
    }

    #[test]
    fn a_restart_refused_is_asked_in_the_window_not_notified() {
        let (notifications, shown) = notifications(true);
        let refused = json!({"type": "error", "code": "daemon.recording", "params": {}, "message": "a session records"});
        notifications.heard(&refused);
        assert!(shown.taken().is_empty());
    }

    #[test]
    fn transcription_down_and_back_are_told_under_the_source_s_name() {
        let (notifications, shown) = notifications(true);
        let events = [
            json!({"type": "transcription", "who": "Eles", "state": "down", "code": "stt.down", "detail": "reset"}),
            json!({"type": "transcription", "who": "Eu", "state": "down", "code": "capture.down", "detail": "gone"}),
            json!({"type": "transcription", "who": "Eu", "state": "down", "code": "other.down", "detail": "?"}),
            json!({"type": "transcription", "who": "Eles", "state": "back", "down_s": 7.5, "dropped_s": 2.6}),
            json!({"type": "transcription", "who": "Eu", "state": "back", "down_s": 1.0, "dropped_s": 0.2}),
        ];
        for event in &events {
            notifications.heard(event);
        }
        let notice = |who: &str, body: &str, urgency| (who.to_string(), body.to_string(), urgency);
        assert_eq!(
            shown.taken(),
            [
                notice(
                    "Eles",
                    "Transcription down · reconnecting",
                    Urgency::Critical
                ),
                notice(
                    "Eu",
                    "Device not heard · opening it again",
                    Urgency::Critical
                ),
                notice("Eu", "Transcription down · reconnecting", Urgency::Critical),
                notice(
                    "Eles",
                    "Transcription back · 3 s of audio lost",
                    Urgency::Normal
                ),
                notice("Eu", "Transcription back", Urgency::Normal),
            ]
        );
    }

    #[test]
    fn a_newer_release_is_told_once_per_version() {
        let (notifications, shown) = notifications(true);
        let update = |latest: &str, newer: bool| json!({"type": "update", "current": "0.4.0", "latest": latest, "newer": newer, "error": ""});
        notifications.heard(&update("0.4.0", false));
        notifications.heard(&update("0.5.0", true));
        notifications.heard(&update("0.5.0", true));
        notifications.heard(&update("0.6.0", true));
        assert_eq!(
            shown.taken(),
            [
                eco("eco 0.5.0 available", Urgency::Normal),
                eco("eco 0.6.0 available", Urgency::Normal),
            ]
        );
    }

    #[test]
    fn a_release_seen_while_a_window_is_active_is_not_told_later() {
        let (notifications, shown) = notifications(true);
        let update = json!({"type": "update", "latest": "0.5.0", "newer": true});
        notifications.window_active(1, true);
        notifications.heard(&update);
        notifications.window_active(1, false);
        notifications.heard(&update);
        assert!(shown.taken().is_empty());
    }

    #[test]
    fn a_finished_answer_names_its_action_never_its_text() {
        let (notifications, shown) = notifications(true);
        let events = [
            json!({"type": "suggestion_start", "id": "a", "session": "s", "action": "resumo", "prompt": "secret prompt"}),
            json!({"type": "suggestion_delta", "id": "a", "text": "the secret answer"}),
            json!({"type": "suggestion_end", "id": "a", "total_ms": 900}),
        ];
        for event in &events {
            notifications.heard(event);
        }
        assert_eq!(
            shown.taken(),
            [eco("resumo · answer ready", Urgency::Normal)]
        );
        // Told once: its id is forgotten.
        notifications.heard(&events[2]);
        assert!(shown.taken().is_empty());
    }

    #[test]
    fn an_answer_removed_before_it_ends_is_not_told() {
        let (notifications, shown) = notifications(true);
        notifications.heard(&json!({"type": "suggestion_start", "id": "a", "action": "ask"}));
        notifications.heard(&json!({"type": "suggestion_removed", "id": "a"}));
        notifications.heard(&json!({"type": "suggestion_end", "id": "a"}));
        assert!(shown.taken().is_empty());
    }

    #[test]
    fn an_answer_started_while_off_is_told_when_it_ends_with_the_setting_on() {
        let (notifications, shown) = notifications(false);
        notifications.heard(&json!({"type": "suggestion_start", "id": "a", "action": "ask"}));
        notifications.configure(true, pack());
        notifications.heard(&json!({"type": "suggestion_end", "id": "a"}));
        assert_eq!(shown.taken(), [eco("ask · answer ready", Urgency::Normal)]);
    }

    #[test]
    fn routine_events_are_never_notified() {
        let (notifications, shown) = notifications(true);
        for event in [
            json!({"type": "transcript", "text": "what was said"}),
            json!({"type": "transcript_partial", "text": "what is said"}),
            json!({"type": "config_saved"}),
            json!({"type": "note", "id": "n", "text": "a note"}),
            json!({"type": "update", "latest": "0.4.0", "newer": false}),
            json!({"type": "transcription", "who": "Eu", "state": "unknown"}),
            json!({"no": "type"}),
        ] {
            notifications.heard(&event);
        }
        assert!(shown.taken().is_empty());
    }

    #[test]
    fn texts_follow_the_pack_configured_and_missing_keys_read_as_themselves() {
        let (notifications, shown) = notifications(true);
        notifications.configure(true, Arc::new(|_| None));
        notifications.heard(&json!({"type": "update", "latest": "0.5.0", "newer": true}));
        assert_eq!(shown.taken(), [eco("update.notice", Urgency::Normal)]);
    }

    #[test]
    fn placeholders_are_filled_from_the_arguments_given() {
        let args = json!({"who": "Eu", "n": 3});
        assert_eq!(fill("{who} lost {n} s", &args), "Eu lost 3 s");
        assert_eq!(fill("{missing} and {who}", &args), "{missing} and Eu");
        assert_eq!(fill("{ who} {} {who", &args), "{ who} {} {who");
        assert_eq!(fill("no placeholder", &args), "no placeholder");
    }
}
