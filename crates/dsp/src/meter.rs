//! Level meters: DAW-style peak meter, RMS meter, and an ITU-R BS.1770 loudness meter
//! (momentary, short-term and gated integrated LUFS plus 4× oversampled true peak).
//!
//! None of the `process` methods allocate.

use crate::biquad::{Biquad, Coeffs};
use crate::util::sane_sr;
use crate::{MIN_DB, gain_to_db};

/// Peak meter with instant attack, linear-in-dB release, peak hold and a clip latch.
#[derive(Debug, Clone)]
pub struct PeakMeter {
    sr: f32,
    level_db: f32,
    hold_db: f32,
    hold_left: f32,
    hold_secs: f32,
    release_db_per_sec: f32,
    clipped: bool,
}

impl PeakMeter {
    /// A meter with 20 dB/s release and a 2 s peak hold.
    pub fn new(sample_rate: f32) -> Self {
        Self { sr: sane_sr(sample_rate), level_db: MIN_DB, hold_db: MIN_DB, hold_left: 0.0, hold_secs: 2.0, release_db_per_sec: 20.0, clipped: false }
    }

    pub fn set_release_db_per_sec(&mut self, rate: f32) {
        if rate.is_finite() {
            self.release_db_per_sec = rate.clamp(0.1, 1000.0);
        }
    }

    pub fn set_hold_secs(&mut self, secs: f32) {
        if secs.is_finite() {
            self.hold_secs = secs.clamp(0.0, 60.0);
        }
    }

    /// Feeds one block of samples.
    pub fn process(&mut self, samples: &[f32]) {
        let peak = samples.iter().filter(|v| v.is_finite()).fold(0.0f32, |m, v| m.max(v.abs()));
        self.feed(peak, samples.len());
    }

    /// Feeds an already-computed block peak (linear) covering `frames` samples.
    pub fn feed(&mut self, peak: f32, frames: usize) {
        let dt = frames as f32 / self.sr;
        if peak >= 1.0 {
            self.clipped = true;
        }
        let peak_db = gain_to_db(peak);
        let decayed = (self.level_db - self.release_db_per_sec * dt).max(MIN_DB);
        self.level_db = decayed.max(peak_db);
        if peak_db >= self.hold_db {
            self.hold_db = peak_db;
            self.hold_left = self.hold_secs;
        } else if self.hold_left > 0.0 {
            self.hold_left -= dt;
        } else {
            self.hold_db = (self.hold_db - self.release_db_per_sec * dt).max(self.level_db);
        }
    }

    /// Displayed level in dBFS.
    pub fn level_db(&self) -> f32 {
        self.level_db
    }

    /// Held peak in dBFS.
    pub fn peak_hold_db(&self) -> f32 {
        self.hold_db
    }

    /// True once any sample reached full scale, until [`PeakMeter::reset_clip`].
    pub fn clipped(&self) -> bool {
        self.clipped
    }

    pub fn reset_clip(&mut self) {
        self.clipped = false;
    }

    pub fn reset(&mut self) {
        self.level_db = MIN_DB;
        self.hold_db = MIN_DB;
        self.hold_left = 0.0;
        self.clipped = false;
    }
}

/// Exponentially averaged RMS meter (300 ms time constant by default).
#[derive(Debug, Clone)]
pub struct RmsMeter {
    sr: f32,
    coef: f32,
    ms: f64,
}

impl RmsMeter {
    pub fn new(sample_rate: f32) -> Self {
        let mut m = Self { sr: sane_sr(sample_rate), coef: 0.0, ms: 0.0 };
        m.set_window_ms(300.0);
        m
    }

    pub fn set_window_ms(&mut self, ms: f32) {
        self.coef = crate::util::time_coef(if ms.is_finite() { ms.clamp(1.0, 10_000.0) } else { 300.0 }, self.sr);
    }

    pub fn process(&mut self, samples: &[f32]) {
        let k = f64::from(1.0 - self.coef);
        for &x in samples {
            let x = if x.is_finite() { f64::from(x) } else { 0.0 };
            self.ms += (x * x - self.ms) * k;
        }
        if self.ms < 1e-30 {
            self.ms = 0.0;
        }
    }

