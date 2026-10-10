//! Hyprland: eco's windows, found by pid, focused and left out of screen
//! sharing through `hyprctl dispatch` (docs/design.md §15).

use std::path::{Path, PathBuf};
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

pub struct HyprlandWindows {
    /// The program that runs Hyprland's dispatchers.
    hyprctl: PathBuf,
    /// Hyprland's event socket, when eco runs under one.
    events: Option<PathBuf>,
    /// The processes whose windows are left out of screen sharing.
    hidden: Arc<Mutex<Vec<u32>>>,
    /// Reads Hyprland's events while some are, to leave out each window they open.
    follower: Mutex<Option<JoinHandle<()>>>,
}

impl Default for HyprlandWindows {
    fn default() -> Self {
        Self::new("hyprctl".into(), paths::hypr_events())
    }
}

impl HyprlandWindows {
    fn new(hyprctl: PathBuf, events: Option<PathBuf>) -> Self {
        Self {
            hyprctl,
            events,
            hidden: Arc::default(),
            follower: Mutex::default(),
        }
    }
}

impl WindowControl for HyprlandWindows {
    fn focus(&self, pid: u32) -> BoxFuture<'static, Result<(), WindowError>> {
        let hyprctl = self.hyprctl.clone();
        async move {
            let focus = focus_script(pid);
            let mut last = String::new();
            // The window shows a moment after its process starts.
            for _ in 0..50 {
                let output = dispatch(&hyprctl, &focus).await?;
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
            *follower = self.events.clone().map(|events| {
                tokio::spawn(follow(
                    self.hyprctl.clone(),
                    events,
                    Arc::clone(&self.hidden),
                ))
            });
        }
        let hyprctl = self.hyprctl.clone();
        async move { share(&hyprctl, &pids, hidden).await }.boxed()
    }
}

impl Drop for HyprlandWindows {
    fn drop(&mut self) {
        if let Some(follower) = self.follower.get_mut().expect("not poisoned").take() {
            follower.abort();
        }
    }
}

