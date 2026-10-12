//! MPEG-1 Layer III frame layout (ISO/IEC 11172-3, 2.4.1.3 and 2.4.2.7): the frame header,
//! frame sizes and padding for constant bitrate, an MSB-first bit writer and the side
//! information.

/// Bitrates of `bitrate_index` 1..=14, in kbit/s.
const BITRATES_KBPS: [u32; 14] = [32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320];
/// Sample rates of `sampling_frequency` 0..=2.
const SAMPLE_RATES: [u32; 3] = [44_100, 48_000, 32_000];

/// Samples per channel in one frame (two granules).
pub(super) const FRAME_SAMPLES: usize = 1152;

/// The header's `bitrate_index` for a bitrate in kbit/s (free format and invalid rates are `None`).
pub(super) fn bitrate_index(kbps: u32) -> Option<u8> {
    BITRATES_KBPS.iter().position(|&b| b == kbps).and_then(|i| u8::try_from(i + 1).ok())
}

/// The header's `sampling_frequency` field (MPEG-1 rates only).
pub(super) fn sample_rate_index(rate: u32) -> Option<u8> {
    SAMPLE_RATES.iter().position(|&r| r == rate).and_then(|i| u8::try_from(i).ok())
}

/// Bytes in one frame: `144 * bitrate / sample_rate`, plus one when padded.
pub(super) fn frame_bytes(kbps: u32, rate: u32, padded: bool) -> usize {
    let base = 144_000u64 * u64::from(kbps) / u64::from(rate.max(1));
    usize::try_from(base).unwrap_or(0) + usize::from(padded)
}

/// Chooses which frames carry the padding byte so the average frame size matches the bitrate
/// exactly (only 44.1 kHz needs it).
#[derive(Default)]
pub(super) struct Padding {
    rest: u64,
}

impl Padding {
    pub(super) fn next(&mut self, kbps: u32, rate: u32) -> bool {
        let rate = u64::from(rate.max(1));
        self.rest += 144_000u64 * u64::from(kbps) % rate;
        if self.rest >= rate {
            self.rest -= rate;
            true
        } else {
            false
        }
    }
}

/// Channel mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Mode {
    Stereo = 0,
    Mono = 3,
}

/// The 4-byte frame header: MPEG-1, Layer III, no CRC, no mode extension, not copyrighted,
/// no emphasis.
pub(super) fn header(bitrate_index: u8, sample_rate_index: u8, padded: bool, mode: Mode) -> [u8; 4] {
    [0xFF, 0xFB, ((bitrate_index & 0x0F) << 4) | ((sample_rate_index & 0x03) << 2) | (u8::from(padded) << 1), (mode as u8) << 6]
}

/// MSB-first bit writer.
#[derive(Default)]
pub(super) struct BitWriter {
    pub(super) bytes: Vec<u8>,
    acc: u64,
    bits: u32,
}

impl BitWriter {
    /// Write the low `n` (at most 32) bits of `v`.
    pub(super) fn write(&mut self, v: u32, n: u32) {
        let n = n.min(32);
        if n == 0 {
            return;
        }
        self.acc = (self.acc << n) | (u64::from(v) & ((1u64 << n) - 1));
        self.bits += n;
        while self.bits >= 8 {
            self.bits -= 8;
            self.bytes.push((self.acc >> self.bits) as u8);
        }
        self.acc &= (1u64 << self.bits) - 1;
    }

    /// Bits written so far.
    pub(super) fn len_bits(&self) -> usize {
        self.bytes.len() * 8 + self.bits as usize
    }

    /// Pad with zero bits to a whole byte.
    pub(super) fn align(&mut self) {
        let pad = (8 - self.bits % 8) % 8;
        self.write(0, pad);
    }
}

/// Side information of one granule of one channel (long blocks only; no window switching).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct GranuleInfo {
    pub(super) part2_3_length: u16,
    pub(super) big_values: u16,
    pub(super) global_gain: u8,
    pub(super) scalefac_compress: u8,
    pub(super) table_select: [u8; 3],
    pub(super) region0_count: u8,
    pub(super) region1_count: u8,
    pub(super) preflag: bool,
    pub(super) scalefac_scale: bool,
    pub(super) count1table_select: bool,
}

