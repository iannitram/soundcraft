//! Read-ahead for streamed (disk-backed) sources.
//!
//! A [`ReadAheadSource`] wraps a [`SampleSource`] whose `read` may touch the disk. A background
//! (non-realtime) thread reads ahead of the playhead and pushes fixed-size blocks into one
//! lock-free SPSC ring (`rtrb`) per channel. On the audio thread `read` only pops from those rings
//! and copies: no lock (a `try_lock` on a mutex only the audio thread takes), no allocation, no
//! disk. If the data is not there yet it writes silence and counts an underrun. Off the audio
//! thread `read` goes straight to the wrapped source (exact, may block), so offline mixdown and
//! bounce are unchanged.
//!
//! A read that is not at or just after what was already delivered (a seek, a loop, a jump) resets
//! the channel: the audio thread bumps a generation number and names the new start frame, the
//! prefetch thread restarts there, and blocks of the old generation are dropped on pop. The
//! ring is shared by every clip that plays the same source.

use rtrb::{Consumer, Producer, RingBuffer};
use soundcraft_model::SampleSource;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

/// Frames per ring block.
pub const BLOCK_FRAMES: usize = 2048;
/// Default read-ahead depth per channel (one second at 48 kHz).
pub const DEFAULT_RING_FRAMES: usize = 48_000;
/// Frames kept before the first requested frame after a reset, for interpolating readers.
const SEEK_MARGIN: u64 = 8;

struct Block {
    epoch: u32,
    start: u64,
    len: usize,
    data: [f32; BLOCK_FRAMES],
}

impl Block {
    fn empty() -> Block {
        Block { epoch: 0, start: 0, len: 0, data: [0.0; BLOCK_FRAMES] }
    }
    fn end(&self) -> u64 {
        self.start + self.len as u64
    }
}

struct ChannelShared {
    epoch: AtomicU32,
    /// First frame the prefetch thread reads after the generation changes.
    target: AtomicU64,
    /// Frame the prefetch thread has read up to (exclusive) in the current generation.
    cursor: AtomicU64,
}

struct Shared {
    inner: Arc<dyn SampleSource>,
    channels: Vec<ChannelShared>,
    stop: AtomicBool,
    underruns: AtomicU64,
    underrun_frames: AtomicU64,
    resets: AtomicU64,
}

/// Audio-thread side of one channel.
struct ChannelReader {
    cons: Consumer<Block>,
    cur: Box<Block>,
    prev: Box<Block>,
    epoch: u32,
    /// Where the known data starts when no block has arrived yet (0, or the last reset target).
    anchor: u64,
    /// Ring capacity in frames: a request further ahead than this is a seek.
    reach: u64,
}

impl ChannelReader {
    fn copy_from_blocks(&self, pos: u64, end: u64, dst: &mut [f32]) -> usize {
        for b in [&*self.cur, &*self.prev] {
            if b.len > 0 && pos >= b.start && pos < b.end() {
                let n = (end.min(b.end()) - pos) as usize;
                let off = (pos - b.start) as usize;
                dst[..n].copy_from_slice(&b.data[off..off + n]);
                return n;
            }
        }
        0
    }

    fn known_range(&self) -> (u64, u64) {
        let hi = if self.cur.len > 0 { self.cur.end() } else { self.anchor };
        let lo = if self.prev.len > 0 {
            self.prev.start
        } else if self.cur.len > 0 {
            self.cur.start
        } else {
            self.anchor
        };
        (lo, hi)
    }

    fn reset(&mut self, start: u64, sh: &ChannelShared) {
        let target = start.saturating_sub(SEEK_MARGIN);
        sh.target.store(target, Ordering::Release);
        self.epoch = sh.epoch.fetch_add(1, Ordering::AcqRel).wrapping_add(1);
        for _ in 0..self.cons.slots() {
            if self.cons.pop().is_err() {
                break;
            }
        }
        self.cur.len = 0;
        self.prev.len = 0;
        self.anchor = target;
    }

