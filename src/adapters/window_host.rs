//! Window control by eco-window itself, as on Windows: the daemon names a
//! window process by pid in an event every client receives — `window_focus`
//! to come to the front with the keyboard, `window_share` to leave its windows
//! out of screen capture or show them again — and the window whose pid it is
//! answers through its host. A window that connects later, as one does just
//! after it opens, is told the last of each when it is greeted
//! (docs/design.md §15).

use std::sync::Mutex;

use futures::FutureExt;
use futures::future::BoxFuture;
use serde_json::json;

use crate::domain::assistant::Emit;
use crate::domain::events::Event;
use crate::ports::{WindowControl, WindowError};

pub struct HostWindows {
    emit: Emit,
    told: Mutex<Told>,
}

/// What the windows were last told.
#[derive(Default)]
struct Told {
    /// The process whose window was last given the keyboard.
    focused: Option<u32>,
    /// The processes whose windows are left out of screen capture.
    hidden: Vec<u32>,
}

impl HostWindows {
    pub fn new(emit: Emit) -> Self {
        Self {
            emit,
            told: Mutex::default(),
        }
    }
}

fn focus(pid: u32) -> Event {
    json!({"type": "window_focus", "pid": pid})
}

fn share(pids: &[u32], hidden: bool) -> Event {
    json!({"type": "window_share", "pids": pids, "hidden": hidden})
}

impl WindowControl for HostWindows {
    fn focus(&self, pid: u32) -> BoxFuture<'static, Result<(), WindowError>> {
        self.told.lock().expect("not poisoned").focused = Some(pid);
        (self.emit)(focus(pid));
        async { Ok(()) }.boxed()
    }

    fn hide_from_share(
        &self,
        pids: Vec<u32>,
        hidden: bool,
    ) -> BoxFuture<'static, Result<(), WindowError>> {
        self.told.lock().expect("not poisoned").hidden =
            if hidden { pids.clone() } else { Vec::new() };
        (self.emit)(share(&pids, hidden));
        async { Ok(()) }.boxed()
    }

    fn greeting(&self) -> Vec<Event> {
        let told = self.told.lock().expect("not poisoned");
        let hidden = (!told.hidden.is_empty()).then(|| share(&told.hidden, true));
        told.focused.map(focus).into_iter().chain(hidden).collect()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    /// Windows told through an emit that keeps every event.
    fn windows() -> (HostWindows, Arc<Mutex<Vec<Event>>>) {
        let events: Arc<Mutex<Vec<Event>>> = Arc::default();
        let kept = Arc::clone(&events);
        let emit: Emit = Arc::new(move |event| kept.lock().unwrap().push(event));
        (HostWindows::new(emit), events)
    }

    #[tokio::test]
    async fn a_window_is_told_by_pid_to_come_to_the_front() {
        let (windows, events) = windows();
        windows.focus(41).await.unwrap();
        windows.focus(42).await.unwrap();
        assert_eq!(
            *events.lock().unwrap(),
            [
                json!({"type": "window_focus", "pid": 41}),
                json!({"type": "window_focus", "pid": 42})
            ]
        );
    }

    #[tokio::test]
    async fn the_windows_are_told_to_leave_the_share_and_come_back() {
        let (windows, events) = windows();
        windows.hide_from_share(vec![41, 42], true).await.unwrap();
        windows.hide_from_share(vec![41], false).await.unwrap();
        assert_eq!(
            *events.lock().unwrap(),
            [
                json!({"type": "window_share", "pids": [41, 42], "hidden": true}),
                json!({"type": "window_share", "pids": [41], "hidden": false})
            ]
        );
    }

    #[tokio::test]
    async fn a_window_that_connects_is_greeted_with_the_last_of_each() {
        let (windows, _) = windows();
        assert_eq!(windows.greeting(), [] as [Event; 0]);
        windows.focus(41).await.unwrap();
        windows.hide_from_share(vec![41], true).await.unwrap();
        windows.focus(42).await.unwrap();
        windows.hide_from_share(vec![41, 42], true).await.unwrap();
        assert_eq!(
            windows.greeting(),
            [
                json!({"type": "window_focus", "pid": 42}),
                json!({"type": "window_share", "pids": [41, 42], "hidden": true})
            ]
        );
        windows.hide_from_share(vec![41, 42], false).await.unwrap();
        assert_eq!(
            windows.greeting(),
            [json!({"type": "window_focus", "pid": 42})]
        );
    }
}
