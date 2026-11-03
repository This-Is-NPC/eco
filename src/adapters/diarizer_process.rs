//! Diarization in a child `eco diarize`: the embedding model's memory goes back
//! to the system when it exits. Speech regions go to its stdin as
//! `start (u64 LE, samples) · length (u32 LE) · samples (i16 LE)`; as soon as it
//! has heard a region it prints one JSON line,
//! `{"end": s, "turns": [[start, end, speaker], …], "voices": [[speaker, […]], …], "merged": [[gone, kept], …], "moved": [[start, end, from, to], …]}`.
//! Every few minutes of speech, and when its input ends, it clusters the speech
//! it kept again on a thread of its own and prints one more line of the same
//! shape: no turns, the stretches that went to another speaker as `moved`.

use std::io::{BufRead, BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::thread;

use serde_json::{Value, json};

use crate::adapters::speaker_tract::TractEmbedder;
use crate::domain::diarization::{
    Clustering, Diarizer, Moved, Recluster, Reclustered, Turn, Update,
};
use crate::domain::segmenter::Segment;
use crate::ports::{SAMPLE_RATE, SpeakerEmbedder};

/// Speech regions that wait, at most, for the child; a region is at most 15 s
/// (480 kB).
const BOUND: usize = 32;
/// The end of the child's complaints kept for the error it ends with, in bytes.
const COMPLAINT: usize = 4096;

/// What a child diarizer tells, in order: an update per region it heard, then
/// how it ended.
#[derive(Debug, PartialEq)]
pub enum Diarized {
    Update(Update),
    Done,
    Failed(String),
}

/// Where speech goes to a child diarizer: at most a bound of regions wait for it.
#[derive(Clone)]
pub struct Regions {
    sender: SyncSender<Segment>,
    /// Regions offered since the diarizer fell behind.
    dropped: Arc<AtomicUsize>,
}

/// Regions that wait, at most `bound` at once, and the end they are read from.
pub fn channel(bound: usize) -> (Regions, Receiver<Segment>) {
    let (sender, received) = mpsc::sync_channel(bound);
    let regions = Regions {
        sender,
        dropped: Arc::default(),
    };
    (regions, received)
}

impl Regions {
    /// Offer live speech: while the diarizer is behind by the bound, a region
    /// goes without it, and the log says so when that starts and when it ends.
    pub fn offer(&self, region: Segment) {
        match self.sender.try_send(region) {
            Ok(()) => {
                let dropped = self.dropped.swap(0, Ordering::Relaxed);
                if dropped > 0 {
                    eprintln!(
                        "eco: the diarizer caught up; {dropped} speech regions went without it"
                    );
                }
            }
            Err(TrySendError::Full(_)) => {
                if self.dropped.fetch_add(1, Ordering::Relaxed) == 0 {
                    eprintln!(
                        "eco: the diarizer is behind; speech regions go without it until it catches up"
                    );
                }
            }
            Err(TrySendError::Disconnected(_)) => {}
        }
    }

    /// Send speech, waiting while the diarizer is behind: an import goes at its pace.
    pub fn send(&self, region: Segment) {
        let _ = self.sender.send(region);
    }
}

fn write_region(out: &mut impl Write, region: &Segment) -> std::io::Result<()> {
    out.write_all(&(region.start as u64).to_le_bytes())?;
    out.write_all(&(region.pcm.len() as u32).to_le_bytes())?;
    for sample in &region.pcm {
        out.write_all(&sample.to_le_bytes())?;
    }
    Ok(())
}

/// The next region on `input`, or `None` at its end.
fn read_region(input: &mut impl Read) -> std::io::Result<Option<Segment>> {
    let mut start = [0u8; 8];
    match input.read_exact(&mut start) {
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        other => other?,
    }
    let mut length = [0u8; 4];
    input.read_exact(&mut length)?;
    let mut bytes = vec![0u8; u32::from_le_bytes(length) as usize * 2];
    input.read_exact(&mut bytes)?;
    Ok(Some(Segment {
        start: u64::from_le_bytes(start) as usize,
        pcm: bytes
            .chunks_exact(2)
            .map(|p| i16::from_le_bytes([p[0], p[1]]))
            .collect(),
    }))
}

fn to_json(update: &Update) -> Value {
    let turns: Vec<Value> = update
        .turns
        .iter()
        .map(|t| json!([t.start, t.end, t.speaker]))
        .collect();
    let moved: Vec<Value> = update
        .moved
        .iter()
        .map(|m| json!([m.start, m.end, m.from, m.to]))
        .collect();
    json!({
        "end": update.end, "turns": turns, "voices": update.voices,
        "merged": update.merged, "moved": moved,
    })
}

fn parse(text: &str) -> Option<Update> {
    #[derive(serde::Deserialize)]
    struct Reply {
        end: f64,
        turns: Vec<(f64, f64, usize)>,
        voices: Vec<(usize, Vec<f32>)>,
        merged: Vec<(usize, usize)>,
        moved: Vec<(f64, f64, usize, usize)>,
    }
    let reply: Reply = serde_json::from_str(text).ok()?;
    let turns = reply
        .turns
        .into_iter()
        .map(|(start, end, speaker)| Turn {
            start,
            end,
            speaker,
        })
        .collect();
    Some(Update {
        end: reply.end,
        turns,
        voices: reply.voices,
        merged: reply.merged,
        moved: reply
            .moved
            .into_iter()
            .map(|(start, end, from, to)| Moved {
                start,
                end,
                from,
                to,
            })
            .collect(),
    })
}

/// Start a child diarizer: each region sent goes to it, and `heard` is told,
/// on threads of their own, each update as the child finds it, then how it
/// ended; the child ends once every `Regions` is dropped.
pub fn spawn(model: &Path, heard: impl FnMut(Diarized) + Send + 'static) -> Regions {
    spawn_program(std::env::current_exe(), model, heard)
}

/// `spawn`, with `program` as the child, or the error that it is unknown.
fn spawn_program(
    program: std::io::Result<PathBuf>,
    model: &Path,
    mut heard: impl FnMut(Diarized) + Send + 'static,
) -> Regions {
    let (regions, received) = channel(BOUND);
    let model = model.to_path_buf();
    thread::spawn(move || {
        let program = program.map_err(|e| e.to_string());
        let ended = program.and_then(|program| diarize(&program, &model, received, &mut heard));
        heard(match ended {
            Ok(()) => Diarized::Done,
            Err(detail) => Diarized::Failed(detail),
        });
    });
    regions
}

/// The last `COMPLAINT` bytes the child says on stderr, read to its end.
fn complaints(mut stderr: impl Read) -> String {
    let mut kept: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 1024];
    while let Ok(read @ 1..) = stderr.read(&mut chunk) {
        kept.extend_from_slice(&chunk[..read]);
        let extra = kept.len().saturating_sub(COMPLAINT);
        kept.drain(..extra);
    }
    String::from_utf8_lossy(&kept).into_owned()
}