/// Side information of one frame: `granules[gr][ch]`.
#[derive(Debug, Clone, Default)]
pub(super) struct SideInfo {
    pub(super) main_data_begin: u16,
    pub(super) scfsi: [[bool; 4]; 2],
    pub(super) granules: [[GranuleInfo; 2]; 2],
}

impl SideInfo {
    /// Bytes of side information: 17 for one channel, 32 for two.
    pub(super) fn bytes(channels: usize) -> usize {
        if channels == 1 { 17 } else { 32 }
    }

    pub(super) fn write(&self, channels: usize, w: &mut BitWriter) {
        let channels = channels.clamp(1, 2);
        w.write(u32::from(self.main_data_begin), 9);
        w.write(0, if channels == 1 { 5 } else { 3 });
        for scfsi in self.scfsi.iter().take(channels) {
            for &band in scfsi {
                w.write(u32::from(band), 1);
            }
        }
        for gr in &self.granules {
            for g in gr.iter().take(channels) {
                w.write(u32::from(g.part2_3_length), 12);
                w.write(u32::from(g.big_values), 9);
                w.write(u32::from(g.global_gain), 8);
                w.write(u32::from(g.scalefac_compress), 4);
                w.write(0, 1); // window_switching_flag
                for &t in &g.table_select {
                    w.write(u32::from(t), 5);
                }
                w.write(u32::from(g.region0_count), 4);
                w.write(u32::from(g.region1_count), 3);
                w.write(u32::from(g.preflag), 1);
                w.write(u32::from(g.scalefac_scale), 1);
                w.write(u32::from(g.count1table_select), 1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_sizes_and_padding_match_the_bitrate() {
        assert_eq!(frame_bytes(128, 44_100, false), 417);
        assert_eq!(frame_bytes(128, 48_000, false), 384);
        assert_eq!(frame_bytes(96, 32_000, false), 432);
        // Over one second of 44.1 kHz frames the padded sizes add up to the bitrate.
        for kbps in [64, 96, 128, 320] {
            let mut pad = Padding::default();
            let frames = 44_100 * 10 / FRAME_SAMPLES as u64; // 382 whole frames, ~9.98 s
            let total: u64 = (0..frames).map(|_| frame_bytes(kbps, 44_100, pad.next(kbps, 44_100)) as u64).sum();
            let exact = frames * 144_000 * u64::from(kbps) / 44_100;
            assert!(total.abs_diff(exact) <= 1, "{kbps} kbit/s: {total} vs {exact}");
        }
        let mut pad = Padding::default();
        assert!((0..100).all(|_| !pad.next(128, 48_000)), "48 kHz frames are never padded");
    }

    #[test]
    fn header_fields() {
        let br = bitrate_index(128).unwrap();
        let sr = sample_rate_index(44_100).unwrap();
        assert_eq!((br, sr), (9, 0));
        assert_eq!(header(br, sr, true, Mode::Stereo), [0xFF, 0xFB, 0x92, 0x00]);
        assert_eq!(header(bitrate_index(64).unwrap(), sample_rate_index(48_000).unwrap(), false, Mode::Mono), [0xFF, 0xFB, 0x54, 0xC0]);
        assert_eq!(bitrate_index(100), None);
        assert_eq!(sample_rate_index(22_050), None);
    }

    #[test]
    fn side_info_has_the_standard_size() {
        for channels in [1, 2] {
            let mut w = BitWriter::default();
            let mut si = SideInfo { main_data_begin: 511, ..SideInfo::default() };
            si.granules[1][channels - 1].part2_3_length = 4095;
            si.write(channels, &mut w);
            assert_eq!(w.len_bits(), SideInfo::bytes(channels) * 8);
            assert_eq!(w.bytes[0], 0xFF, "main_data_begin leads");
        }
    }

    #[test]
    fn bit_writer_packs_msb_first() {
        let mut w = BitWriter::default();
        w.write(0b101, 3);
        w.write(0x1F, 5);
        w.write(0xABCD, 16);
        w.write(1, 1);
        w.align();
        assert_eq!(w.bytes, vec![0b1011_1111, 0xAB, 0xCD, 0x80]);
    }
}