    /// Current RMS level in dBFS (sine-referenced: an RMS of 0.707 reads -3 dB).
    pub fn rms_db(&self) -> f32 {
        gain_to_db(self.ms.sqrt() as f32)
    }

    pub fn reset(&mut self) {
        self.ms = 0.0;
    }
}

const SUB_BLOCKS: usize = 30; // 3 s of 100 ms sub-blocks
const HIST_MIN: f32 = -70.0;
const HIST_STEP: f32 = 0.05;
const HIST_BINS: usize = 1800; // -70 .. +20 LUFS
const TP_PHASES: usize = 4;
const TP_TAPS: usize = 12;

/// Frames per call above which [`LoudnessMeter::process`] measures channels on their own threads
/// (offline only: a realtime block is far smaller).
const PARALLEL_MIN_FRAMES: usize = 1 << 16;

/// One channel's share of a threaded [`LoudnessMeter::process`] call.
struct ChannelRun<'a> {
    filters: [Biquad; 2],
    acc: f64,
    hist: [f32; 2 * TP_TAPS],
    pos: usize,
    peak: f32,
    /// Energy of each sub-block completed during the call.
    blocks: Vec<f64>,
    input: &'a [f32],
}

/// Runs `x` through the K-weighting filters and adds the squared output to `acc`, sample by
/// sample in order (so the f64 sum is the same as one sample at a time).
fn k_weighted_energy(f: &mut [Biquad; 2], x: &[f32], mut acc: f64) -> f64 {
    let [f1, f2] = f;
    for &v in x {
        let v = if v.is_finite() { v } else { 0.0 };
        let y = f2.process_sample(f1.process_sample(v));
        acc += f64::from(y) * f64::from(y);
    }
    acc
}

/// Highest sample or 4x-interpolated peak over `x`, continuing the interpolator history `h`
/// (a doubled ring of the last `TP_TAPS` inputs, newest first at `pos`, so every window is one
/// contiguous slice). Each phase sums its taps in order, as a sample-at-a-time loop would.
fn true_peak_run(coefs: &[[f32; TP_PHASES]; TP_TAPS], h: &mut [f32; 2 * TP_TAPS], pos: &mut usize, x: &[f32]) -> f32 {
    let mut peak = 0.0f32;
    for &v in x {
        let v = if v.is_finite() { v } else { 0.0 };
        *pos = (*pos + TP_TAPS - 1) % TP_TAPS;
        let p = *pos;
        for slot in [p, p + TP_TAPS] {
            if let Some(s) = h.get_mut(slot) {
                *s = v;
            }
        }
        let Some(w) = h.get(p..p + TP_TAPS).and_then(|w| <&[f32; TP_TAPS]>::try_from(w).ok()) else { continue };
        let mut y = [-0.0f32; TP_PHASES];
        for (cs, s) in coefs.iter().zip(w) {
            for (acc, k) in y.iter_mut().zip(cs) {
                *acc += k * s;
            }
        }
        peak = peak.max(v.abs());
        for out in y {
            peak = peak.max(out.abs());
        }
    }
    peak
}

/// ITU-R BS.1770 / EBU R128 loudness meter.
#[derive(Debug, Clone)]
pub struct LoudnessMeter {
    sr: f32,
    weights: Vec<f32>,
    filters: Vec<[Biquad; 2]>,
    block_len: usize,
    block_pos: usize,
    block_acc: Vec<f64>,
    sub: [f64; SUB_BLOCKS],
    sub_idx: usize,
    sub_filled: usize,
    hist_count: Vec<u64>,
    hist_energy: Vec<f64>,
    tp_coefs: [[f32; TP_PHASES]; TP_TAPS],
    tp_hist: Vec<[f32; 2 * TP_TAPS]>,
    tp_pos: Vec<usize>,
    true_peak: f32,
    max_momentary: f32,
}

/// Loudness from mean-square energy (BS.1770 eq. 2).
fn lufs(energy: f64) -> f32 {
    if energy <= 1e-20 { MIN_DB } else { (-0.691 + 10.0 * energy.log10()) as f32 }
}

