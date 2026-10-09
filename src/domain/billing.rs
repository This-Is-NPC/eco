//! What transcription requests cost the sessions they served: each request a
//! provider names is shared by the sessions listening with its transcriber, in
//! proportion to how long each listened while it was open.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::Duration;

use tokio::time::Instant;

use crate::domain::session::{Unknown, now};
use crate::domain::transcribers::Listening;
use crate::ports::{BillingError, TranscriptionBilling};

/// Seconds after a request closes before its provider is first asked its cost.
const FIRST_ASK: f64 = 5.0;
/// The longest wait between two asks: the waits double from the first up to it.
const LONGEST_WAIT: f64 = 3600.0;
/// Seconds after a request closes when its provider is asked no more.
const GIVE_UP: f64 = 24.0 * 3600.0;

/// How long one session listened while a request was open.
#[derive(Debug, Default)]
struct Stretch {
    seconds: f64,
    /// When the stretch running now began.
    since: Option<f64>,
}

impl Stretch {
    fn stop(&mut self, at: f64) {
        if let Some(since) = self.since.take() {
            self.seconds += (at - since).max(0.0);
        }
    }
}

struct Open {
    listening: Listening,
    billing: Arc<dyn TranscriptionBilling>,
    /// By session id.
    listeners: BTreeMap<String, Stretch>,
}

/// One session's part of a closed request.
#[derive(Debug, Clone, PartialEq)]
pub struct Share {
    pub session: String,
    /// Seconds it listened while the request was open.
    pub seconds: f64,
    /// Its fraction of the request's cost.
    pub share: f64,
}

/// A closed request and the sessions it served.
#[derive(Debug, Clone, PartialEq)]
pub struct Bill {
    pub request: String,
    pub model: String,
    pub shares: Vec<Share>,
}

/// The requests the transcribers have open, and who listens to each.
#[derive(Default)]
pub struct Requests {
    open: HashMap<String, Open>,
}

/// A session that starts listening to an open request: the request, its model
/// and the session.
#[derive(Debug, Clone, PartialEq)]
pub struct Joined {
    pub request: String,
    pub model: String,
    pub session: String,
}

impl Requests {
    /// The transcriber for `listening` bills under `request` from `at`, its cost
    /// told by `billing`; `heard` is each recording session with what it
    /// listens with. Returns the sessions that listen to it.
    pub fn open(
        &mut self,
        request: &str,
        (listening, billing): (&Listening, Arc<dyn TranscriptionBilling>),
        heard: &[(String, Listening)],
        at: f64,
    ) -> Vec<Joined> {
        let mut open = Open {
            listening: listening.clone(),
            billing,
            listeners: BTreeMap::new(),
        };
        let joined = open.follow(request, heard, at);
        self.open.insert(request.into(), open);
        joined
    }

    /// The recording sessions are `heard` from `at`: those listening with a
    /// request's transcriber count for it, the others stop counting. Returns
    /// the sessions that start listening to a request.
    pub fn follow(&mut self, heard: &[(String, Listening)], at: f64) -> Vec<Joined> {
        self.open
            .iter_mut()
            .flat_map(|(request, open)| open.follow(request, heard, at))
            .collect()
    }

    /// Whether `request` is open.
    pub fn is_open(&self, request: &str) -> bool {
        self.open.contains_key(request)
    }

    /// Close `request` at `at`: each session that listened gets its share by
    /// its time, and what tells its cost comes with it; `None` for a request not
    /// open or that nobody listened to.
    pub fn close(
        &mut self,
        request: &str,
        at: f64,
    ) -> Option<(Bill, Arc<dyn TranscriptionBilling>)> {
        let mut open = self.open.remove(request)?;
        for stretch in open.listeners.values_mut() {
            stretch.stop(at);
        }
        let listened = open
            .listeners
            .into_iter()
            .map(|(session, stretch)| (session, stretch.seconds));
        let bill = Bill {
            request: request.into(),
            model: open.listening.model,
            shares: shares(listened),
        };
        (!bill.shares.is_empty()).then_some((bill, open.billing))
    }
}

/// Each session's share of a request, by the seconds it `listened`: those that
/// listened no time have none, unless no session did, when they share alike.
pub fn shares(listened: impl IntoIterator<Item = (String, f64)>) -> Vec<Share> {
    let listened: Vec<(String, f64)> = listened.into_iter().collect();
    let total: f64 = listened.iter().map(|(_, seconds)| seconds).sum();
    if total > 0.0 {
        listened
            .into_iter()
            .filter(|(_, seconds)| *seconds > 0.0)
            .map(|(session, seconds)| Share {
                session,
                seconds,
                share: seconds / total,
            })
            .collect()
    } else {
        let alike = 1.0 / listened.len() as f64;
        listened
            .into_iter()
            .map(|(session, seconds)| Share {
                session,
                seconds,
                share: alike,
            })
            .collect()
    }
}