    /// Fills `out` (frames `start..`) from the ring; returns the frames actually delivered. The
    /// rest of `out` is zeroed.
    fn fill(&mut self, start: u64, out: &mut [f32], sh: &ChannelShared, resets: &AtomicU64) -> usize {
        let end = start + out.len() as u64;
        let (lo, hi) = self.known_range();
        if start < lo || start > hi.saturating_add(self.reach) {
            self.reset(start, sh);
            resets.fetch_add(1, Ordering::Relaxed);
        }
        let mut pos = start;
        while pos < end {
            let o = (pos - start) as usize;
            let n = self.copy_from_blocks(pos, end, &mut out[o..]);
            if n > 0 {
                pos += n as u64;
                continue;
            }
            match self.cons.pop() {
                Ok(b) if b.epoch == self.epoch && b.len > 0 => {
                    std::mem::swap(&mut self.prev, &mut self.cur);
                    *self.cur = b;
                    if self.cur.start > pos {
                        // A gap before the next block: silence up to it.
                        let upto = self.cur.start.min(end);
                        out[o..(upto - start) as usize].fill(0.0);
                        pos = upto;
                        if upto < end && self.cur.start >= end {
                            break;
                        }
                    }
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }
        let got = (pos - start) as usize;
        out[got..].fill(0.0);
        got
    }
}

/// A streamed source with a non-realtime read-ahead thread; see the module docs.
pub struct ReadAheadSource {
    shared: Arc<Shared>,
    readers: Vec<Mutex<ChannelReader>>,
    thread: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for ReadAheadSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReadAheadSource")
            .field("channels", &self.readers.len())
            .field("underruns", &self.underruns())
            .field("resets", &self.resets())
            .finish()
    }
}

impl ReadAheadSource {
    /// Wraps `inner`, reading `ring_frames` (rounded up to whole blocks) ahead per channel, and
    /// starts the prefetch thread from frame 0.
    pub fn new(inner: Arc<dyn SampleSource>, ring_frames: usize) -> Self {
        let nch = inner.channels();
        let blocks = ring_frames.div_ceil(BLOCK_FRAMES).max(2);
        let shared = Arc::new(Shared {
            inner,
            channels: (0..nch).map(|_| ChannelShared { epoch: AtomicU32::new(0), target: AtomicU64::new(0), cursor: AtomicU64::new(0) }).collect(),
            stop: AtomicBool::new(false),
            underruns: AtomicU64::new(0),
            underrun_frames: AtomicU64::new(0),
            resets: AtomicU64::new(0),
        });
        let mut readers = Vec::with_capacity(nch);
        let mut producers = Vec::with_capacity(nch);
        for _ in 0..nch {
            let (p, c) = RingBuffer::<Block>::new(blocks);
            producers.push(p);
            readers.push(Mutex::new(ChannelReader {
                cons: c,
                cur: Box::new(Block::empty()),
                prev: Box::new(Block::empty()),
                epoch: 0,
                anchor: 0,
                reach: (blocks * BLOCK_FRAMES) as u64,
            }));
        }
        let sh = Arc::clone(&shared);
        let thread = std::thread::Builder::new().name("soundcraft-readahead".into()).spawn(move || prefetch(&sh, producers)).ok();
        ReadAheadSource { shared, readers, thread }
    }