/// K-weighting pre-filter (high shelf) and RLB high-pass designed for any sample rate.
fn k_weighting(sr: f32) -> [Coeffs; 2] {
    let sr = f64::from(sr);
    // Bilinear prewarp. A corner at or past Nyquist wraps tan() negative (poles outside the unit
    // circle) or aliases it, so it is held just below Nyquist, as `Coeffs::design` does.
    let prewarp = |f0: f64| (std::f64::consts::PI * f0.min(sr * 0.49) / sr).tan();
    // Stage 1: shelving.
    let (f0, g, q) = (1_681.974_450_955_533, 3.999_843_853_973_347, 0.707_175_236_955_419_6);
    let k = prewarp(f0);
    let vh = 10f64.powf(g / 20.0);
    let vb = vh.powf(0.499_666_774_154_541_6);
    let a0 = 1.0 + k / q + k * k;
    let s1 = Coeffs {
        b0: ((vh + vb * k / q + k * k) / a0) as f32,
        b1: (2.0 * (k * k - vh) / a0) as f32,
        b2: ((vh - vb * k / q + k * k) / a0) as f32,
        a1: (2.0 * (k * k - 1.0) / a0) as f32,
        a2: ((1.0 - k / q + k * k) / a0) as f32,
    };
    // Stage 2: high-pass.
    let (f0, q) = (38.135_470_876_024_44, 0.500_327_037_323_877_3);
    let k = prewarp(f0);
    let a0 = 1.0 + k / q + k * k;
    let s2 = Coeffs { b0: 1.0, b1: -2.0, b2: 1.0, a1: (2.0 * (k * k - 1.0) / a0) as f32, a2: ((1.0 - k / q + k * k) / a0) as f32 };
    [s1, s2]
}

impl LoudnessMeter {
    /// Creates a meter. Channel weights default to 1.0, except a 6-channel (5.1: L R C LFE Ls Rs)
    /// layout which uses 0 for LFE and 1.41 for the surrounds.
    pub fn new(sample_rate: f32, channels: usize) -> Self {
        let sr = sane_sr(sample_rate);
        let channels = channels.clamp(1, crate::util::MAX_CHANNELS);
        let weights = if channels == 6 { vec![1.0, 1.0, 1.0, 0.0, 1.41, 1.41] } else { vec![1.0; channels] };
        let [c1, c2] = k_weighting(sr);
        // Interpolation filter: windowed sinc at the original Nyquist, split into 4 phases.
        let mut tp_coefs = [[0.0f32; TP_PHASES]; TP_TAPS];
        let total = TP_TAPS * TP_PHASES;
        let center = (total as f64 - 1.0) / 2.0;
        for i in 0..total {
            let t = (i as f64 - center) / TP_PHASES as f64;
            let sinc = if t.abs() < 1e-12 { 1.0 } else { (std::f64::consts::PI * t).sin() / (std::f64::consts::PI * t) };
            let x = i as f64 / (total as f64 - 1.0);
            let w = 0.42 - 0.5 * (2.0 * std::f64::consts::PI * x).cos() + 0.08 * (4.0 * std::f64::consts::PI * x).cos();
            if let Some(slot) = tp_coefs.get_mut(i / TP_PHASES).and_then(|tap| tap.get_mut(i % TP_PHASES)) {
                *slot = (sinc * w) as f32;
            }
        }
        for p in 0..TP_PHASES {
            let s: f32 = tp_coefs.iter().filter_map(|tap| tap.get(p)).sum();
            if s.abs() > 1e-6 {
                tp_coefs.iter_mut().filter_map(|tap| tap.get_mut(p)).for_each(|c| *c /= s);
            }
        }
        Self {
            sr,
            weights,
            filters: vec![[Biquad::new(c1), Biquad::new(c2)]; channels],
            block_len: ((sr * 0.1).round() as usize).max(1),
            block_pos: 0,
            block_acc: vec![0.0; channels],
            sub: [0.0; SUB_BLOCKS],
            sub_idx: 0,
            sub_filled: 0,
            hist_count: vec![0; HIST_BINS],
            hist_energy: vec![0.0; HIST_BINS],
            tp_coefs,
            tp_hist: vec![[0.0; 2 * TP_TAPS]; channels],
            tp_pos: vec![0; channels],
            true_peak: 0.0,
            max_momentary: MIN_DB,
        }
    }

