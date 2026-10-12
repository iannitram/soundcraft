//! Layer III nonlinear quantization (ISO/IEC 11172-3, 2.4.3.4 and Annex C.1.5.4): MDCT lines
//! become integers `ix = nint((|xr| / step)^(3/4) - 0.0946)` with `step = 2^((global_gain - 210) / 4)`;
//! a decoder restores `|ix|^(4/3) * step`.

/// The largest magnitude the Huffman tables can code: 15 plus 13 linbits.
pub(super) const MAX_IX: i32 = 15 + (1 << 13) - 1;

fn step(global_gain: u8) -> f64 {
    2f64.powf((f64::from(global_gain) - 210.0) / 4.0)
}

/// Quantize `xr` with `global_gain` (no scalefactors) into `ix`; returns the largest magnitude.
pub(super) fn quantize(xr: &[f64], global_gain: u8, ix: &mut [i32]) -> i32 {
    let inv = 1.0 / step(global_gain);
    let mut max = 0;
    for (x, q) in xr.iter().zip(ix.iter_mut()) {
        let a = (x.abs() * inv).powf(0.75) + 0.4054;
        let m = if a.is_finite() { a.min(f64::from(i32::MAX)) as i32 } else { 0 };
        *q = if *x < 0.0 { -m } else { m };
        max = max.max(m);
    }
    max
}

/// What a decoder reconstructs from `ix` and `global_gain`.
pub(super) fn dequantize(ix: i32, global_gain: u8) -> f64 {
    let m = f64::from(ix.unsigned_abs()).powf(4.0 / 3.0) * step(global_gain);
    if ix < 0 { -m } else { m }
}

/// The smallest global gain (finest step) whose quantized values all fit the Huffman tables.
pub(super) fn min_global_gain(xr: &[f64], scratch: &mut [i32]) -> u8 {
    let (mut lo, mut hi) = (0u8, 255u8);
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        if quantize(xr, mid, scratch) <= MAX_IX { hi = mid } else { lo = mid + 1 }
    }
    lo
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dequantized_values_stay_within_one_step() {
        let xr: Vec<f64> = (0..576).map(|i| ((i as f64) * 0.37).sin() * 0.8 / (1.0 + i as f64 / 40.0)).collect();
        let mut ix = vec![0; 576];
        for gain in [140u8, 170, 200] {
            quantize(&xr, gain, &mut ix);
            for (&x, &q) in xr.iter().zip(&ix) {
                let back = dequantize(q, gain);
                assert!(x * back >= 0.0, "sign kept");
                // Error is under one quantization interval at that level.
                let upper = dequantize(q.abs() + 1, gain).abs() - back.abs();
                assert!((x - back).abs() <= upper, "gain {gain}: {x} -> {q} -> {back}");
            }
        }
    }

    #[test]
    fn min_global_gain_fits_the_tables() {
        let xr: Vec<f64> = (0..576).map(|i| if i == 3 { 1.0 } else { 0.01 }).collect();
        let mut ix = vec![0; 576];
        let g = min_global_gain(&xr, &mut ix);
        assert!(quantize(&xr, g, &mut ix) <= MAX_IX);
        assert!(quantize(&xr, g - 1, &mut ix) > MAX_IX);
        assert_eq!(min_global_gain(&[0.0; 576], &mut ix), 0);
    }
}
