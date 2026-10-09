//! Hyprland: eco's windows, found by pid, focused and left out of screen
//! sharing through `hyprctl dispatch` (docs/design.md §15).

use std::path::PathBuf;
use std::process::Output;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::FutureExt;
use futures::future::BoxFuture;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::UnixStream;
use tokio::process::Command;
use tokio::task::JoinHandle;

use crate::paths;
use crate::ports::{WindowControl, WindowError};

#[derive(Default)]
pub struct HyprlandWindows {
    /// The processes whose windows are left out of screen sharing.
    hidden: Arc<Mutex<Vec<u32>>>,
    /// Reads Hyprland's events while some are, to leave out each window they open.
    follower: Mutex<Option<JoinHandle<()>>>,
}

impl WindowControl for HyprlandWindows {
    fn focus(&self, pid: u32) -> BoxFuture<'static, Result<(), WindowError>> {
        async move {
            let focus = focus_script(pid);
            let mut last = String::new();
            // The window shows a moment after its process starts.
            for _ in 0..50 {
                let output = dispatch(&focus).await?;
                if output.status.success() && String::from_utf8_lossy(&output.stdout).trim() == "ok"
                {
                    return Ok(());
                }
                last = said(&output);
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            Err(WindowError(format!(
                "Hyprland could not focus the eco window ({last})"
            )))
        }
        .boxed()
    }

    fn hide_from_share(
        &self,
        pids: Vec<u32>,
        hidden: bool,
    ) -> BoxFuture<'static, Result<(), WindowError>> {
        *self.hidden.lock().expect("not poisoned") = if hidden { pids.clone() } else { Vec::new() };
        let mut follower = self.follower.lock().expect("not poisoned");
        if !hidden {
            if let Some(follower) = follower.take() {
                follower.abort();
            }
        } else if follower.is_none() {
            *follower = paths::hypr_events()
                .map(|events| tokio::spawn(follow(events, Arc::clone(&self.hidden))));
        }
        async move { share(&pids, hidden).await }.boxed()
    }
}

impl Drop for HyprlandWindows {
    fn drop(&mut self) {
        if let Some(follower) = self.follower.get_mut().expect("not poisoned").take() {
            follower.abort();
        }
    }
}

/// Run Hyprland's Lua dispatcher on `lua`.
async fn dispatch(lua: &str) -> Result<Output, WindowError> {
    Command::new("hyprctl")
        .args(["dispatch", lua])
        .output()
        .await
        .map_err(|error| WindowError(format!("hyprctl: {error}")))
}

/// What hyprctl answered: its status and output.
fn said(output: &Output) -> String {
    format!(
        "{}: {}{}",
        output.status,
        String::from_utf8_lossy(&output.stdout).trim(),
        String::from_utf8_lossy(&output.stderr).trim()
    )
}

/// Set Hyprland's `no_screen_share` on every window of the processes `pids`.
async fn share(pids: &[u32], hidden: bool) -> Result<(), WindowError> {
    if pids.is_empty() {
        return Ok(());
    }
    let output = dispatch(&share_script(pids, hidden)).await?;
    if output.status.success() {
        Ok(())
    } else {
        Err(WindowError(format!(
            "Hyprland could not set no_screen_share on the eco windows ({})",
            said(&output)
        )))
    }
}

/// The Lua that sets `no_screen_share` on each window of `pids`.
fn share_script(pids: &[u32], hidden: bool) -> String {
    let listed: Vec<String> = pids.iter().map(|pid| format!("[{pid}] = true")).collect();
    format!(
        r#"(function()
  local pids = {{ {} }}
  for _, w in ipairs(hl.get_windows()) do
    if pids[w.pid] then
      hl.dispatch(hl.dsp.window.set_prop({{ prop = "no_screen_share", value = "{}", window = "address:" .. w.address }}))
    end
  end
  return hl.dsp.no_op()
end)()"#,
        listed.join(", "),
        u8::from(hidden)
    )
}

/// The Lua that gives the keyboard to the overlay of process `pid` (its window
/// titled `eco`, never its settings window) and raises it above that process's
/// other windows, so a dialog it opens is not hidden under the settings; an
/// error while the window has not shown yet.
fn focus_script(pid: u32) -> String {
    format!(
        r#"(function()
  for _, w in ipairs(hl.get_windows()) do
    if w.pid == {pid} and w.title == "eco" then
      hl.dispatch(hl.dsp.focus({{ window = "address:" .. w.address }}))
      return hl.dsp.window.bring_to_top({{ window = "address:" .. w.address }})
    end
  end
  error("no eco window yet")
end)()"#
    )
}

/// Leave out of screen sharing each eco window that opens while `hidden` lists
/// processes, as Hyprland's event socket `events` reports it; connects again
/// every second while the socket is down.
async fn follow(events: PathBuf, hidden: Arc<Mutex<Vec<u32>>>) {
    loop {
        if let Ok(stream) = UnixStream::connect(&events).await {
            let mut lines = BufReader::new(stream).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if opens_eco_window(&line) {
                    let pids = hidden.lock().expect("not poisoned").clone();
                    if let Err(error) = share(&pids, true).await {
                        eprintln!("eco: {error}");
                    }
                }
            }
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

/// Whether `event` is Hyprland's `openwindow>>address,workspace,class,title`
/// for a window of class `eco`.
fn opens_eco_window(event: &str) -> bool {
    event
        .strip_prefix("openwindow>>")
        .and_then(|fields| fields.split(',').nth(2))
        == Some("eco")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_an_eco_window_opening_counts() {
        assert!(opens_eco_window(
            "openwindow>>55d1,1,eco,eco · configuração"
        ));
        assert!(!opens_eco_window("openwindow>>55d1,1,firefox,eco"));
        assert!(!opens_eco_window("closewindow>>55d1"));
    }

    #[test]
    fn focus_finds_the_overlay_of_the_process_and_raises_it() {
        let script = focus_script(42);
        assert!(
            script.contains(r#"w.pid == 42 and w.title == "eco""#),
            "{script}"
        );
        assert!(script.contains("hl.dsp.window.bring_to_top"), "{script}");
    }

    #[test]
    fn the_script_sets_the_prop_on_the_listed_processes() {
        let script = share_script(&[12, 34], true);
        assert!(
            script.contains("local pids = { [12] = true, [34] = true }"),
            "{script}"
        );
        assert!(
            script.contains(r#"prop = "no_screen_share", value = "1""#),
            "{script}"
        );
        assert!(share_script(&[12], false).contains(r#"value = "0""#));
    }
}
