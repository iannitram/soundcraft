//! Offline performance benchmark on synthetic audio sized like a long spoken-word episode.
//!
//! `cargo run --release -p soundcraft-engine --example bench -- [--minutes 60] [--rate 48000]
//! [--only name,name] [--json]`
//!
//! Generates `--minutes` of 32-bit float stereo (speech-like: noise bursts shaped by a syllable
//! envelope over a low noise floor, deterministic), then times each stage once: WAV encode and
//! decode, FLAC encode, waveform peaks, a full-file STFT, integrated loudness, an offline EQ
//! render, a one-track bounce, and 44.1 → 48 kHz sample-rate conversion (what importing a
//! 44.1 kHz file into a 48 kHz session runs). Numbers are wall-clock and machine-specific: compare
//! runs on the same machine only.

use std::sync::Arc;
use std::time::{Duration, Instant};

use soundcraft_audio_io::{AudioBuffer, BitDepth, EncodeOptions, FileFormat, peaks::Peaks};
use soundcraft_dsp::meter::LoudnessMeter;
use soundcraft_model::{ChannelFormat, Clip, Session, SourceAudio, SourceId, TrackKind};
use soundcraft_time::Range;

const STAGES: &[&str] = &["wav_encode", "wav_decode", "flac_encode", "peaks", "stft", "loudness", "eq_render", "bounce", "resample"];

struct Args {
    minutes: f64,
    rate: u32,
    only: Option<Vec<String>>,
    json: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut a = Args { minutes: 60.0, rate: 48_000, only: None, json: false };
    let mut it = std::env::args().skip(1);
    while let Some(k) = it.next() {
        match k.as_str() {
            "--minutes" => a.minutes = it.next().and_then(|v| v.parse().ok()).filter(|m: &f64| *m > 0.0 && *m <= 240.0).ok_or("--minutes 0..240")?,
            "--rate" => {
                a.rate = it.next().and_then(|v| v.parse().ok()).filter(|r: &u32| (8_000..=384_000).contains(r)).ok_or("--rate 8000..384000")?
            }
            "--only" => a.only = Some(it.next().ok_or("--only name,name")?.split(',').map(str::to_string).collect()),
            "--json" => a.json = true,
            other => return Err(format!("unknown argument `{other}` (stages: {})", STAGES.join(", "))),
        }
    }
    Ok(a)
}

/// Deterministic speech-like stereo: syllable-rate bursts of filtered noise over a -60 dBFS floor.
fn synth(rate: u32, frames: usize) -> AudioBuffer {
    let mut seed = 0x2545_F491_4F6C_DD1Du64;
    let mut noise = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed >> 40) as f32 / (1u64 << 24) as f32 * 2.0 - 1.0
    };
    let sr = rate as f32;
    let (mut lp_l, mut lp_r) = (0.0f32, 0.0f32);
    let mut left = Vec::with_capacity(frames);
    let mut right = Vec::with_capacity(frames);
    for i in 0..frames {
        let t = i as f32 / sr;
        // ~4 syllables a second, with pauses every few seconds.
        let syllable = (std::f32::consts::PI * 4.0 * t).sin().max(0.0);
        let phrase = if (t % 7.0) < 5.5 { 1.0 } else { 0.0 };
        let env = 0.35 * syllable * syllable * phrase + 0.001;
        lp_l += 0.25 * (noise() - lp_l);
        lp_r += 0.25 * (noise() - lp_r);
        left.push(lp_l * env * 3.0);
        right.push((0.8 * lp_l + 0.2 * lp_r) * env * 3.0);
    }
    AudioBuffer { sample_rate: rate, channels: vec![left, right] }
}

