//! Byte-level PCM helpers shared by the WAV, AIFF and FLAC code: bounded readers, sample
//! decoding to `f32`, and quantization (with optional TPDF dither) for encoding.

use crate::{AudioError, BitDepth, Result};
use std::io::Write;

/// Maximum number of decoded `f32` samples (all channels) we will produce: 4 GiB of output.
pub(crate) const MAX_DECODE_SAMPLES: u64 = (4u64 << 30) / 4;

/// Little-endian `u16` at `pos`.
pub(crate) fn le_u16(b: &[u8], pos: usize) -> Option<u16> {
    let s = b.get(pos..pos.checked_add(2)?)?;
    Some(u16::from_le_bytes([*s.first()?, *s.get(1)?]))
}

/// Little-endian `u32` at `pos`.
pub(crate) fn le_u32(b: &[u8], pos: usize) -> Option<u32> {
    let s: [u8; 4] = b.get(pos..pos.checked_add(4)?)?.try_into().ok()?;
    Some(u32::from_le_bytes(s))
}

/// Little-endian `u64` at `pos`.
pub(crate) fn le_u64(b: &[u8], pos: usize) -> Option<u64> {
    let s: [u8; 8] = b.get(pos..pos.checked_add(8)?)?.try_into().ok()?;
    Some(u64::from_le_bytes(s))
}

/// Big-endian `u16` at `pos`.
pub(crate) fn be_u16(b: &[u8], pos: usize) -> Option<u16> {
    let s: [u8; 2] = b.get(pos..pos.checked_add(2)?)?.try_into().ok()?;
    Some(u16::from_be_bytes(s))
}

/// Big-endian `u32` at `pos`.
pub(crate) fn be_u32(b: &[u8], pos: usize) -> Option<u32> {
    let s: [u8; 4] = b.get(pos..pos.checked_add(4)?)?.try_into().ok()?;
    Some(u32::from_be_bytes(s))
}

/// Big-endian `u64` at `pos`.
pub(crate) fn be_u64(b: &[u8], pos: usize) -> Option<u64> {
    let s: [u8; 8] = b.get(pos..pos.checked_add(8)?)?.try_into().ok()?;
    Some(u64::from_be_bytes(s))
}

/// Four-byte tag at `pos`.
pub(crate) fn tag(b: &[u8], pos: usize) -> Option<[u8; 4]> {
    b.get(pos..pos.checked_add(4)?)?.try_into().ok()
}

/// How the samples in a PCM byte stream are encoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PcmKind {
    /// Signed (or, for 1-byte WAV, unsigned) integers occupying `bytes` bytes each, left-justified.
    Int { bytes: usize, big_endian: bool, unsigned: bool },
    /// IEEE float, 4 or 8 bytes.
    Float { bytes: usize, big_endian: bool },
}

impl PcmKind {
    pub(crate) fn bytes(self) -> usize {
        match self {
            PcmKind::Int { bytes, .. } | PcmKind::Float { bytes, .. } => bytes,
        }
    }
}

/// Decode one sample from exactly `kind.bytes()` bytes. Returns 0 for a short slice.
fn decode_sample(s: &[u8], kind: PcmKind) -> f32 {
    match kind {
        PcmKind::Int { bytes, big_endian, unsigned } => {
            if bytes == 0 || bytes > 4 || s.len() < bytes {
                return 0.0;
            }
            let mut acc: u32 = 0;
            for i in 0..bytes {
                let idx = if big_endian { i } else { bytes - 1 - i };
                let mut byte = s.get(idx).copied().unwrap_or(0);
                if unsigned && i == 0 {
                    byte ^= 0x80;
                }
                acc = (acc << 8) | u32::from(byte);
            }
            let shifted = acc << (32 - 8 * bytes as u32);
            (f64::from(shifted as i32) / 2_147_483_648.0) as f32
        }
        PcmKind::Float { bytes, big_endian } => {
            let v = match bytes {
                4 => {
                    let Some(a) = s.get(..4).and_then(|x| <[u8; 4]>::try_from(x).ok()) else { return 0.0 };
                    if big_endian { f32::from_be_bytes(a) } else { f32::from_le_bytes(a) }
                }
                8 => {
                    let Some(a) = s.get(..8).and_then(|x| <[u8; 8]>::try_from(x).ok()) else { return 0.0 };
                    (if big_endian { f64::from_be_bytes(a) } else { f64::from_le_bytes(a) }) as f32
                }
                _ => 0.0,
            };
            if v.is_finite() { v } else { 0.0 }
        }
    }
}

