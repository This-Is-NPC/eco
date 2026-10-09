//! Mono s16le audio read frame by frame from a child process's stdout. The
//! process lives exactly as long as the stream: dropping the stream kills it.

use std::process::Stdio;

use futures::stream::{self, BoxStream, StreamExt};
use tokio::io::AsyncReadExt;
use tokio::process::{Child, Command};

use crate::ports::{AudioError, FRAME_BYTES, Frame};

/// Run `command` and stream its output as frames. A last partial frame is padded
/// with silence; a process that exits with a failure ends the stream with an
/// error carrying the last line it wrote to stderr.
pub fn frames(mut command: Command) -> BoxStream<'static, Result<Frame, AudioError>> {
    let name = command
        .as_std()
        .get_program()
        .to_string_lossy()
        .into_owned();
    let spawned = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn();
    let child = match spawned {
        Ok(child) => child,
        Err(error) => {
            return stream::once(async move { Err(AudioError(format!("{name}: {error}"))) })
                .boxed();
        }
    };
    stream::unfold(Some((child, name)), |state| async move {
        let (mut child, name) = state?;
        let mut bytes = [0u8; FRAME_BYTES];
        let mut filled = 0;
        while filled < FRAME_BYTES {
            match child.stdout.as_mut()?.read(&mut bytes[filled..]).await {
                Ok(0) => break,
                Ok(read) => filled += read,
                Err(error) => return Some((Err(AudioError(format!("{name}: {error}"))), None)),
            }
        }
        if filled > 0 {
            return Some((Ok(samples(&bytes)), Some((child, name))));
        }
        let failure = exit_failure(&mut child).await;
        failure.map(|reason| (Err(AudioError(format!("{name}: {reason}"))), None))
    })
    .boxed()
}

/// Why the process failed, once it has exited; none when it succeeded.
async fn exit_failure(child: &mut Child) -> Option<String> {
    let mut stderr = String::new();
    if let Some(pipe) = child.stderr.as_mut() {
        let _ = pipe.read_to_string(&mut stderr).await;
    }
    let status = child.wait().await.ok()?;
    if status.success() {
        return None;
    }
    let last = stderr.lines().rev().find(|line| !line.trim().is_empty());
    Some(last.map_or_else(|| status.to_string(), |line| line.trim().to_string()))
}

fn samples(bytes: &[u8]) -> Frame {
    bytes
        .chunks_exact(2)
        .map(|pair| i16::from_le_bytes([pair[0], pair[1]]))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::FRAME_SAMPLES;

    fn shell(script: &str) -> Command {
        let mut command = Command::new("sh");
        command.args(["-c", script]);
        command
    }

    #[test]
    fn samples_are_little_endian() {
        assert_eq!(
            samples(&[0x01, 0x00, 0xff, 0xff, 0x00, 0x80]),
            [1, -1, i16::MIN]
        );
    }

    #[tokio::test]
    async fn pads_the_last_frame_and_ends_with_the_output() {
        let frames: Vec<_> = frames(shell("head -c 1500 /dev/zero | tr '\\0' '\\1'"))
            .collect()
            .await;
        assert_eq!(frames.len(), 2);
        let last = frames[1].as_ref().unwrap();
        assert_eq!(last.len(), FRAME_SAMPLES);
        assert_eq!(last[(1500 - FRAME_BYTES) / 2 - 1], 0x0101);
        assert_eq!(last[(1500 - FRAME_BYTES) / 2], 0);
    }

    #[tokio::test]
    async fn a_failing_process_ends_with_its_last_error_line() {
        let frames: Vec<_> = frames(shell("echo noise >&2; echo 'bad input' >&2; exit 3"))
            .collect()
            .await;
        let [Err(error)] = frames.as_slice() else {
            panic!("{frames:?}")
        };
        assert_eq!(error.0, "sh: bad input");
    }

    #[tokio::test]
    async fn a_missing_program_is_an_error() {
        let frames: Vec<_> = frames(Command::new("no-such-eco-program")).collect().await;
        assert!(matches!(frames.as_slice(), [Err(e)] if e.0.starts_with("no-such-eco-program: ")));
    }
}