    /// Audio-thread reads that came up short (some frames were silence).
    pub fn underruns(&self) -> u64 {
        self.shared.underruns.load(Ordering::Relaxed)
    }
    /// Total frames replaced by silence.
    pub fn underrun_frames(&self) -> u64 {
        self.shared.underrun_frames.load(Ordering::Relaxed)
    }
    /// How many times a seek (a read away from the delivered position) reset the rings.
    pub fn resets(&self) -> u64 {
        self.shared.resets.load(Ordering::Relaxed)
    }
    /// Frame the prefetch thread has read up to (exclusive) on `channel` since the last reset.
    pub fn prefetched_until(&self, channel: usize) -> u64 {
        self.shared.channels.get(channel).map_or(0, |c| c.cursor.load(Ordering::Acquire))
    }
}

impl Drop for ReadAheadSource {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Release);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn prefetch(sh: &Shared, mut producers: Vec<Producer<Block>>) {
    let frames = sh.inner.frames();
    let mut state: Vec<(u32, u64)> = vec![(0, 0); producers.len()];
    while !sh.stop.load(Ordering::Acquire) {
        let mut worked = false;
        for (ch, prod) in producers.iter_mut().enumerate() {
            let cs = &sh.channels[ch];
            let g = cs.epoch.load(Ordering::Acquire);
            if g != state[ch].0 {
                state[ch] = (g, cs.target.load(Ordering::Acquire));
                cs.cursor.store(state[ch].1, Ordering::Release);
            }
            let mut pushed = 0;
            while pushed < 4 && prod.slots() > 0 && state[ch].1 < frames {
                let mut blk = Block::empty();
                let n = sh.inner.read(ch, state[ch].1, &mut blk.data);
                blk.epoch = state[ch].0;
                blk.start = state[ch].1;
                blk.len = n;
                if cs.epoch.load(Ordering::Acquire) != state[ch].0 {
                    break; // a seek happened while reading: restart on the next pass
                }
                state[ch].1 = if n == 0 { frames } else { state[ch].1 + n as u64 };
                if n > 0 && prod.push(blk).is_err() {
                    break;
                }
                cs.cursor.store(state[ch].1, Ordering::Release);
                pushed += 1;
                worked = true;
            }
        }
        if !worked {
            std::thread::sleep(Duration::from_micros(500));
        }
    }
}

impl SampleSource for ReadAheadSource {
    fn channels(&self) -> usize {
        self.shared.inner.channels()
    }
    fn sample_rate(&self) -> u32 {
        self.shared.inner.sample_rate()
    }
    fn frames(&self) -> u64 {
        self.shared.inner.frames()
    }
    fn read(&self, channel: usize, start_frame: u64, out: &mut [f32]) -> usize {
        if !crate::on_audio_thread() {
            return self.shared.inner.read(channel, start_frame, out);
        }
        let frames = self.shared.inner.frames();
        let (Some(reader), Some(cs)) = (self.readers.get(channel), self.shared.channels.get(channel)) else { return 0 };
        if start_frame >= frames || out.is_empty() {
            return 0;
        }
        let want = out.len().min((frames - start_frame) as usize);
        let out = &mut out[..want];
        let got = match reader.try_lock() {
            Ok(mut r) => r.fill(start_frame, out, cs, &self.shared.resets),
            Err(_) => {
                out.fill(0.0);
                0
            }
        };
        if got < want {
            self.shared.underruns.fetch_add(1, Ordering::Relaxed);
            self.shared.underrun_frames.fetch_add((want - got) as u64, Ordering::Relaxed);
        }
        want
    }
    fn realtime_safe(&self) -> bool {
        true
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use std::time::Instant;

    /// A ramp whose reads each take a while (a slow disk).
    #[derive(Debug)]
    struct SlowRamp {
        frames: usize,
        delay: Duration,
    }
    impl SampleSource for SlowRamp {
        fn channels(&self) -> usize {
            1
        }
        fn sample_rate(&self) -> u32 {
            48_000
        }
        fn frames(&self) -> u64 {
            self.frames as u64
        }
        fn read(&self, channel: usize, start: u64, out: &mut [f32]) -> usize {
            std::thread::sleep(self.delay);
            if channel != 0 || start >= self.frames as u64 {
                return 0;
            }
            let n = out.len().min(self.frames - start as usize);
            for (k, o) in out[..n].iter_mut().enumerate() {
                *o = (start as usize + k) as f32;
            }
            n
        }
    }

    fn wait_until(what: &str, f: impl Fn() -> bool) {
        let t = Instant::now();
        while !f() {
            assert!(t.elapsed() < Duration::from_secs(10), "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn on_audio<T: Send>(f: impl FnOnce() -> T + Send) -> T {
        std::thread::scope(|s| {
            s.spawn(|| {
                crate::mark_audio_thread();
                f()
            })
            .join()
            .unwrap()
        })
    }

    fn src(frames: usize, delay_ms: u64, ring: usize) -> ReadAheadSource {
        ReadAheadSource::new(Arc::new(SlowRamp { frames, delay: Duration::from_millis(delay_ms) }), ring)
    }

    #[test]
    fn off_the_audio_thread_reads_pass_through() {
        let s = src(10_000, 0, 4096);
        let mut b = [0.0; 4];
        assert_eq!(s.read(0, 9_998, &mut b), 2);
        assert_eq!(&b[..2], &[9_998.0, 9_999.0]);
        assert_eq!(s.underruns(), 0);
    }

    #[test]
    fn slow_reader_plays_without_underruns_after_warm_up() {
        // Each 2048-frame block takes 2 ms to read; playback consumes 512 frames every 5 ms.
        let s = src(60_000, 2, 16_384);
        wait_until("warm-up", || s.prefetched_until(0) >= 8192);
        on_audio(|| {
            let mut b = [0.0f32; 512];
            for blk in 0..100u64 {
                assert_eq!(s.read(0, blk * 512, &mut b), 512);
                assert!(b.iter().enumerate().all(|(i, v)| *v == (blk * 512 + i as u64) as f32), "block {blk}");
                std::thread::sleep(Duration::from_millis(5));
            }
        });
        assert_eq!(s.underruns(), 0);
        assert_eq!(s.resets(), 0);
    }

    #[test]
    fn seek_resets_and_refills() {
        let s = src(200_000, 1, 8192);
        wait_until("warm-up", || s.prefetched_until(0) >= 4096);
        on_audio(|| {
            let mut b = [0.0f32; 256];
            assert_eq!(s.read(0, 0, &mut b), 256);
            // Jump far ahead: nothing there yet, so silence and one underrun.
            assert_eq!(s.read(0, 150_000, &mut b), 256);
            assert!(b.iter().all(|v| *v == 0.0));
            assert_eq!(s.resets(), 1);
            assert_eq!(s.underruns(), 1);
            wait_until("refill", || s.prefetched_until(0) >= 150_000 + 4096);
            let before = s.underruns();
            for blk in 0..8u64 {
                let at = 150_000 + blk * 256;
                assert_eq!(s.read(0, at, &mut b), 256);
                assert!(b.iter().enumerate().all(|(i, v)| *v == (at + i as u64) as f32), "block {blk}");
            }
            assert_eq!(s.underruns(), before);
            // Backwards is a seek too.
            s.read(0, 10, &mut b);
            assert_eq!(s.resets(), 2);
        });
    }

    #[test]
    fn underruns_are_counted_and_silent() {
        // A reader slower than playback: the ring runs dry.
        let s = src(100_000, 30, 4096);
        on_audio(|| {
            let mut b = [1.0f32; 2048];
            assert_eq!(s.read(0, 0, &mut b), 2048);
            assert!(b.iter().all(|v| *v == 0.0), "silence while the first block is still being read");
            assert_eq!(s.underruns(), 1);
            assert_eq!(s.underrun_frames(), 2048);
        });
    }

    #[test]
    fn end_of_source_is_not_an_underrun() {
        let s = src(3000, 0, 8192);
        wait_until("all read", || s.prefetched_until(0) >= 3000);
        on_audio(|| {
            let mut b = [0.0f32; 1000];
            assert_eq!(s.read(0, 2500, &mut b), 500);
            assert_eq!(b[499], 2999.0);
            assert_eq!(s.read(0, 3000, &mut b), 0);
        });
        assert_eq!(s.underruns(), 0);
    }

    #[test]
    fn overlapping_reads_for_interpolation() {
        let s = src(20_000, 0, 8192);
        wait_until("warm-up", || s.prefetched_until(0) >= 8192);
        on_audio(|| {
            let mut b = [0.0f32; 100];
            for at in [0u64, 97, 194, 3000, 2997] {
                // Each read starts a few frames before the previous one ended (a window margin).
                assert_eq!(s.read(0, at, &mut b), 100);
                assert_eq!(b[0], at as f32);
            }
        });
    }
}