    /// Overrides a channel's weighting (e.g. 1.41 for surrounds, 0 to exclude LFE).
    pub fn set_channel_weight(&mut self, channel: usize, weight: f32) {
        if let Some(w) = self.weights.get_mut(channel) {
            *w = if weight.is_finite() { weight.clamp(0.0, 4.0) } else { 1.0 };
        }
    }

    pub fn sample_rate(&self) -> f32 {
        self.sr
    }

    pub fn reset(&mut self) {
        for f in &mut self.filters {
            f[0].reset();
            f[1].reset();
        }
        self.block_pos = 0;
        self.block_acc.iter_mut().for_each(|v| *v = 0.0);
        self.sub = [0.0; SUB_BLOCKS];
        self.sub_idx = 0;
        self.sub_filled = 0;
        self.hist_count.iter_mut().for_each(|v| *v = 0);
        self.hist_energy.iter_mut().for_each(|v| *v = 0.0);
        self.tp_hist.iter_mut().for_each(|h| *h = [0.0; 2 * TP_TAPS]);
        self.tp_pos.iter_mut().for_each(|p| *p = 0);
        self.true_peak = 0.0;
        self.max_momentary = MIN_DB;
    }

    /// Feeds planar audio. Channels beyond the meter's channel count are ignored.
    ///
    /// Channels are independent until each 100 ms sub-block is summed, so they are processed one
    /// at a time (and, for long offline inputs, on their own threads) and combined in channel
    /// order: the result is bit-identical to feeding the samples one frame at a time.
    pub fn process(&mut self, io: &[Vec<f32>], frames: usize) {
        let ch = self.filters.len().min(io.len());
        let frames = io.iter().take(ch).map(Vec::len).fold(frames, usize::min);
        if ch >= 2 && frames >= PARALLEL_MIN_FRAMES && self.process_parallel(io, ch, frames) {
            return;
        }
        let mut n = 0;
        while n < frames {
            // Up to the end of the current sub-block.
            let len = (self.block_len - self.block_pos).min(frames - n);
            for c in 0..ch {
                let Some(x) = io.get(c).and_then(|b| b.get(n..n + len)) else { continue };
                let (Some(f), Some(acc), Some(h), Some(pos)) =
                    (self.filters.get_mut(c), self.block_acc.get_mut(c), self.tp_hist.get_mut(c), self.tp_pos.get_mut(c))
                else {
                    continue;
                };
                *acc = k_weighted_energy(f, x, *acc);
                self.true_peak = self.true_peak.max(true_peak_run(&self.tp_coefs, h, pos, x));
            }
            n += len;
            self.block_pos += len;
            if self.block_pos >= self.block_len {
                self.finish_sub_block();
            }
        }
    }