/// Send each region `regions` yields to the child diarizer `program`, telling
/// `heard` each update it prints, until the regions end and the child exits.
fn diarize(
    program: &Path,
    model: &Path,
    regions: Receiver<Segment>,
    heard: &mut (dyn FnMut(Diarized) + Send),
) -> Result<(), String> {
    let mut child = Command::new(program)
        .arg("diarize")
        .arg(model)
        // Each clip's tensors are sized by its length; without a fixed threshold
        // glibc keeps the freed ones, and the child grew to ~160 MB instead of ~70.
        .env("MALLOC_TRIM_THRESHOLD_", "1048576")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    let stdin = child.stdin.take().expect("piped");
    let stdout = child.stdout.take().expect("piped");
    let stderr = child.stderr.take().expect("piped");
    // Read as they come, so a child that says much never blocks on them.
    let complained = thread::spawn(move || complaints(stderr));
    let understood = thread::scope(|scope| {
        let reader = scope.spawn(move || {
            let mut understood = true;
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                match parse(&line) {
                    Some(update) => heard(Diarized::Update(update)),
                    None => understood = false,
                }
            }
            understood
        });
        let mut stdin = BufWriter::new(stdin);
        for region in regions {
            // Flushed whole, so the child hears each region as soon as it is said.
            if write_region(&mut stdin, &region)
                .and_then(|()| stdin.flush())
                .is_err()
            {
                break;
            }
        }
        // Closing stdin tells the child the recording is over.
        drop(stdin);
        reader.join().unwrap_or(false)
    });
    let status = child.wait().map_err(|e| e.to_string())?;
    let complaint = complained.join().unwrap_or_default();
    if !status.success() {
        return Err(complaint.trim().to_string());
    }
    if !understood {
        return Err("the diarizer's reply is not turns and voices".into());
    }
    Ok(())
}