/// Deinterleave `frames` frames of `channels`-channel PCM from `data` into planar `f32`.
pub(crate) fn deinterleave(data: &[u8], kind: PcmKind, channels: usize, frames: u64) -> Result<Vec<Vec<f32>>> {
    let bytes = kind.bytes();
    let block = bytes.checked_mul(channels).filter(|&b| b > 0).ok_or_else(|| AudioError::Malformed("zero-sized sample frame".into()))?;
    let available = (data.len() / block) as u64;
    let frames = frames.min(available);
    let total = frames.checked_mul(channels as u64).ok_or_else(|| AudioError::TooLarge("sample count overflows".into()))?;
    if total > MAX_DECODE_SAMPLES {
        return Err(AudioError::TooLarge(format!("{total} samples exceeds the {MAX_DECODE_SAMPLES}-sample decode limit")));
    }
    let frames = usize::try_from(frames).map_err(|_| AudioError::TooLarge("frame count exceeds address space".into()))?;
    let mut out: Vec<Vec<f32>> = (0..channels).map(|_| vec![0.0; frames]).collect();
    let data = data.get(..frames.saturating_mul(block)).unwrap_or(data);
    let threads = decode_threads(frames.saturating_mul(channels));
    if threads <= 1 {
        deinterleave_into(data, kind, block, out.iter_mut().map(Vec::as_mut_slice).collect());
        return Ok(out);
    }
    // Split the frames into one contiguous run per thread; every thread writes its own part of
    // every channel, so the result is identical to the serial loop.
    let run = frames.div_ceil(threads).max(1);
    let mut parts: Vec<Vec<&mut [f32]>> = Vec::new();
    for ch in out.iter_mut() {
        for (i, piece) in ch.chunks_mut(run).enumerate() {
            if parts.len() <= i {
                parts.push(Vec::with_capacity(channels));
            }
            if let Some(p) = parts.get_mut(i) {
                p.push(piece);
            }
        }
    }
    // `Scope::spawn` panics when the OS refuses a thread; a run that got none is decoded here
    // afterwards instead.
    let mut unspawned = Vec::new();
    std::thread::scope(|scope| {
        for (i, (chunk, part)) in data.chunks(run.saturating_mul(block)).zip(parts).enumerate() {
            if std::thread::Builder::new().spawn_scoped(scope, move || deinterleave_into(chunk, kind, block, part)).is_err() {
                unspawned.push(i);
            }
        }
    });
    for i in unspawned {
        let first = i.saturating_mul(run);
        let chunk = data.get(first.saturating_mul(block)..).unwrap_or_default();
        let chunk = chunk.get(..run.saturating_mul(block)).unwrap_or(chunk);
        let end = first.saturating_add(run).min(frames);
        let part = out.iter_mut().filter_map(|ch| ch.get_mut(first..end)).collect();
        deinterleave_into(chunk, kind, block, part);
    }
    Ok(out)
}

/// Decode whole frames of `data` into `out` (one slice per channel, all the same length).
fn deinterleave_into(data: &[u8], kind: PcmKind, block: usize, mut out: Vec<&mut [f32]>) {
    let bytes = kind.bytes();
    for (f, frame) in data.chunks_exact(block).enumerate() {
        for (ch, s) in out.iter_mut().zip(frame.chunks_exact(bytes)) {
            if let Some(d) = ch.get_mut(f) {
                *d = decode_sample(s, kind);
            }
        }
    }
}

/// Worker threads for decoding `samples` samples: one for short files and where threads aren't
/// available (wasm), otherwise up to the machine's parallelism (at most 16).
fn decode_threads(samples: usize) -> usize {
    if cfg!(target_arch = "wasm32") || samples < 1 << 20 {
        return 1;
    }
    std::thread::available_parallelism().map_or(1, |n| n.get().min(16))
}

/// Converts `f32` samples to integers of a given bit depth, clamping to [-1, 1] and optionally
/// adding triangular (TPDF) dither of ±1 LSB.
pub(crate) struct Quantizer {
    bits: u32,
    dither: bool,
    rng: u64,
}

impl Quantizer {
    /// A quantizer for one channel; each channel gets an independent dither sequence.
    pub(crate) fn new(bits: u32, dither: bool, channel: usize) -> Self {
        let rng = 0x9E37_79B9_7F4A_7C15u64 ^ (channel as u64).wrapping_add(1).wrapping_mul(0xD1B5_4A32_D192_ED03);
        Self { bits: bits.clamp(2, 32), dither, rng: if rng == 0 { 1 } else { rng } }
    }

    /// One quantizer per channel.
    pub(crate) fn per_channel(bits: u32, dither: bool, channels: usize) -> Vec<Self> {
        (0..channels).map(|c| Self::new(bits, dither, c)).collect()
    }

    /// Uniform in [0, 1) from a xorshift64* generator (deterministic, no OS entropy needed).
    fn uniform(&mut self) -> f64 {
        let mut x = self.rng;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.rng = x;
        (x.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 11) as f64 / (1u64 << 53) as f64
    }

    pub(crate) fn quantize(&mut self, x: f32) -> i32 {
        let x = if x.is_nan() { 0.0 } else { x.clamp(-1.0, 1.0) };
        let scale = (1u64 << (self.bits - 1)) as f64;
        let mut v = f64::from(x) * scale;
        if self.dither {
            v += self.uniform() - self.uniform();
        }
        let max = scale - 1.0;
        v.round().clamp(-scale, max) as i32
    }
}