impl Open {
    /// Returns the sessions of `heard` that start listening to `request`.
    fn follow(&mut self, request: &str, heard: &[(String, Listening)], at: f64) -> Vec<Joined> {
        for (session, stretch) in &mut self.listeners {
            let listens = heard
                .iter()
                .any(|(id, listening)| id == session && *listening == self.listening);
            if !listens {
                stretch.stop(at);
            }
        }
        let mut joined = Vec::new();
        for (session, _) in heard.iter().filter(|(_, l)| *l == self.listening) {
            let stretch = self.listeners.entry(session.clone()).or_default();
            if stretch.since.is_none() {
                stretch.since = Some(at);
                joined.push(Joined {
                    request: request.into(),
                    model: self.listening.model.clone(),
                    session: session.clone(),
                });
            }
        }
        joined
    }
}

/// How long to wait before asking again for the cost of a request closed `age`
/// seconds ago: about as long as it has been closed, from the first wait up to
/// the longest; `None` once it is time to give up.
fn wait(age: f64) -> Option<Duration> {
    (age < GIVE_UP).then(|| Duration::from_secs_f64(age.clamp(FIRST_ASK, LONGEST_WAIT)))
}

/// What the provider charged for `request`, closed at `closed` (seconds since
/// the epoch): asked first a few seconds from now, then with waits that grow
/// with the request's age, as it may not list it yet or fail to answer.
/// `Err(why)` when it cannot tell — a key without the scope, say, or a day
/// went by without an answer.
pub async fn priced(
    billing: &dyn TranscriptionBilling,
    request: &str,
    closed: f64,
) -> Result<f64, Unknown> {
    let (asked, aged) = (Instant::now(), (now() - closed).max(0.0));
    let mut next = Some(Duration::from_secs_f64(FIRST_ASK));
    while let Some(pause) = next {
        tokio::time::sleep(pause).await;
        match billing.cost(request).await {
            Ok(Some(usd)) => return Ok(usd),
            Err(BillingError::Forbidden(_)) => return Err(Unknown::NoScope),
            Ok(None) | Err(BillingError::Failed(_)) => {}
        }
        next = wait(aged + asked.elapsed().as_secs_f64());
    }
    Err(Unknown::Unreported)
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use futures::future::BoxFuture;

    use super::*;

    fn listening(model: &str) -> Listening {
        Listening {
            model: model.into(),
            language: "pt".into(),
        }
    }

    fn heard(sessions: &[(&str, &str)]) -> Vec<(String, Listening)> {
        sessions
            .iter()
            .map(|(id, model)| (id.to_string(), listening(model)))
            .collect()
    }

    fn nova() -> (Listening, Arc<dyn TranscriptionBilling>) {
        (listening("nova"), Arc::new(provider(0, false)))
    }

    #[test]
    fn a_request_is_shared_by_the_time_each_session_listened() {
        let mut requests = Requests::default();
        let (nova, billing) = nova();
        // a listens from 0, b joins at 30, a pauses at 60, c listens elsewhere.
        let first = heard(&[("a", "nova"), ("c", "whisper")]);
        requests.open("r", (&nova, billing), &first, 0.0);
        requests.follow(&heard(&[("a", "nova"), ("b", "nova")]), 30.0);
        requests.follow(&heard(&[("b", "nova")]), 60.0);
        let (bill, _) = requests.close("r", 90.0).unwrap();
        assert_eq!(bill.model, "nova");
        assert_eq!(
            bill.shares,
            [
                Share {
                    session: "a".into(),
                    seconds: 60.0,
                    share: 0.5
                },
                Share {
                    session: "b".into(),
                    seconds: 60.0,
                    share: 0.5
                },
            ]
        );
        assert!(requests.close("r", 100.0).is_none(), "closed once");
    }

    #[test]
    fn a_request_nobody_listened_to_bills_no_one() {
        let mut requests = Requests::default();
        let (nova, billing) = nova();
        requests.open("r", (&nova, billing), &heard(&[]), 0.0);
        assert!(requests.close("r", 10.0).is_none());
    }

    #[test]
    fn the_sessions_that_join_an_open_request_are_told() {
        let mut requests = Requests::default();
        let (nova, billing) = nova();
        let joined = requests.open("r", (&nova, billing), &heard(&[("a", "nova")]), 0.0);
        let named = |joined: Vec<Joined>| -> Vec<String> {
            joined
                .into_iter()
                .map(|j| format!("{} {} {}", j.request, j.model, j.session))
                .collect()
        };
        assert_eq!(named(joined), ["r nova a"]);
        // Only a session starting to listen is told: a still listens, b joins.
        let both = heard(&[("a", "nova"), ("b", "nova")]);
        assert_eq!(named(requests.follow(&both, 10.0)), ["r nova b"]);
        assert!(requests.follow(&both, 20.0).is_empty());
        // a pauses and comes back: a new stretch.
        requests.follow(&heard(&[("b", "nova")]), 30.0);
        assert_eq!(named(requests.follow(&both, 40.0)), ["r nova a"]);
        assert!(requests.is_open("r"));
        requests.close("r", 50.0);
        assert!(!requests.is_open("r"));
    }

    #[test]
    fn sessions_that_listened_no_time_share_alike_only_when_none_did() {
        let listened = |pairs: &[(&str, f64)]| {
            shares(pairs.iter().map(|(id, seconds)| (id.to_string(), *seconds)))
                .into_iter()
                .map(|share| (share.session, share.share))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            listened(&[("a", 30.0), ("b", 0.0), ("c", 10.0)]),
            [("a".into(), 0.75), ("c".into(), 0.25)]
        );
        assert_eq!(
            listened(&[("a", 0.0), ("b", 0.0)]),
            [("a".into(), 0.5), ("b".into(), 0.5)]
        );
        assert!(listened(&[]).is_empty());
    }

    /// A provider that fails the first `failing` asks, then does not list the
    /// request until `listed_after` asks; or refuses. It keeps when it was asked.
    struct Provider {
        asked: Mutex<Vec<Instant>>,
        failing: usize,
        listed_after: usize,
        refuses: bool,
    }

    impl TranscriptionBilling for Provider {
        fn cost(&self, _: &str) -> BoxFuture<'static, Result<Option<f64>, BillingError>> {
            let asked = {
                let mut asked = self.asked.lock().unwrap();
                asked.push(Instant::now());
                asked.len()
            };
            let answer = if self.refuses {
                Err(BillingError::Forbidden("no usage:read scope".into()))
            } else if asked <= self.failing {
                Err(BillingError::Failed("deepgram 502".into()))
            } else {
                Ok((asked > self.listed_after).then_some(0.0123))
            };
            Box::pin(async move { answer })
        }
    }

    fn provider(listed_after: usize, refuses: bool) -> Provider {
        Provider {
            asked: Mutex::new(Vec::new()),
            failing: 0,
            listed_after,
            refuses,
        }
    }

    fn asked(provider: &Provider) -> usize {
        provider.asked.lock().unwrap().len()
    }

    #[tokio::test(start_paused = true)]
    async fn the_cost_is_asked_for_until_the_provider_lists_it() {
        let late = provider(2, false);
        assert_eq!(priced(&late, "r", now()).await, Ok(0.0123));
        assert_eq!(asked(&late), 3);
        let refusing = provider(0, true);
        assert_eq!(priced(&refusing, "r", now()).await, Err(Unknown::NoScope));
        assert_eq!(asked(&refusing), 1);
        // A provider that fails to answer is asked again, not given up on.
        let failing = Provider {
            failing: 2,
            ..provider(0, false)
        };
        assert_eq!(priced(&failing, "r", now()).await, Ok(0.0123));
        assert_eq!(asked(&failing), 3);
    }

    #[tokio::test(start_paused = true)]
    async fn a_cost_never_listed_is_asked_for_a_day_with_waits_that_grow() {
        let never = provider(usize::MAX, false);
        let start = Instant::now();
        assert_eq!(priced(&never, "r", now()).await, Err(Unknown::Unreported));
        let times = never.asked.lock().unwrap().clone();
        let waits: Vec<u64> = std::iter::once(start)
            .chain(times.iter().copied())
            .zip(times.iter())
            .map(|(before, after)| (*after - before).as_secs())
            .collect();
        assert_eq!(waits[..6], [5, 5, 10, 20, 40, 80]);
        assert!(waits.windows(2).all(|pair| pair[0] <= pair[1]), "{waits:?}");
        assert_eq!(waits.last(), Some(&3600));
        let asked_for = (*times.last().unwrap() - start).as_secs_f64();
        assert!((GIVE_UP..GIVE_UP + LONGEST_WAIT).contains(&asked_for));
    }

    #[tokio::test(start_paused = true)]
    async fn a_request_closed_long_ago_is_asked_soon_then_at_the_longest_wait() {
        // Closed ten hours ago, before the daemon started again.
        let left = provider(1, false);
        let start = Instant::now();
        assert_eq!(priced(&left, "r", now() - 36_000.0).await, Ok(0.0123));
        let times = left.asked.lock().unwrap().clone();
        let at: Vec<u64> = times.iter().map(|t| (*t - start).as_secs()).collect();
        assert_eq!(at, [5, 3605]);
    }
}
