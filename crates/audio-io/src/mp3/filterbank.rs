//! The Layer III analysis filterbank (ISO/IEC 11172-3, 2.4.3.2 and Annex C.1.5.3): the
//! 32-band polyphase filterbank, frequency inversion, an 18-line MDCT per subband with the
//! normal (long block) window, and the alias-reduction butterflies. One granule of 576 samples
//! in, 576 MDCT lines out (line `18 * subband + k`), scaled so a standard decoder reproduces
//! the input level.

use super::tables::{ALIAS_C, WINDOW_D};
use std::f64::consts::PI;

/// Samples per granule (and MDCT lines per granule).
pub(super) const GRANULE: usize = 576;
const SUBBANDS: usize = 32;
const SLOTS: usize = 18;

/// Per-channel analysis state.
pub(super) struct Analysis {
    /// The analysis window `C[i]`.
    c: Vec<f64>,
    /// Matrixing coefficients `M[k][i] = cos((2k + 1)(i - 16) pi / 64)`, row-major.
    m: Vec<f64>,
    /// MDCT coefficients with the long window folded in: `w[n] cos(pi / 72 (2n + 19)(2k + 1)) / 9`.
    mdct: Vec<f64>,
    /// Alias-reduction `(cs, ca)` pairs.
    alias: [(f64, f64); 8],
    /// The last 512 input samples, newest first (`X[0]` is the newest).
    x: Vec<f64>,
    /// The previous granule's subband samples (`[subband][slot]`), the first half of each MDCT.
    prev: Vec<[f64; SLOTS]>,
}

impl Analysis {
    pub(super) fn new() -> Self {
        let c = WINDOW_D.iter().map(|&d| f64::from(d) / 65536.0 / 32.0).collect();
        let mut m = Vec::with_capacity(SUBBANDS * 64);
        for k in 0..SUBBANDS {
            for i in 0..64 {
                m.push(((2 * k + 1) as f64 * (i as f64 - 16.0) * PI / 64.0).cos());
            }
        }
        let mut mdct = Vec::with_capacity(SLOTS * 36);
        for k in 0..SLOTS {
            for n in 0..36 {
                let w = (PI / 36.0 * (n as f64 + 0.5)).sin();
                mdct.push(w * (PI / 72.0 * (2 * n + 1 + 18) as f64 * (2 * k + 1) as f64).cos() / 9.0);
            }
        }
        let alias = ALIAS_C.map(|ci| {
            let r = (1.0 + ci * ci).sqrt();
            (1.0 / r, ci / r)
        });
        Self { c, m, mdct, alias, x: vec![0.0; 512], prev: vec![[0.0; SLOTS]; SUBBANDS] }
    }

    /// Feed 32 new samples (oldest first) through the polyphase filterbank: 32 subband samples.
    fn polyphase(&mut self, input: &[f32], out: &mut [f64; SUBBANDS]) {
        self.x.copy_within(0..512 - 32, 32);
        for (i, s) in input.iter().take(32).enumerate() {
            if let Some(x) = self.x.get_mut(31 - i) {
                *x = f64::from(*s);
            }
        }
        let mut y = [0.0f64; 64];
        for (i, yi) in y.iter_mut().enumerate() {
            *yi = (0..8).map(|j| i + 64 * j).map(|n| self.c.get(n).copied().unwrap_or(0.0) * self.x.get(n).copied().unwrap_or(0.0)).sum();
        }
        for (k, o) in out.iter_mut().enumerate() {
            let row = self.m.get(64 * k..64 * k + 64).unwrap_or(&[]);
            *o = row.iter().zip(y.iter()).map(|(a, b)| a * b).sum();
        }
    }

    /// Analyse one granule (`GRANULE` samples; a short slice is padded with silence) into
    /// `GRANULE` MDCT lines.
    pub(super) fn granule(&mut self, pcm: &[f32], lines: &mut [f64; GRANULE]) {
        let mut cur = vec![[0.0f64; SLOTS]; SUBBANDS];
        let mut block = [0.0f32; 32];
        let mut sub = [0.0f64; SUBBANDS];
        for t in 0..SLOTS {
            for (i, b) in block.iter_mut().enumerate() {
                *b = pcm.get(32 * t + i).copied().unwrap_or(0.0);
            }
            self.polyphase(&block, &mut sub);
            for (sb, v) in sub.iter().enumerate() {
                if let Some(c) = cur.get_mut(sb).and_then(|c| c.get_mut(t)) {
                    // Frequency inversion: odd time samples of odd subbands change sign.
                    *c = if sb % 2 == 1 && t % 2 == 1 { -v } else { *v };
                }
            }
        }
        for (sb, (p, c)) in self.prev.iter().zip(cur.iter()).enumerate() {
            let z: Vec<f64> = p.iter().chain(c.iter()).copied().collect();
            for k in 0..SLOTS {
                let row = self.mdct.get(36 * k..36 * k + 36).unwrap_or(&[]);
                if let Some(l) = lines.get_mut(SLOTS * sb + k) {
                    *l = row.iter().zip(z.iter()).map(|(a, b)| a * b).sum();
                }
            }
        }
        self.prev = cur;
        // Alias reduction across each subband boundary.
        for sb in 1..SUBBANDS {
            for (i, &(cs, ca)) in self.alias.iter().enumerate() {
                let (lo_i, hi_i) = (SLOTS * sb - 1 - i, SLOTS * sb + i);
                let (Some(&lo), Some(&hi)) = (lines.get(lo_i), lines.get(hi_i)) else { continue };
                if let Some(l) = lines.get_mut(lo_i) {
                    *l = lo * cs + hi * ca;
                }
                if let Some(h) = lines.get_mut(hi_i) {
                    *h = hi * cs - lo * ca;
                }
            }
        }
    }
}