/// `eco diarize <model>`: the child side.
pub fn serve(model: &Path) -> anyhow::Result<()> {
    serve_on(model, std::io::stdin().lock(), std::io::stdout().lock())
}

/// `serve`, reading regions from `input` and writing what each told to `out`.
fn serve_on(model: &Path, input: impl Read, out: impl Write) -> anyhow::Result<()> {
    let embedder = TractEmbedder::load(model).map_err(|e| anyhow::anyhow!(e.0))?;
    serve_with(&embedder, Clustering::default(), &start_worker, input, out)
}

/// Run a re-cluster on a thread of its own; its result arrives on the channel.
fn start_worker(job: Recluster) -> Receiver<Reclustered> {
    let (send, found) = mpsc::channel();
    thread::spawn(move || {
        let _ = send.send(job.run());
    });
    found
}

/// `serve_on` with `embedder` and `clustering`. `start` begins a re-cluster
/// apart from this thread; its result is told between two regions, and the
/// speech is clustered once more, ignoring one still on its way, when the
/// input ends.
fn serve_with(
    embedder: &dyn SpeakerEmbedder,
    clustering: Clustering,
    start: &dyn Fn(Recluster) -> Receiver<Reclustered>,
    input: impl Read,
    mut out: impl Write,
) -> anyhow::Result<()> {
    let mut diarizer = Diarizer::new(embedder, clustering);
    let mut tell = |update: &Update| -> std::io::Result<()> {
        writeln!(out, "{}", to_json(update))?;
        out.flush()
    };
    let mut working: Option<Receiver<Reclustered>> = None;
    let mut input = BufReader::new(input);
    while let Some(region) = read_region(&mut input)? {
        let at = region.start as f64 / f64::from(SAMPLE_RATE);
        let update = diarizer
            .add(at, &region.pcm)
            .map_err(|e| anyhow::anyhow!(e.0))?;
        tell(&update)?;
        match working.as_ref().map(Receiver::try_recv) {
            Some(Ok(found)) => {
                working = None;
                diarizer.regroup(&found).iter().try_for_each(&mut tell)?;
            }
            Some(Err(mpsc::TryRecvError::Disconnected)) => working = None,
            Some(Err(mpsc::TryRecvError::Empty)) | None => {}
        }
        if working.is_none() && diarizer.due() {
            working = diarizer.snapshot().map(start);
        }
    }
    drop(working);
    let last = diarizer.snapshot().map(Recluster::run);
    let moved = last.and_then(|found| diarizer.regroup(&found));
    moved.iter().try_for_each(&mut tell)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    use std::fs;

    use super::*;
    use crate::domain::diarization::{Diarization, Recording};

    fn update() -> Update {
        Update {
            end: 3.25,
            turns: vec![Turn {
                start: 1.5,
                end: 3.25,
                speaker: 1,
            }],
            voices: vec![(1, vec![0.6, 0.8])],
            merged: vec![(0, 1)],
            moved: vec![Moved {
                start: 1.0,
                end: 1.5,
                from: 2,
                to: 1,
            }],
        }
    }

    #[test]
    fn regions_and_updates_cross_the_pipe_unchanged() {
        let regions = [
            Segment {
                pcm: vec![1, -2, i16::MAX, i16::MIN],
                start: 48_000,
            },
            Segment {
                pcm: Vec::new(),
                start: 7,
            },
        ];
        let mut bytes = Vec::new();
        for region in &regions {
            write_region(&mut bytes, region).unwrap();
        }
        let mut input = bytes.as_slice();
        assert_eq!(read_region(&mut input).unwrap().as_ref(), Some(&regions[0]));
        assert_eq!(read_region(&mut input).unwrap().as_ref(), Some(&regions[1]));
        assert_eq!(read_region(&mut input).unwrap(), None);

        assert_eq!(parse(&to_json(&update()).to_string()), Some(update()));
        assert_eq!(parse("oops"), None);
    }

    #[test]
    fn live_speech_waits_within_the_bound_and_the_rest_goes_without() {
        let segment = |start| Segment {
            pcm: vec![1],
            start,
        };
        let (regions, received) = channel(1);
        regions.offer(segment(0));
        regions.offer(segment(1));
        regions.offer(segment(2));
        assert_eq!(regions.dropped.load(Ordering::Relaxed), 2);
        assert_eq!(received.recv().unwrap(), segment(0));
        regions.offer(segment(3));
        assert_eq!(regions.dropped.load(Ordering::Relaxed), 0);
        assert_eq!(received.recv().unwrap(), segment(3));
        drop(received);
        regions.offer(segment(4));
        regions.send(segment(5));
        assert_eq!(regions.dropped.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn only_the_end_of_the_complaints_is_kept() {
        let long = "x".repeat(COMPLAINT) + "the end";
        let kept = complaints(long.as_bytes());
        assert_eq!(kept.len(), COMPLAINT);
        assert!(kept.ends_with("xthe end"));
        assert_eq!(complaints(&b"short"[..]), "short");
    }

    #[cfg(unix)]
    /// A child that keeps its arguments, environment and input in `dir`, then
    /// runs `reply`.
    fn child(dir: &Path, reply: &str) -> PathBuf {
        crate::adapters::fake_program::fake_program(
            dir,
            "eco",
            &format!(
                r#"cd '{}'
echo "$1 $2 $MALLOC_TRIM_THRESHOLD_" > args
cat > input
{reply}"#,
                dir.display()
            ),
        )
    }

    /// What the child `program` told for `regions`, through `spawn`.
    fn told(program: std::io::Result<PathBuf>, regions: &[Segment]) -> Vec<Diarized> {
        let (tell, told) = mpsc::channel();
        let sender = spawn_program(program, Path::new("/m.onnx"), move |diarized| {
            tell.send(diarized).unwrap();
        });
        for region in regions {
            sender.send(region.clone());
        }
        drop(sender);
        told.iter().collect()
    }

    #[cfg(unix)]
    const REPLY: &str = r#"{"end": 3.25, "turns": [[1.5, 3.25, 1]], "voices": [[1, [0.6, 0.8]]], "merged": [[0, 1]], "moved": [[1.0, 1.5, 2, 1]]}"#;

    #[cfg(unix)]
    #[test]
    fn the_child_gets_the_regions_and_each_update_is_told() {
        let dir = tempfile::tempdir().unwrap();
        let program = child(dir.path(), &format!("echo '{REPLY}'"));
        let region = Segment {
            pcm: vec![3, -3],
            start: 16_000,
        };
        assert_eq!(
            told(Ok(program), std::slice::from_ref(&region)),
            [Diarized::Update(update()), Diarized::Done]
        );
        let mut sent = Vec::new();
        write_region(&mut sent, &region).unwrap();
        assert_eq!(fs::read(dir.path().join("input")).unwrap(), sent);
        assert_eq!(
            fs::read_to_string(dir.path().join("args")).unwrap(),
            "diarize /m.onnx 1048576\n"
        );
    }

    #[cfg(unix)]
    #[test]
    fn an_update_is_told_while_the_input_still_records() {
        let dir = tempfile::tempdir().unwrap();
        let program = crate::adapters::fake_program::fake_program(
            dir.path(),
            "eco",
            &format!("echo '{REPLY}'\ncat > /dev/null"),
        );
        let (tell, told) = mpsc::channel();
        let sender = spawn_program(Ok(program), Path::new("/m.onnx"), move |diarized| {
            tell.send(diarized).unwrap();
        });
        assert_eq!(told.recv().unwrap(), Diarized::Update(update()));
        drop(sender);
        assert_eq!(told.iter().collect::<Vec<_>>(), [Diarized::Done]);
    }

    #[cfg(unix)]
    #[test]
    fn a_child_that_fails_or_says_nonsense_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let failing = child(dir.path(), "echo '  no model at /m.onnx  ' >&2; exit 1");
        assert_eq!(
            told(Ok(failing), &[]),
            [Diarized::Failed("no model at /m.onnx".into())]
        );

        let dir = tempfile::tempdir().unwrap();
        let confused = child(dir.path(), &format!("echo oops; echo '{REPLY}'"));
        assert_eq!(
            told(Ok(confused), &[]),
            [
                Diarized::Update(update()),
                Diarized::Failed("the diarizer's reply is not turns and voices".into())
            ]
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_child_that_complains_at_length_ends_with_the_last_of_it() {
        let dir = tempfile::tempdir().unwrap();
        let program = child(
            dir.path(),
            "yes complaint | head -c 200000 >&2; echo 'out of memory' >&2; exit 1",
        );
        let told = told(Ok(program), &[]);
        let [Diarized::Failed(complaint)] = told.as_slice() else {
            panic!("{told:?}");
        };
        assert!(complaint.len() <= COMPLAINT);
        assert!(complaint.ends_with("out of memory"), "{complaint}");
    }

    #[cfg(unix)]
    #[test]
    fn a_child_that_stops_reading_ends_the_diarization() {
        let dir = tempfile::tempdir().unwrap();
        let program = crate::adapters::fake_program::fake_program(
            dir.path(),
            "eco",
            "echo 'out of memory' >&2; exit 1",
        );
        let long = Segment {
            pcm: vec![0; 200_000],
            start: 0,
        };
        let told = told(Ok(program), &[long.clone(), long]);
        assert_eq!(told, [Diarized::Failed("out of memory".into())]);
    }

    #[test]
    fn a_child_that_cannot_start_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        // The system's "file not found", error 2 on Linux and on Windows.
        let missing = std::io::Error::from_raw_os_error(2).to_string();
        assert_eq!(
            told(Ok(dir.path().join("eco")), &[]),
            [Diarized::Failed(missing)]
        );
        let unknown = told(Err(std::io::Error::other("no exe")), &[]);
        assert_eq!(unknown, [Diarized::Failed("no exe".into())]);
    }

    /// A voice by the first sample of a clip: 100 is the first speaker, 50
    /// sounds like a mix of both at first, and anything else is the second.
    struct ByFirstSample;

    impl SpeakerEmbedder for ByFirstSample {
        fn embed(&self, pcm: &[i16]) -> Result<Vec<f32>, crate::ports::EmbeddingError> {
            Ok(match pcm[0] {
                100 => vec![1.0, 0.0],
                50 => vec![0.6, 0.8],
                _ => vec![0.0, 1.0],
            })
        }
    }

    fn clip(value: i16, seconds: usize, start: usize) -> Segment {
        Segment {
            pcm: vec![value; seconds * SAMPLE_RATE as usize],
            start: start * SAMPLE_RATE as usize,
        }
    }

    /// What `serve_with` tells for `clips` when a re-cluster `start`ed is
    /// delivered as `delivery` says, as updates.
    fn served(
        clustering: Clustering,
        clips: &[Segment],
        delivery: impl Fn(Recluster) -> Receiver<Reclustered>,
    ) -> Vec<Update> {
        let mut input = Vec::new();
        for clip in clips {
            write_region(&mut input, clip).unwrap();
        }
        let mut out = Vec::new();
        serve_with(
            &ByFirstSample,
            clustering,
            &delivery,
            input.as_slice(),
            &mut out,
        )
        .unwrap();
        let text = String::from_utf8(out).unwrap();
        text.lines().map(|line| parse(line).unwrap()).collect()
    }

    /// Done at once, and waiting in the channel for the next region.
    fn at_once(job: Recluster) -> Receiver<Reclustered> {
        let (send, found) = mpsc::channel();
        send.send(job.run()).unwrap();
        found
    }

    fn early_sound_moved() -> Moved {
        Moved {
            start: 6.0,
            end: 9.0,
            from: 0,
            to: 1,
        }
    }

    /// The early sound of the second speaker joins the first when it is heard.
    /// A re-cluster done meanwhile tells it moved between two regions; one
    /// that is not done when the input ends is dropped, and the clustering
    /// that ends the input tells it; a worker that dies tells nothing.
    #[test]
    fn what_was_heard_wrong_is_moved_between_regions_or_when_the_input_ends() {
        let clips = [
            clip(100, 6, 0),
            clip(50, 3, 6),
            clip(-100, 6, 9),
            clip(-100, 3, 15),
        ];
        // 17 windows are heard after the third region.
        let clustering = Clustering {
            every: 17,
            ..Clustering::default()
        };
        let updates = served(clustering, &clips, at_once);
        let moved: Vec<bool> = updates.iter().map(|u| !u.moved.is_empty()).collect();
        assert_eq!(moved, [false, false, false, false, true]);
        assert_eq!(updates[4].moved, [early_sound_moved()]);
        assert_eq!(updates[4].end, 18.0, "after the fourth region");

        let held = std::cell::RefCell::new(Vec::new());
        let never = |job: Recluster| {
            let (send, found) = mpsc::channel();
            held.borrow_mut().push((send, job));
            found
        };
        let updates = served(clustering, &clips, never);
        let (last, regions) = updates.split_last().unwrap();
        assert!(regions.iter().all(|u| u.moved.is_empty()));
        assert_eq!(last.moved, [early_sound_moved()]);
        assert_eq!(
            held.borrow().len(),
            1,
            "one re-cluster began and none piled up"
        );
        let mut recording = Recording::default();
        updates
            .into_iter()
            .for_each(|update| recording.apply(update));
        let speakers: Vec<usize> = recording.finish().turns.iter().map(|t| t.speaker).collect();
        assert_eq!(speakers, [0, 1]);

        let died = |_: Recluster| mpsc::channel().1;
        let updates = served(clustering, &clips, died);
        assert_eq!(updates.last().unwrap().moved, [early_sound_moved()]);
        assert_eq!(updates.len(), 5);
        // On a real thread, early or late, the speech heard so far moves nothing.
        let early = Clustering {
            every: 10,
            ..Clustering::default()
        };
        let updates = served(early, &clips, start_worker);
        let (last, regions) = updates.split_last().unwrap();
        assert!(regions.iter().all(|u| u.moved.is_empty()));
        assert_eq!(last.moved, [early_sound_moved()]);
        assert!(served(Clustering::default(), &[], start_worker).is_empty());
    }

    /// The child side over the speaker model and three AMI clips (see
    /// `speaker_tract`): an update per clip, a speaker per clip; a model that
    /// is not there is an error.
    #[test]
    fn the_child_side_diarizes_its_input() {
        let model = crate::paths::speaker_model();
        assert!(model.exists(), "run `mise run setup` first");
        let bytes = include_bytes!("../../tests/fixtures/speaker-clips.s16");
        let mut input = Vec::new();
        for (index, clip) in bytes.chunks(bytes.len() / 3).enumerate() {
            let region = Segment {
                pcm: clip
                    .chunks_exact(2)
                    .map(|p| i16::from_le_bytes([p[0], p[1]]))
                    .collect(),
                start: index * 40_000,
            };
            write_region(&mut input, &region).unwrap();
        }
        let mut out = Vec::new();
        serve_on(&model, input.as_slice(), &mut out).unwrap();
        let mut recording = Recording::default();
        let text = String::from_utf8(out).unwrap();
        for line in text.lines() {
            recording.apply(parse(line).unwrap());
        }
        assert_eq!(text.lines().count(), 3);
        let Diarization { turns, voices } = recording.finish();
        let speakers: Vec<usize> = turns.iter().map(|t| t.speaker).collect();
        assert_eq!(speakers, [0, 1, 2]);
        assert_eq!(voices.len(), 3);

        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("model.onnx");
        assert!(serve_on(&missing, &[][..], Vec::new()).is_err());
    }
}
