//! Audio sources (files in the session's Audio Files folder) and their decoded data, and the
//! movies video clips reference.

use crate::SourceId;
use soundcraft_audio_io::{AudioBuffer, FileFormat, peaks::Peaks};
use std::collections::BTreeMap;
use std::sync::Arc;

/// Metadata for an audio file used by the session (serialised).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Source {
    pub id: SourceId,
    pub name: String,
    /// Path relative to the session folder (or absolute for referenced media).
    pub path: String,
    pub channels: u16,
    pub frames: u64,
    pub sample_rate: u32,
    pub format: FileFormat,
    /// Original BWF time stamp (samples), if any.
    #[serde(default)]
    pub time_reference: u64,
    /// True when the file has not been written to disk yet (recorded/rendered this session).
    #[serde(default)]
    pub unsaved: bool,
}

/// A movie used by Video track clips (serialised). The pictures themselves are never stored in
/// the session: the UI opens the movie from `path` and decodes frames on demand.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct VideoSource {
    pub id: SourceId,
    pub name: String,
    /// Absolute path of the movie (referenced, never copied into the session folder).
    pub path: String,
    #[serde(default)]
    pub width: u32,
    #[serde(default)]
    pub height: u32,
    /// Frames per second (0 = unknown).
    #[serde(default)]
    pub frame_rate: f64,
    /// Picture length in seconds.
    #[serde(default)]
    pub duration: f64,
    /// Codec id (`h264`, `prores`, `mjpeg`, `hevc`, …).
    #[serde(default)]
    pub codec: String,
    /// Why the movie is not opened, when its stored path was refused on opening the session.
    #[serde(skip)]
    pub offline: Option<String>,
}

impl VideoSource {
    /// Display aspect ratio (width / height), 16:9 when unknown.
    pub fn aspect(&self) -> f64 {
        if self.width == 0 || self.height == 0 { 16.0 / 9.0 } else { f64::from(self.width) / f64::from(self.height) }
    }
}

/// Decoded audio plus waveform overviews (not serialised).
#[derive(Debug)]
pub struct SourceAudio {
    pub buffer: AudioBuffer,
    pub peaks: Vec<Peaks>,
}

impl SourceAudio {
    pub fn new(buffer: AudioBuffer) -> Self {
        let peaks = buffer.channels.iter().map(|c| Peaks::build(c)).collect();
        SourceAudio { buffer, peaks }
    }
    pub fn frames(&self) -> usize {
        self.buffer.frames()
    }
    pub fn channel(&self, ch: usize) -> Option<&[f32]> {
        self.buffer.channels.get(ch).map(Vec::as_slice)
    }
}

/// Read access to a source's samples without owning them as one in-RAM buffer. [`SourceAudio`]
/// is the in-RAM implementation; a host can supply one that reads from disk in chunks.
pub trait SampleSource: Send + Sync + std::fmt::Debug {
    fn channels(&self) -> usize;
    fn sample_rate(&self) -> u32;
    /// Length in frames (the same for every channel).
    fn frames(&self) -> u64;
    /// Copy up to `out.len()` samples of `channel` starting at `start_frame` into `out`; returns
    /// how many were written (fewer than asked only at the end of the source, 0 past it or for an
    /// unknown channel).
    fn read(&self, channel: usize, start_frame: u64, out: &mut [f32]) -> usize;
    /// True when `read` never blocks, locks or allocates, so the audio thread may call it (an
    /// in-RAM source or a read-ahead wrapper). Default: false; a source that may touch the disk
    /// on `read` is only read off the audio thread.
    fn realtime_safe(&self) -> bool {
        false
    }
}

impl SampleSource for SourceAudio {
    fn channels(&self) -> usize {
        self.buffer.channels.len()
    }
    fn sample_rate(&self) -> u32 {
        self.buffer.sample_rate
    }
    fn frames(&self) -> u64 {
        SourceAudio::frames(self) as u64
    }
    fn read(&self, channel: usize, start_frame: u64, out: &mut [f32]) -> usize {
        let Some(ch) = self.channel(channel) else { return 0 };
        let Ok(start) = usize::try_from(start_frame) else { return 0 };
        if start >= ch.len() {
            return 0;
        }
        let n = out.len().min(ch.len() - start);
        out[..n].copy_from_slice(&ch[start..start + n]);
        n
    }
}

/// Shared audio, keyed by source id: decoded in-RAM audio (`map`) and sources that stream their
/// samples (`streams`, see [`SampleSource`]). Ids are unique across both.
#[derive(Debug, Clone, Default)]
pub struct SourcePool {
    map: BTreeMap<SourceId, Arc<SourceAudio>>,
    streams: BTreeMap<SourceId, Arc<dyn SampleSource>>,
}

