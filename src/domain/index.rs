//! A stored session as the session index keeps it (docs/design.md §8): what
//! the sessions list, people, tags, search and pricing ask of every stored
//! session, so they read a log only when it changed or they need its lines.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::domain::session::{Rate, Session, folded, summary};
use crate::ports::Record;

/// One stored session in the index.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Indexed {
    /// Its summary, as `summarize` makes it; `None` for a log that cannot be
    /// read back.
    pub summary: Option<Value>,
    /// The price each transcription model had when its cost was summed: per
    /// minute, and whether its provider bills each request.
    rates: BTreeMap<String, (Option<f64>, bool)>,
    /// The transcription requests it was billed for.
    pub billed: Vec<String>,
    /// Whether it listened to a request that was never billed.
    pub unclosed: bool,
    /// The models of the requests billed to it whose cost is not kept yet.
    pub unpaid: BTreeSet<String>,
}

impl Indexed {
    /// The entry of the session `records` hold, its cost priced by `rate`.
    pub fn of(records: &[Record], rate: impl Fn(&str) -> Rate) -> Self {
        let Ok(session) = Session::restore(records, Box::new(|_| {})) else {
            return Self {
                summary: None,
                rates: BTreeMap::new(),
                billed: Vec::new(),
                unclosed: false,
                unpaid: BTreeSet::new(),
            };
        };
        let rates = RefCell::new(BTreeMap::new());
        let summary = summary(&session, |model| {
            let priced = rate(model);
            let kept = (priced.per_minute, priced.billed);
            rates.borrow_mut().insert(model.to_string(), kept);
            priced
        });
        Self {
            summary: Some(summary),
            rates: rates.into_inner(),
            billed: session.billed().map(String::from).collect(),
            unclosed: !session.unclosed().is_empty(),
            unpaid: session
                .unpaid()
                .map(|(_, model, _)| model.to_string())
                .collect(),
        }
    }

    /// The entry an index keeps, or `None` when it is not one.
    pub fn read(kept: &Value) -> Option<Self> {
        serde_json::from_value(kept.clone()).ok()
    }

    pub fn json(&self) -> Value {
        serde_json::to_value(self).expect("an entry is JSON")
    }

    /// Whether its cost was summed at the prices `rate` gives now.
    pub fn priced_at(&self, rate: impl Fn(&str) -> Rate) -> bool {
        self.rates
            .iter()
            .all(|(model, &(per_minute, billed))| rate(model) == Rate { per_minute, billed })
    }

    /// Its session's id; `None` for a log that cannot be read back.
    pub fn id(&self) -> Option<&str> {
        self.summary.as_ref()?["id"].as_str()
    }

    /// The `field` of its summary that lists strings: `tags`, `people`.
    pub fn listed(&self, field: &str) -> Vec<String> {
        self.summary
            .as_ref()
            .and_then(|summary| summary[field].as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(String::from)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Whether its title or a tag holds `needle`, which is already `folded`.
    pub fn named(&self, needle: &str) -> bool {
        let Some(summary) = &self.summary else {
            return false;
        };
        summary["title"]
            .as_str()
            .into_iter()
            .map(String::from)
            .chain(self.listed("tags"))
            .any(|text| folded(&text).contains(needle))
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::domain::session::{ENDED, LIVE, RECORDING};

    fn records(items: Vec<Value>) -> Vec<Record> {
        items
            .into_iter()
            .map(|item| item.as_object().unwrap().clone())
            .collect()
    }

    fn per_minute(price: Option<f64>) -> Rate {
        Rate {
            per_minute: price,
            billed: false,
        }
    }

    /// A session that listened to `nova` twice: once billed and unpriced,
    /// once never billed.
    fn heard() -> Vec<Record> {
        records(vec![
            json!({"type": "session", "id": "s1", "title": "Kickoff Ágil", "kind": "meeting",
                "source": LIVE, "language": "pt", "started_at": 100.0}),
            json!({"type": "state", "state": RECORDING, "at": 100.0}),
            json!({"type": "transcriber", "model": "nova", "inputs": 1, "at": 100.0}),
            json!({"type": "listening", "request": "r1", "model": "nova", "at": 100.0}),
            json!({"type": "billed", "request": "r1", "model": "nova", "seconds": 10.0,
                "share": 1.0, "at": 110.0}),
            json!({"type": "listening", "request": "r2", "model": "nova", "at": 110.0}),
            json!({"type": "speech", "who": "Eu", "text": "Oi.", "at": 130.0}),
            json!({"type": "tags", "tags": ["Cliente X"], "at": 131.0}),
            json!({"type": "state", "state": ENDED, "at": 160.0}),
        ])
    }

    #[test]
    fn an_entry_keeps_the_summary_the_prices_it_used_and_what_is_unsettled() {
        let records = heard();
        let price = |_: &str| per_minute(Some(0.01));
        let entry = Indexed::of(&records, price);
        let summarized = crate::domain::session::summarize(&records, price).unwrap();
        assert_eq!(entry.summary.as_ref(), Some(&summarized));
        assert_eq!(entry.id(), Some("s1"));
        assert_eq!(entry.listed("tags"), ["Cliente X"]);
        assert_eq!(entry.billed, ["r1"]);
        assert!(entry.unclosed);
        assert_eq!(entry.unpaid, BTreeSet::from(["nova".to_string()]));
        assert_eq!(Indexed::read(&entry.json()), Some(entry.clone()));

        assert!(entry.priced_at(price));
        assert!(!entry.priced_at(|_| per_minute(Some(0.02))), "a new price");
        assert!(!entry.priced_at(|_| Rate {
            per_minute: Some(0.01),
            billed: true
        }));
    }

    #[test]
    fn a_title_or_a_tag_names_it_whatever_the_case_and_accents() {
        let entry = Indexed::of(&heard(), |_| Rate::default());
        assert!(entry.named(&folded("agil")));
        assert!(entry.named(&folded("CLIENTE")));
        assert!(
            !entry.named(&folded("Oi")),
            "its lines are not in the index"
        );
    }

    #[test]
    fn a_log_that_cannot_be_read_back_has_an_empty_entry() {
        let entry = Indexed::of(&[], |_| Rate::default());
        assert_eq!(entry.summary, None);
        assert_eq!(entry.id(), None);
        assert!(entry.listed("people").is_empty());
        assert!(!entry.named(""));
        assert!(!entry.unclosed && entry.unpaid.is_empty() && entry.billed.is_empty());
        assert_eq!(Indexed::read(&json!({"summary": 1})), None, "not an entry");
    }
}