#[cfg(test)]
// The reference decoder keeps the standard's index notation.
#[allow(clippy::needless_range_loop)]
mod tests {
    use super::*;

    /// A reference Layer III decoder back end (alias butterflies, IMDCT, overlap-add, frequency
    /// inversion, polyphase synthesis) written from the standard's decoding process.
    struct Synthesis {
        overlap: Vec<[f64; SLOTS]>,
        v: Vec<f64>,
        alias: [(f64, f64); 8],
    }

    impl Synthesis {
        fn new() -> Self {
            let alias = ALIAS_C.map(|ci| {
                let r = (1.0 + ci * ci).sqrt();
                (1.0 / r, ci / r)
            });
            Self { overlap: vec![[0.0; SLOTS]; SUBBANDS], v: vec![0.0; 1024], alias }
        }

        fn granule(&mut self, lines: &[f64; GRANULE], out: &mut Vec<f32>) {
            let mut xr = *lines;
            for sb in 1..SUBBANDS {
                for (i, &(cs, ca)) in self.alias.iter().enumerate() {
                    let (lo, hi) = (xr[SLOTS * sb - 1 - i], xr[SLOTS * sb + i]);
                    xr[SLOTS * sb - 1 - i] = lo * cs - hi * ca;
                    xr[SLOTS * sb + i] = hi * cs + lo * ca;
                }
            }
            let mut sub = vec![[0.0f64; SLOTS]; SUBBANDS];
            for sb in 0..SUBBANDS {
                let mut y = [0.0f64; 36];
                for (n, yn) in y.iter_mut().enumerate() {
                    let w = (PI / 36.0 * (n as f64 + 0.5)).sin();
                    *yn = w * (0..SLOTS).map(|k| xr[SLOTS * sb + k] * (PI / 72.0 * (2 * n + 1 + 18) as f64 * (2 * k + 1) as f64).cos()).sum::<f64>();
                }
                for t in 0..SLOTS {
                    let v = y[t] + self.overlap[sb][t];
                    sub[sb][t] = if sb % 2 == 1 && t % 2 == 1 { -v } else { v };
                    self.overlap[sb][t] = y[t + 18];
                }
            }
            for t in 0..SLOTS {
                self.v.copy_within(0..1024 - 64, 64);
                for i in 0..64 {
                    self.v[i] = (0..SUBBANDS).map(|k| ((16 + i) as f64 * (2 * k + 1) as f64 * PI / 64.0).cos() * sub[k][t]).sum();
                }
                let mut u = [0.0f64; 512];
                for i in 0..8 {
                    for j in 0..32 {
                        u[i * 64 + j] = self.v[i * 128 + j];
                        u[i * 64 + 32 + j] = self.v[i * 128 + 96 + j];
                    }
                }
                for j in 0..32 {
                    let s: f64 = (0..16).map(|i| u[j + 32 * i] * f64::from(WINDOW_D[j + 32 * i]) / 65536.0).sum();
                    out.push(s as f32);
                }
            }
        }
    }

    fn noise(n: usize) -> Vec<f32> {
        let mut s = 0x1234_5678u32;
        (0..n)
            .map(|i| {
                s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let r = (s >> 8) as f32 / (1u32 << 24) as f32 - 0.5;
                0.4 * r + 0.3 * (i as f32 * 0.05).sin()
            })
            .collect()
    }

    #[test]
    fn window_sums_to_unity_gain() {
        // The prototype lowpass (D with every other 64-block negated) has a DC gain of 64.
        let sum: i64 = WINDOW_D.iter().enumerate().map(|(n, &d)| if (n / 64) % 2 == 1 { -i64::from(d) } else { i64::from(d) }).sum();
        assert!((sum - 64 * 65536).abs() < 256, "window sum {sum}");
    }

    #[test]
    fn analysis_then_standard_synthesis_reconstructs_the_input() {
        let input = noise(GRANULE * 40);
        let mut ana = Analysis::new();
        let mut syn = Synthesis::new();
        let mut out = Vec::new();
        let mut lines = [0.0f64; GRANULE];
        for g in input.chunks(GRANULE) {
            ana.granule(g, &mut lines);
            syn.granule(&lines, &mut out);
        }
        // Find the codec delay, then compare.
        let span = 2000..input.len() - 3000;
        let err = |d: usize| -> f64 { span.clone().map(|i| (f64::from(out[i + d]) - f64::from(input[i])).powi(2)).sum() };
        let delay = (0..2000).min_by(|&a, &b| err(a).total_cmp(&err(b))).unwrap_or(0);
        let sig: f64 = span.clone().map(|i| f64::from(input[i]).powi(2)).sum();
        let snr = 10.0 * (sig / err(delay)).log10();
        assert!(snr > 60.0, "reconstruction SNR {snr:.1} dB at delay {delay}");
    }
}