fn main() {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("bench: {e}");
            std::process::exit(2);
        }
    };
    let want = |name: &str| args.only.as_ref().is_none_or(|o| o.iter().any(|n| n == name));
    let frames = (args.minutes * 60.0 * f64::from(args.rate)) as usize;
    let t = Instant::now();
    let buf = synth(args.rate, frames);
    let audio_secs = frames as f64 / f64::from(args.rate);
    if !args.json {
        println!("synthetic {:.1} min, {} Hz, 2 ch float ({} frames) generated in {:.2?}", args.minutes, args.rate, frames, t.elapsed());
        println!("{:<12} {:>10} {:>10}  note", "stage", "seconds", "x realtime");
    }
    let mut results: Vec<(String, Duration, String)> = Vec::new();
    let mut record = |name: &str, d: Duration, note: String| {
        if !args.json {
            println!("{:<12} {:>10.3} {:>10.1}  {}", name, d.as_secs_f64(), audio_secs / d.as_secs_f64().max(1e-9), note);
        }
        results.push((name.to_string(), d, note));
    };

    let wav_opts = EncodeOptions { format: FileFormat::Wav, bit_depth: BitDepth::Float32, dither: false, bwf: None };
    let wav = if want("wav_encode") || want("wav_decode") {
        let t = Instant::now();
        let bytes = soundcraft_audio_io::encode(&buf, &wav_opts).unwrap_or_default();
        record("wav_encode", t.elapsed(), format!("{} MB", bytes.len() / 1_000_000));
        bytes
    } else {
        Vec::new()
    };
    if want("wav_decode") {
        let t = Instant::now();
        let frames_back = soundcraft_audio_io::decode(&wav, Some("wav")).map_or(0, |(_, b)| b.frames());
        record("wav_decode", t.elapsed(), format!("{frames_back} frames"));
    }
    drop(wav);
    if want("flac_encode") {
        let opts = EncodeOptions { format: FileFormat::Flac, bit_depth: BitDepth::Int24, dither: false, bwf: None };
        let t = Instant::now();
        let n = soundcraft_audio_io::encode(&buf, &opts).map_or(0, |b| b.len());
        record("flac_encode", t.elapsed(), format!("24-bit, {} MB", n / 1_000_000));
    }
    if want("peaks") {
        let t = Instant::now();
        let levels: usize = buf.channels.iter().map(|c| Peaks::build(c).num_levels()).sum();
        record("peaks", t.elapsed(), format!("{levels} levels over 2 ch"));
    }
    if want("stft") {
        // A spectrogram's worth of FFTs: 2048-point, hop 512, left channel.
        let (n, hop) = (2048usize, 512usize);
        let ch = buf.channels.first().map_or(&[][..], Vec::as_slice);
        let t = Instant::now();
        let mut columns = 0usize;
        let mut acc = 0.0f32;
        let mut start = 0usize;
        while start + n <= ch.len() {
            let m = soundcraft_dsp::spectrum::magnitude_db(ch.get(start..start + n).unwrap_or_default(), n);
            acc += m.get(64).copied().unwrap_or(0.0);
            columns += 1;
            start += hop;
        }
        record("stft", t.elapsed(), format!("{columns} columns of 2048 (mono), check {:.1}", acc / columns.max(1) as f32));
    }
    if want("loudness") {
        let t = Instant::now();
        let mut m = LoudnessMeter::new(args.rate as f32, 2);
        let block = 4096usize;
        let mut tmp = vec![vec![0.0f32; block]; 2];
        let mut s = 0usize;
        while s < frames {
            let e = (s + block).min(frames);
            for (d, c) in tmp.iter_mut().zip(&buf.channels) {
                d.clear();
                d.extend_from_slice(c.get(s..e).unwrap_or_default());
            }
            m.process(&tmp, e - s);
            s = e;
        }
        record("loudness", t.elapsed(), format!("{:.2} LUFS, {:.2} dBTP", m.integrated_lufs(), m.true_peak_db()));
    }
    if want("eq_render") {
        let mut ch = buf.channels.clone();
        let t = Instant::now();
        if let Some(mut eq) = soundcraft_dsp::create("eq_7band") {
            soundcraft_dsp::offline::apply_plugin(&mut ch, eq.as_mut(), args.rate as f32);
        }
        record("eq_render", t.elapsed(), "7-band EQ, offline".into());
    }
    if want("bounce") {
        let mut s = Session::default();
        if let Ok(sr) = soundcraft_time::SampleRate::new(args.rate) {
            s.sample_rate = sr;
        }
        let tr = s.add_track(TrackKind::Audio, ChannelFormat::Stereo, None);
        s.pool.insert(SourceId(9), Arc::new(SourceAudio::new(buf.clone())));
        let id = s.new_clip_id();
        let len = i64::try_from(frames).unwrap_or(i64::MAX);
        if let Some(pl) = s.track_mut(tr).and_then(|t| t.playlist_mut()) {
            pl.clips.push(Clip::audio(id, "speech", SourceId(9), 0, 0, len));
        }
        let t = Instant::now();
        let out = soundcraft_mix::render_range(&s, Range::new(0, len), 1024);
        record("bounce", t.elapsed(), format!("1 stereo track, {} ch out", out.len()));
    }
    if want("resample") {
        let t = Instant::now();
        let out = soundcraft_dsp::offline::resample(&buf.channels, 44_100, 48_000);
        record("resample", t.elapsed(), format!("44.1 -> 48 kHz, {} frames out", out.first().map_or(0, Vec::len)));
    }
    if args.json {
        let rows: Vec<String> =
            results.iter().map(|(n, d, note)| format!("{{\"stage\":\"{n}\",\"seconds\":{:.4},\"note\":{:?}}}", d.as_secs_f64(), note)).collect();
        println!("{{\"minutes\":{},\"rate\":{},\"stages\":[{}]}}", args.minutes, args.rate, rows.join(","));
    }
}