    /// [`Self::process`] with one scoped thread per channel: each returns its energy per
    /// completed sub-block, combined here in channel order. False (nothing consumed) when
    /// threads are unavailable (wasm, a failed spawn).
    fn process_parallel(&mut self, io: &[Vec<f32>], ch: usize, frames: usize) -> bool {
        let (block_len, block_pos) = (self.block_len, self.block_pos);
        let coefs = self.tp_coefs;
        let mut work: Vec<_> = self
            .filters
            .iter()
            .zip(&self.block_acc)
            .zip(&self.tp_hist)
            .zip(&self.tp_pos)
            .zip(io)
            .take(ch)
            .map(|((((f, acc), h), pos), x)| ChannelRun {
                filters: *f,
                acc: *acc,
                hist: *h,
                pos: *pos,
                peak: 0.0,
                blocks: Vec::new(),
                input: x.get(..frames).unwrap_or_default(),
            })
            .collect();
        let run = |w: &mut ChannelRun| {
            let mut pos = block_pos;
            let mut n = 0;
            while n < w.input.len() {
                let len = (block_len - pos).min(w.input.len() - n);
                let Some(x) = w.input.get(n..n + len) else { break };
                w.acc = k_weighted_energy(&mut w.filters, x, w.acc);
                w.peak = w.peak.max(true_peak_run(&coefs, &mut w.hist, &mut w.pos, x));
                n += len;
                pos += len;
                if pos >= block_len {
                    w.blocks.push(w.acc);
                    w.acc = 0.0;
                    pos = 0;
                }
            }
        };
        let spawned = std::thread::scope(|scope| {
            let mut rest = work.iter_mut();
            let first = rest.next();
            let handles: Vec<_> = rest.map(|w| std::thread::Builder::new().spawn_scoped(scope, || run(w))).collect();
            if let Some(w) = first {
                run(w);
            }
            let joined: Vec<bool> = handles.into_iter().map(|h| h.is_ok_and(|h| h.join().is_ok())).collect();
            joined.into_iter().all(|ok| ok)
        });
        if !spawned {
            return false;
        }
        let blocks = work.first().map_or(0, |w| w.blocks.len());
        for i in 0..blocks {
            for (acc, w) in self.block_acc.iter_mut().zip(&work) {
                *acc = w.blocks.get(i).copied().unwrap_or(0.0);
            }
            self.block_pos = block_len;
            self.finish_sub_block();
        }
        for (c, w) in work.into_iter().enumerate() {
            if let (Some(f), Some(acc), Some(h), Some(pos)) =
                (self.filters.get_mut(c), self.block_acc.get_mut(c), self.tp_hist.get_mut(c), self.tp_pos.get_mut(c))
            {
                *f = w.filters;
                *acc = w.acc;
                *h = w.hist;
                *pos = w.pos;
            }
            self.true_peak = self.true_peak.max(w.peak);
        }
        self.block_pos = (block_pos + frames) % block_len;
        true
    }

    fn finish_sub_block(&mut self) {
        let len = self.block_pos.max(1) as f64;
        let mut energy = 0.0;
        for (acc, w) in self.block_acc.iter_mut().zip(&self.weights) {
            energy += f64::from(*w) * *acc / len;
            *acc = 0.0;
        }
        self.block_pos = 0;
        if let Some(s) = self.sub.get_mut(self.sub_idx) {
            *s = energy;
        }
        self.sub_idx = (self.sub_idx + 1) % SUB_BLOCKS;
        self.sub_filled = (self.sub_filled + 1).min(SUB_BLOCKS);
        if self.sub_filled >= 4 {
            let z = self.recent_energy(4);
            let l = lufs(z);
            self.max_momentary = self.max_momentary.max(l);
            if l > HIST_MIN {
                let bin = (((l - HIST_MIN) / HIST_STEP) as usize).min(HIST_BINS - 1);
                if let (Some(c), Some(e)) = (self.hist_count.get_mut(bin), self.hist_energy.get_mut(bin)) {
                    *c += 1;
                    *e += z;
                }
            }
        }
    }

    /// Mean energy of the last `n` completed 100 ms sub-blocks.
    fn recent_energy(&self, n: usize) -> f64 {
        let n = n.min(self.sub_filled);
        if n == 0 {
            return 0.0;
        }
        let mut sum = 0.0;
        for k in 1..=n {
            let idx = (self.sub_idx + SUB_BLOCKS - k) % SUB_BLOCKS;
            sum += self.sub.get(idx).copied().unwrap_or(0.0);
        }
        sum / n as f64
    }

    /// Momentary loudness (400 ms window), LUFS. -144 until 400 ms have been measured.
    pub fn momentary_lufs(&self) -> f32 {
        if self.sub_filled < 4 { MIN_DB } else { lufs(self.recent_energy(4)) }
    }

    /// Short-term loudness (3 s window, or what is available so far), LUFS.
    pub fn short_term_lufs(&self) -> f32 {
        if self.sub_filled < 4 { MIN_DB } else { lufs(self.recent_energy(SUB_BLOCKS)) }
    }

