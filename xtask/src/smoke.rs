//! `cargo xtask smoke`: a headless round trip through `soundcraft-cli` (docs/gaps.md G-14).
//!
//! Writes a 2 s, 48 kHz, stereo, 24-bit 440 Hz sine at -12 dBFS, imports it and bounces the mix
//! with `soundcraft-cli run`, then checks the bounce is 2 s long, stereo and at the same level.

use crate::{cargo_bin, root};
use soundcraft_audio_io::{AudioBuffer, EncodeOptions, decode, encode};
use std::path::Path;
use std::process::Command;

const RATE: u32 = 48_000;
const FRAMES: i64 = 96_000;
const BLOCK: i64 = 512;

pub fn run() -> Result<(), String> {
    let dir = std::env::temp_dir().join(format!("soundcraft-smoke-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| format!("smoke: {e}"))?;
    let result = smoke(&dir);
    let _ = std::fs::remove_dir_all(&dir);
    result?;
    println!("smoke: ok");
    Ok(())
}

fn smoke(dir: &Path) -> Result<(), String> {
    let tone = dir.join("tone.wav");
    let out = dir.join("out.wav");

    let amp = 10f32.powf(-12.0 / 20.0);
    let sine: Vec<f32> = (0..FRAMES).map(|i| (std::f32::consts::TAU * 440.0 * i as f32 / RATE as f32).sin() * amp).collect();
    let buf = AudioBuffer { sample_rate: RATE, channels: vec![sine.clone(), sine] };
    let bytes = encode(&buf, &EncodeOptions::default()).map_err(|e| format!("smoke: encoding tone.wav: {e}"))?;
    std::fs::write(&tone, bytes).map_err(|e| format!("smoke: writing tone.wav: {e}"))?;

    let import = serde_json::json!({ "path": tone.to_string_lossy() });
    let status = Command::new(cargo_bin())
        .args(["run", "-q", "-p", "soundcraft-cli", "--release", "--", "run", "--cmd"])
        .arg(format!("file.import_audio={import}"))
        .arg("--bounce")
        .arg(&out)
        .current_dir(root())
        .status()
        .map_err(|e| format!("smoke: starting soundcraft-cli: {e}"))?;
    if !status.success() {
        return Err("smoke: soundcraft-cli run failed".into());
    }

    let bytes = std::fs::read(&out).map_err(|e| format!("smoke: reading out.wav: {e}"))?;
    let (_, got) = decode(&bytes, Some("wav")).map_err(|e| format!("smoke: decoding out.wav: {e}"))?;
    if got.channels.len() != 2 {
        return Err(format!("smoke: out.wav has {} channels, want 2", got.channels.len()));
    }
    let frames = got.channels[0].len() as i64;
    if (frames - FRAMES).abs() > BLOCK {
        return Err(format!("smoke: out.wav is {frames} frames, want {FRAMES} ± {BLOCK}"));
    }
    let peak = got.channels.iter().flatten().fold(0f32, |m, s| m.max(s.abs()));
    let db = 20.0 * peak.log10();
    if !(-13.0..=-11.0).contains(&db) {
        return Err(format!("smoke: out.wav peaks at {db:.2} dBFS, want -13 to -11"));
    }
    Ok(())
}