/// Run Hyprland's Lua dispatcher on `lua` through the program `hyprctl`.
async fn dispatch(hyprctl: &Path, lua: &str) -> Result<Output, WindowError> {
    Command::new(hyprctl)
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
async fn share(hyprctl: &Path, pids: &[u32], hidden: bool) -> Result<(), WindowError> {
    if pids.is_empty() {
        return Ok(());
    }
    let output = dispatch(hyprctl, &share_script(pids, hidden)).await?;
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
async fn follow(hyprctl: PathBuf, events: PathBuf, hidden: Arc<Mutex<Vec<u32>>>) {
    loop {
        if let Ok(stream) = UnixStream::connect(&events).await {
            let mut lines = BufReader::new(stream).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if opens_eco_window(&line) {
                    let pids = hidden.lock().expect("not poisoned").clone();
                    if let Err(error) = share(&hyprctl, &pids, true).await {
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
    use std::fs;
    use std::time::Instant;

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::UnixListener;

    use super::*;
    use crate::adapters::fake_program::fake_program;

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

    /// A hyprctl that notes each call's arguments, one file per call in
    /// `dir/calls`, and runs `answer` for its reply.
    fn hyprctl(dir: &Path, answer: &str) -> PathBuf {
        let calls = dir.join("calls");
        fs::create_dir(&calls).unwrap();
        fake_program(
            dir,
            "hyprctl",
            &format!(
                r#"n=$(ls '{calls}' | wc -l)
printf '%s\n%s' "$1" "$2" > '{calls}'/$n
{answer}"#,
                calls = calls.display()
            ),
        )
    }

    /// The calls hyprctl received, in order: its arguments, one per line.
    fn calls(dir: &Path) -> Vec<String> {
        let mut found: Vec<(usize, String)> = fs::read_dir(dir.join("calls"))
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                let number = entry.file_name().to_str().unwrap().parse().unwrap();
                (number, fs::read_to_string(entry.path()).unwrap())
            })
            .collect();
        found.sort();
        found.into_iter().map(|(_, call)| call).collect()
    }

    /// The calls once there are `count` of them; waits up to five seconds.
    async fn calls_once(dir: &Path, count: usize) -> Vec<String> {
        let started = Instant::now();
        loop {
            let found = calls(dir);
            if found.len() >= count || started.elapsed() > Duration::from_secs(5) {
                return found;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    #[tokio::test(start_paused = true)]
    async fn focus_retries_until_the_window_shows() {
        let dir = tempfile::tempdir().unwrap();
        let program = hyprctl(
            dir.path(),
            r#"[ "$n" -lt 2 ] && echo "error: no eco window yet" && exit 0
echo ok"#,
        );
        let windows = HyprlandWindows::new(program, None);
        windows.focus(42).await.unwrap();
        let calls = calls(dir.path());
        assert_eq!(calls.len(), 3);
        assert_eq!(calls[0], format!("dispatch\n{}", focus_script(42)));
    }

    #[tokio::test(start_paused = true)]
    async fn focus_gives_up_with_what_hyprland_last_said() {
        let dir = tempfile::tempdir().unwrap();
        let program = hyprctl(dir.path(), "echo 'no eco window yet' >&2; exit 3");
        let windows = HyprlandWindows::new(program, None);
        let error = windows.focus(42).await.unwrap_err();
        assert_eq!(
            error.0,
            "Hyprland could not focus the eco window (exit status: 3: no eco window yet)"
        );
        assert_eq!(calls(dir.path()).len(), 50);
    }

    #[tokio::test]
    async fn a_missing_hyprctl_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("hyprctl");
        let windows = HyprlandWindows::new(missing, None);
        let error = windows.focus(42).await.unwrap_err();
        assert!(error.0.starts_with("hyprctl: "), "{}", error.0);
    }

    #[tokio::test]
    async fn sharing_dispatches_the_script_for_the_processes() {
        let dir = tempfile::tempdir().unwrap();
        let program = hyprctl(dir.path(), "exit 0");
        let windows = HyprlandWindows::new(program, None);
        windows.hide_from_share(vec![12], true).await.unwrap();
        windows.hide_from_share(vec![12], false).await.unwrap();
        windows.hide_from_share(Vec::new(), true).await.unwrap();
        assert_eq!(
            calls(dir.path()),
            [
                format!("dispatch\n{}", share_script(&[12], true)),
                format!("dispatch\n{}", share_script(&[12], false)),
            ]
        );
    }

    #[tokio::test]
    async fn a_refused_share_says_what_hyprland_said() {
        let dir = tempfile::tempdir().unwrap();
        let program = hyprctl(dir.path(), "echo 'no such prop'; exit 1");
        let windows = HyprlandWindows::new(program, None);
        let error = windows.hide_from_share(vec![12], true).await.unwrap_err();
        assert_eq!(
            error.0,
            "Hyprland could not set no_screen_share on the eco windows \
             (exit status: 1: no such prop)"
        );
    }

    /// An eco window opening while sharing is off for eco's windows is left
    /// out too, also after the event socket went down and came back; showing
    /// them again, or dropping the control, stops following.
    #[tokio::test]
    async fn each_eco_window_that_opens_is_left_out_while_hidden() {
        let dir = tempfile::tempdir().unwrap();
        let program = hyprctl(dir.path(), "exit 1");
        let socket = dir.path().join(".socket2.sock");
        let events = UnixListener::bind(&socket).unwrap();
        let windows = HyprlandWindows::new(program, Some(socket));
        let shared = format!("dispatch\n{}", share_script(&[7], true));

        assert!(windows.hide_from_share(vec![7], true).await.is_err());
        let (first, _) = events.accept().await.unwrap();
        drop(first);
        let (mut stream, _) = events.accept().await.unwrap();
        stream
            .write_all(b"openwindow>>55d1,1,firefox,x\nopenwindow>>55d2,1,eco,eco\n")
            .await
            .unwrap();
        assert_eq!(
            calls_once(dir.path(), 2).await,
            [shared.clone(), shared.clone()]
        );

        assert!(windows.hide_from_share(vec![7], true).await.is_err());
        windows.hide_from_share(vec![7], false).await.unwrap_err();
        assert_eq!(stream.read(&mut [0; 1]).await.unwrap(), 0, "unfollowed");

        assert!(windows.hide_from_share(vec![7], true).await.is_err());
        let (mut stream, _) = events.accept().await.unwrap();
        drop(windows);
        assert_eq!(stream.read(&mut [0; 1]).await.unwrap(), 0, "dropped");
    }
}