    /// Highest momentary loudness seen.
    pub fn max_momentary_lufs(&self) -> f32 {
        self.max_momentary
    }

    /// Gated integrated loudness (absolute gate -70 LUFS, relative gate -10 LU), LUFS.
    pub fn integrated_lufs(&self) -> f32 {
        let (mut n, mut e) = (0u64, 0.0f64);
        for (c, s) in self.hist_count.iter().zip(&self.hist_energy) {
            n += c;
            e += s;
        }
        if n == 0 {
            return MIN_DB;
        }
        let gate = lufs(e / n as f64) - 10.0;
        let (mut n2, mut e2) = (0u64, 0.0f64);
        for (i, (c, s)) in self.hist_count.iter().zip(&self.hist_energy).enumerate() {
            let center = HIST_MIN + (i as f32 + 0.5) * HIST_STEP;
            if center > gate {
                n2 += c;
                e2 += s;
            }
        }
        if n2 == 0 { MIN_DB } else { lufs(e2 / n2 as f64) }
    }

    /// Maximum true peak (4× oversampled) in dBTP.
    pub fn true_peak_db(&self) -> f32 {
        gain_to_db(self.true_peak)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn noise(len: usize, seed: u64, amp: f32) -> Vec<f32> {
        let mut x = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
        (0..len)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                ((x >> 40) as f32 / (1u64 << 24) as f32 * 2.0 - 1.0) * amp
            })
            .collect()
    }

    /// True peak as first written: the history shifted every sample, each phase summed on its own.
    fn true_peak_reference(coefs: &[[f32; TP_PHASES]; TP_TAPS], x: &[f32]) -> f32 {
        let mut h = [0.0f32; TP_TAPS];
        let mut tp = 0.0f32;
        for v in x.iter().map(|v| if v.is_finite() { *v } else { 0.0 }) {
            h.copy_within(0..TP_TAPS - 1, 1);
            h[0] = v;
            let mut peak = v.abs();
            for p in 0..TP_PHASES {
                let y: f32 = coefs.iter().zip(h.iter()).map(|(c, b)| c[p] * b).sum();
                peak = peak.max(y.abs());
            }
            if peak > tp {
                tp = peak;
            }
        }
        tp
    }

    #[test]
    fn true_peak_matches_the_shifted_history() {
        let sr = 48_000.0;
        let len = 20_000;
        let mut l: Vec<f32> =
            (0..len).map(|i| 0.6 * (std::f32::consts::TAU * 0.25 * i as f32 + 0.7).sin()).zip(noise(len, 1, 0.3)).map(|(a, b)| a + b).collect();
        let r = noise(len, 2, 0.9);
        l[123] = f32::NAN;
        l[4_567] = f32::INFINITY;
        for block in [1, 7, 480, len] {
            let mut m = LoudnessMeter::new(sr, 2);
            let want = true_peak_reference(&m.tp_coefs, &l).max(true_peak_reference(&m.tp_coefs, &r));
            for at in (0..len).step_by(block) {
                let n = block.min(len - at);
                m.process(&[l[at..at + n].to_vec(), r[at..at + n].to_vec()], n);
            }
            assert_eq!(m.true_peak.to_bits(), want.to_bits(), "block {block}");
            m.reset();
            let quiet = noise(500, 3, 0.1);
            m.process(&[quiet.clone(), quiet.clone()], 500);
            assert_eq!(m.true_peak.to_bits(), true_peak_reference(&m.tp_coefs, &quiet).to_bits(), "after reset, block {block}");
        }
    }

    /// The meter as it was written before the channel-major rewrite: one frame at a time.
    fn reference(m: &mut LoudnessMeter, io: &[Vec<f32>], frames: usize) {
        let mut hist = vec![[0.0f32; TP_TAPS]; m.filters.len()];
        let ch = m.filters.len().min(io.len());
        let frames = io.iter().take(ch).map(Vec::len).fold(frames, usize::min);
        for n in 0..frames {
            for c in 0..ch {
                let x = io.get(c).and_then(|b| b.get(n)).copied().filter(|v| v.is_finite()).unwrap_or(0.0);
                if let Some([f1, f2]) = m.filters.get_mut(c) {
                    let y = f2.process_sample(f1.process_sample(x));
                    if let Some(acc) = m.block_acc.get_mut(c) {
                        *acc += f64::from(y) * f64::from(y);
                    }
                }
                if let Some(h) = hist.get_mut(c) {
                    h.copy_within(0..TP_TAPS - 1, 1);
                    h[0] = x;
                    let mut peak = x.abs();
                    for p in 0..TP_PHASES {
                        let y: f32 = m.tp_coefs.iter().zip(h.iter()).map(|(c, b)| c[p] * b).sum();
                        peak = peak.max(y.abs());
                    }
                    if peak > m.true_peak {
                        m.true_peak = peak;
                    }
                }
            }
            m.block_pos += 1;
            if m.block_pos >= m.block_len {
                m.finish_sub_block();
            }
        }
    }

    fn signal(channels: usize, frames: usize) -> Vec<Vec<f32>> {
        let mut seed = 0x2545_f491_u32;
        (0..channels)
            .map(|c| {
                (0..frames)
                    .map(|i| {
                        seed ^= seed << 13;
                        seed ^= seed >> 17;
                        seed ^= seed << 5;
                        let noise = (seed as f32 / u32::MAX as f32 - 0.5) * 0.2;
                        let speech = (i as f32 * (0.03 + c as f32 * 0.011)).sin() * (0.5 + 0.45 * (i as f32 * 1.3e-4).sin());
                        if i == 7_777 {
                            f32::NAN
                        } else if i == 9_999 {
                            f32::INFINITY
                        } else {
                            speech + noise
                        }
                    })
                    .collect()
            })
            .collect()
    }

    fn readings(m: &LoudnessMeter) -> [u32; 5] {
        [m.integrated_lufs(), m.momentary_lufs(), m.short_term_lufs(), m.max_momentary_lufs(), m.true_peak_db()].map(f32::to_bits)
    }

    fn state(m: &LoudnessMeter) -> (Vec<u64>, Vec<u64>, usize) {
        (m.block_acc.iter().map(|v| v.to_bits()).collect(), m.hist_energy.iter().map(|v| v.to_bits()).collect(), m.block_pos)
    }

    #[test]
    fn matches_the_frame_at_a_time_meter_bit_for_bit() {
        for (sr, channels) in [(48_000.0, 2), (44_100.0, 1), (96_000.0, 6)] {
            let x = signal(channels, 300_007);
            let mut want = LoudnessMeter::new(sr, channels);
            reference(&mut want, &x, 300_007);
            // One long call (threaded), and uneven small blocks (sequential).
            let mut whole = LoudnessMeter::new(sr, channels);
            whole.process(&x, 300_007);
            assert_eq!((readings(&whole), state(&whole)), (readings(&want), state(&want)), "whole, {sr} Hz x{channels}");
            let mut blocks = LoudnessMeter::new(sr, channels);
            let mut at = 0;
            for len in [1, 17, 4096, 70_000, 513].into_iter().cycle() {
                if at >= 300_007 {
                    break;
                }
                let end = (at + len).min(300_007);
                let part: Vec<Vec<f32>> = x.iter().map(|c| c[at..end].to_vec()).collect();
                blocks.process(&part, end - at);
                at = end;
            }
            assert_eq!((readings(&blocks), state(&blocks)), (readings(&want), state(&want)), "blocks, {sr} Hz x{channels}");
        }
    }

    #[test]
    fn short_and_mismatched_inputs_are_safe() {
        let mut m = LoudnessMeter::new(48_000.0, 2);
        m.process(&[], 100);
        m.process(&[vec![0.5; 10]], 1_000);
        m.process(&[vec![0.5; 100_000], vec![0.5; 70_000]], 200_000);
        let mut r = LoudnessMeter::new(48_000.0, 2);
        reference(&mut r, &[vec![0.5; 10]], 1_000);
        reference(&mut r, &[vec![0.5; 100_000], vec![0.5; 70_000]], 200_000);
        assert_eq!(readings(&m), readings(&r));
    }
}