/// Bits per sample at `depth`, and whether they are float.
pub(crate) fn sample_format(depth: BitDepth) -> (u16, bool) {
    match depth {
        BitDepth::Int16 => (16, false),
        BitDepth::Int24 => (24, false),
        BitDepth::Int32 => (32, false),
        BitDepth::Float32 => (32, true),
    }
}

/// Interleaves `frames` frames of planar audio (missing samples are silence) as `bits`-bit
/// samples and writes them to `out`, a few thousand frames at a time through `scratch`.
pub(crate) fn write_interleaved(
    out: &mut impl Write,
    scratch: &mut Vec<u8>,
    block: &[Vec<f32>],
    frames: usize,
    quantizers: &mut [Quantizer],
    (bits, is_float): (u16, bool),
    big_endian: bool,
) -> std::io::Result<()> {
    let size = usize::from(bits / 8);
    let mut start = 0;
    while start < frames {
        let end = frames.min(start + 4096);
        scratch.clear();
        for f in start..end {
            for (c, q) in quantizers.iter_mut().enumerate() {
                let s = block.get(c).and_then(|ch| ch.get(f)).copied().unwrap_or(0.0);
                match (is_float, big_endian) {
                    (true, false) => scratch.extend_from_slice(&sanitize_float(s).to_le_bytes()),
                    (true, true) => scratch.extend_from_slice(&sanitize_float(s).to_be_bytes()),
                    (false, false) => scratch.extend_from_slice(q.quantize(s).to_le_bytes().get(..size).unwrap_or_default()),
                    (false, true) => scratch.extend_from_slice(q.quantize(s).to_be_bytes().get(4 - size..).unwrap_or_default()),
                }
            }
        }
        out.write_all(scratch)?;
        start = end;
    }
    Ok(())
}

/// Preserve finite float samples, mapping NaN to 0 and infinities to their sign (±1).
fn sanitize_float(x: f32) -> f32 {
    if x.is_finite() {
        x
    } else if x.is_nan() {
        0.0
    } else {
        x.signum()
    }
}

/// Check that a buffer is encodable: at least one channel and a non-zero sample rate.
pub(crate) fn validate_buffer(buf: &crate::AudioBuffer) -> Result<()> {
    validate_layout(buf.channels.len(), buf.sample_rate)
}

/// Check that a stream is encodable: at least one channel and a non-zero sample rate.
pub(crate) fn validate_layout(channels: usize, sample_rate: u32) -> Result<()> {
    if channels == 0 {
        return Err(AudioError::Encode("buffer has no channels".into()));
    }
    if channels > usize::from(u16::MAX) {
        return Err(AudioError::Encode("too many channels".into()));
    }
    if sample_rate == 0 {
        return Err(AudioError::Encode("sample rate is 0".into()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The threaded decode of a long file is identical to a frame-by-frame serial decode, for
    /// every sample format, including a trailing partial frame and a frame count shorter than
    /// the data.
    #[test]
    fn parallel_deinterleave_matches_serial() {
        let kinds = [
            PcmKind::Int { bytes: 1, big_endian: false, unsigned: true },
            PcmKind::Int { bytes: 2, big_endian: false, unsigned: false },
            PcmKind::Int { bytes: 3, big_endian: true, unsigned: false },
            PcmKind::Float { bytes: 4, big_endian: false },
            PcmKind::Float { bytes: 8, big_endian: true },
        ];
        for kind in kinds {
            for channels in [1usize, 2, 3] {
                let frames = (1usize << 20) / channels + 777;
                let block = kind.bytes() * channels;
                let mut x = 0x2545_f491u32;
                let data: Vec<u8> = (0..frames * block + 5)
                    .map(|_| {
                        x ^= x << 13;
                        x ^= x >> 17;
                        x ^= x << 5;
                        (x >> 24) as u8
                    })
                    .collect();
                for want in [frames as u64, frames as u64 - 1000, u64::MAX] {
                    let got = deinterleave(&data, kind, channels, want).unwrap();
                    let n = want.min((data.len() / block) as u64) as usize;
                    let mut serial: Vec<Vec<f32>> = (0..channels).map(|_| Vec::new()).collect();
                    for frame in data.chunks_exact(block).take(n) {
                        for (ch, s) in serial.iter_mut().zip(frame.chunks_exact(kind.bytes())) {
                            ch.push(decode_sample(s, kind));
                        }
                    }
                    // Compare bit patterns so NaNs from random float bytes compare equal.
                    let bits = |v: &Vec<Vec<f32>>| v.iter().map(|c| c.iter().map(|x| x.to_bits()).collect::<Vec<_>>()).collect::<Vec<_>>();
                    assert_eq!(bits(&got), bits(&serial), "{kind:?} x{channels} frames {want}");
                }
            }
        }
    }
}