impl SourcePool {
    pub fn insert(&mut self, id: SourceId, audio: Arc<SourceAudio>) {
        self.streams.remove(&id);
        self.map.insert(id, audio);
    }
    /// Add a source that streams its samples (replaces any source with this id).
    pub fn insert_stream(&mut self, id: SourceId, source: Arc<dyn SampleSource>) {
        self.map.remove(&id);
        self.streams.insert(id, source);
    }
    /// The source as a [`SampleSource`], whether in RAM or streamed.
    pub fn get_source(&self, id: SourceId) -> Option<Arc<dyn SampleSource>> {
        if let Some(a) = self.map.get(&id) {
            return Some(a.clone() as Arc<dyn SampleSource>);
        }
        self.streams.get(&id).cloned()
    }
    pub fn get(&self, id: SourceId) -> Option<&Arc<SourceAudio>> {
        self.map.get(&id)
    }
    pub fn remove(&mut self, id: SourceId) -> Option<Arc<SourceAudio>> {
        self.streams.remove(&id);
        self.map.remove(&id)
    }
    pub fn contains(&self, id: SourceId) -> bool {
        self.map.contains_key(&id) || self.streams.contains_key(&id)
    }
    pub fn len(&self) -> usize {
        self.map.len() + self.streams.len()
    }
    pub fn is_empty(&self) -> bool {
        self.map.is_empty() && self.streams.is_empty()
    }
    pub fn ids(&self) -> impl Iterator<Item = SourceId> + '_ {
        let mut v: Vec<SourceId> = self.map.keys().chain(self.streams.keys()).copied().collect();
        v.sort();
        v.into_iter()
    }
}

impl PartialEq for SourcePool {
    fn eq(&self, other: &Self) -> bool {
        self.map.len() == other.map.len()
            && self.map.iter().zip(other.map.iter()).all(|((a, x), (b, y))| a == b && Arc::ptr_eq(x, y))
            && self.streams.len() == other.streams.len()
            && self.streams.iter().zip(other.streams.iter()).all(|((a, x), (b, y))| a == b && Arc::ptr_eq(x, y))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A source that only hands out samples in small chunks, like a disk reader would.
    #[derive(Debug)]
    struct Chunked {
        data: Vec<Vec<f32>>,
        rate: u32,
    }
    impl SampleSource for Chunked {
        fn channels(&self) -> usize {
            self.data.len()
        }
        fn sample_rate(&self) -> u32 {
            self.rate
        }
        fn frames(&self) -> u64 {
            self.data[0].len() as u64
        }
        fn read(&self, channel: usize, start: u64, out: &mut [f32]) -> usize {
            let Some(ch) = self.data.get(channel) else { return 0 };
            let start = start as usize;
            if start >= ch.len() {
                return 0;
            }
            let n = out.len().min(ch.len() - start).min(7);
            out[..n].copy_from_slice(&ch[start..start + n]);
            n
        }
    }

    fn ramp(n: usize, k: f32) -> Vec<f32> {
        (0..n).map(|i| i as f32 * k).collect()
    }

    fn read_all(s: &dyn SampleSource, ch: usize) -> Vec<f32> {
        let mut out = vec![0.0; s.frames() as usize];
        let mut at = 0;
        while at < out.len() {
            let n = s.read(ch, at as u64, &mut out[at..]);
            assert!(n > 0);
            at += n;
        }
        out
    }

    #[test]
    fn in_ram_and_chunked_sources_read_the_same_samples() {
        let data = vec![ramp(100, 0.5), ramp(100, -0.25)];
        let ram = Arc::new(SourceAudio::new(AudioBuffer { sample_rate: 48_000, channels: data.clone() }));
        let chunked = Arc::new(Chunked { data: data.clone(), rate: 48_000 });
        let mut pool = SourcePool::default();
        pool.insert(SourceId(1), ram);
        pool.insert_stream(SourceId(2), chunked);
        assert_eq!(pool.len(), 2);
        assert!(pool.contains(SourceId(1)) && pool.contains(SourceId(2)));
        assert_eq!(pool.ids().collect::<Vec<_>>(), vec![SourceId(1), SourceId(2)]);
        for id in [SourceId(1), SourceId(2)] {
            let s = pool.get_source(id).unwrap();
            assert_eq!((s.channels(), s.sample_rate(), s.frames()), (2, 48_000, 100));
            for (ch, want) in data.iter().enumerate() {
                assert_eq!(&read_all(&*s, ch), want);
            }
            let mut buf = [0.0; 10];
            assert_eq!(s.read(0, 95, &mut buf), 5);
            assert_eq!(s.read(0, 100, &mut buf), 0);
            assert_eq!(s.read(5, 0, &mut buf), 0);
        }
        assert!(pool.get(SourceId(2)).is_none());
        assert!(pool.remove(SourceId(2)).is_none());
        assert!(!pool.contains(SourceId(2)));
    }
}
