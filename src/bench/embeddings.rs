//! Window embeddings of a recording in a file, so the clustering can be run
//! again without the audio or the model.
//!
//! Little endian: `u32` dimension, `u32` regions, then per region `f64` start,
//! `f64` end, `u32` windows, and per window `f64` start, `f64` end and the
//! embedding as `f32`s.

use std::path::Path;

use anyhow::{Context, Result};

use crate::domain::diarization::Region;

pub fn write(path: &Path, regions: &[Region]) -> Result<()> {
    let dimension = regions
        .iter()
        .find_map(|r| r.windows().next().map(|w| w.2.len()))
        .unwrap_or(0);
    let mut bytes = Vec::new();
    bytes.extend((dimension as u32).to_le_bytes());
    bytes.extend((regions.len() as u32).to_le_bytes());
    for region in regions {
        let (start, end) = region.span();
        bytes.extend(start.to_le_bytes());
        bytes.extend(end.to_le_bytes());
        bytes.extend((region.windows().count() as u32).to_le_bytes());
        for (start, end, embedding) in region.windows() {
            bytes.extend(start.to_le_bytes());
            bytes.extend(end.to_le_bytes());
            bytes.extend(embedding.iter().flat_map(|x| x.to_le_bytes()));
        }
    }
    std::fs::write(path, bytes).with_context(|| path.display().to_string())
}

/// Reads what `write` wrote.
#[cfg(test)]
pub fn read(bytes: &[u8]) -> Result<Vec<Region>> {
    use anyhow::bail;
    struct Reader<'a>(&'a [u8]);
    impl Reader<'_> {
        fn take<const N: usize>(&mut self) -> Result<[u8; N]> {
            let Some((head, tail)) = self.0.split_first_chunk::<N>() else {
                bail!("embeddings file ends early");
            };
            self.0 = tail;
            Ok(*head)
        }
        fn u32(&mut self) -> Result<usize> {
            Ok(u32::from_le_bytes(self.take()?) as usize)
        }
        fn f64(&mut self) -> Result<f64> {
            Ok(f64::from_le_bytes(self.take()?))
        }
        fn f32(&mut self) -> Result<f32> {
            Ok(f32::from_le_bytes(self.take()?))
        }
    }
    let mut reader = Reader(bytes);
    let dimension = reader.u32()?;
    let mut regions = Vec::new();
    for _ in 0..reader.u32()? {
        let (start, end) = (reader.f64()?, reader.f64()?);
        let mut windows = Vec::new();
        for _ in 0..reader.u32()? {
            let (from, to) = (reader.f64()?, reader.f64()?);
            let embedding: Result<Vec<f32>> = (0..dimension).map(|_| reader.f32()).collect();
            windows.push((from, to, embedding?));
        }
        regions.push(Region::new(start, end, windows));
    }
    Ok(regions)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bench::clusters::{Agreement, lifetimes, margins, overlap, replay, small, spread};
    use crate::bench::der::{Segment, parse_rttm};
    use crate::domain::diarization::Clustering;

    #[test]
    fn regions_come_back_as_they_were_written() {
        let regions = vec![
            Region::new(
                1.0,
                4.0,
                vec![(1.0, 2.5, vec![0.5, -0.5]), (2.5, 4.0, vec![1.0, 0.0])],
            ),
            Region::new(6.0, 6.25, Vec::new()),
        ];
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.emb");
        write(&path, &regions).unwrap();
        let back = read(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(back.len(), 2);
        for (a, b) in regions.iter().zip(&back) {
            assert_eq!(a.span(), b.span());
            assert!(a.windows().eq(b.windows()));
        }
        let bytes = std::fs::read(&path).unwrap();
        assert!(read(&bytes[..bytes.len() - 1]).is_err());
    }

    /// The last 7 minutes of AMI TS3004a (CC BY 4.0): CAM++ embeddings of its
    /// 433 windows and the reference turns of the same stretch.
    #[test]
    fn the_online_path_splits_the_tail_of_ts3004a_into_extra_short_speakers() {
        let regions = read(include_bytes!("../../tests/fixtures/ami-TS3004a-tail.emb")).unwrap();
        let reference =
            parse_rttm(include_str!("../../tests/fixtures/ami-TS3004a-tail.rttm")).unwrap();
        assert_eq!(regions.iter().flat_map(Region::windows).count(), 433);
        let (found, churn) = replay(&regions, Clustering::default());
        let turns: Vec<Segment> = found
            .turns
            .iter()
            .map(|t| Segment {
                start: t.start,
                end: t.end,
                speaker: t.speaker.to_string(),
            })
            .collect();
        let overlap = overlap(&reference, &turns, None);
        let centres: Vec<f64> = regions
            .iter()
            .flat_map(Region::windows)
            .map(|w| (w.0 + w.1) / 2.0)
            .collect();
        let lives = lifetimes(&found.turns, &centres, &overlap);
        // Three people spoke; five extra speakers of 1 to 2 windows were born
        // in the last minutes and never settled.
        assert_eq!(lives.len(), 8);
        assert_eq!(small(&lives), 5);
        assert!(
            lives[3..]
                .iter()
                .all(|l| l.windows <= 2 && l.first > 1290.0)
        );
        let agreement = Agreement::of(&overlap);
        assert_eq!(
            (percent(agreement.purity()), percent(agreement.coverage())),
            (64, 79)
        );
        assert_eq!((churn.changed.round(), churn.longest.round()), (5.0, 109.0));
        let spread = spread(&margins(&regions, &found.voices)).unwrap();
        assert_eq!(spread.windows, 433);
        assert_eq!(
            spread.below.map(|share| (share * 100.0).round()),
            [3.0, 4.0, 11.0]
        );
    }

    fn percent(share: f64) -> i64 {
        (share * 100.0).round() as i64
    }
}
