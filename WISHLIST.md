# SoundCraft: our wishlist

Our priorities for this fork, in order. **Ian owns this file.** The nightly agent reads it at the start of every run and never edits it; it suggests changes in its log under "Needs Ian". Upstream's own roadmap is in the code branch (`ROADMAP.md`, `docs/roadmap.md`, `docs/gaps.md`), not here.

Use: spoken-word editing (podcast and audiobook voice-over). Long files: 1–3 hours, 48 kHz, often 32-bit float stereo WAV. Formats in and out: WAV, AIFF, MP3. Hardware: AMD Strix Halo laptop (Ryzen AI Max+ 395, 16 cores, full AVX-512, Radeon 8060S, SteamOS, RADV Vulkan, unified memory) and an Apple-Silicon Mac.

*Last changed: 2026-10-12 (first version).*

## 1. Correctness of the crates we'll build on

We plan an Audition-style single-file Waveform Editor (a separate repo) on top of SoundCraft's crates, so their correctness matters most to us.

- Upstream gaps: G-04 (open correctness bugs)
- Code areas: `crates/audio-io` (WAV/BWF/RF64, AIFF read/write, peaks), `crates/dsp` (`meter.rs` incl. the BS.1770 loudness meter, `offline.rs`, `spectrum.rs`), `crates/engine` (edit ops, undo, offline processing, file commands), `crates/playback`
- Look for: off-by-one in frame math, overflow on 1–3 hour files, sample-rate/channel mismatches, fades and gain at clip edges, undo that doesn't restore exactly, WAV/AIFF header edge cases, allocation or locks on the audio thread. Round-trip WAV and AIFF writes and compare samples.
- One `fix/<slug>` branch per confirmed bug, with a regression test.

## 2. Upstream gaps that block our use

- **G-21 compressed export:** we need MP3 out. Only with a permissively licensed encoder (no LAME/LGPL). If no clean MP3 encoder exists, write up the options and do what's clean.
- **G-06 disk streaming:** every source is decoded whole into RAM; long episodes are heavy. Design write-up first; code only if it's contained.
- **G-14 PR CI** (`cargo xtask ci` on Linux/macOS/Windows): if pushing a workflow file is refused, put it in the log.

## 3. AMD / Strix Halo performance

Upstream rates performance "~35 %, unmeasured". Measure first; every speed-up dispatches at runtime so Apple Silicon and other PCs keep working; GPU work stays in `wgpu`.

- B0: a benchmark harness (synthetic audio sized like a 1-hour 48 kHz stereo episode): decode, waveform peaks, full-file spectrogram, integrated-LUFS analysis, one offline effect render. Branch `perf/bench-harness`.
- B1: an opt-in max-optimization profile (fat LTO, `codegen-units = 1`) measured against `release`. Don't change `release`.
- B2: SIMD (runtime-dispatched) and `rayon` for work that splits cleanly: peaks, spectrogram, offline analysis and renders. Identical output, tested.
- B3: spectrogram and waveform drawn from GPU data (`wgpu` compute + texture).
- B4/B5: Linux audio path (PipeWire/JACK, realtime thread priority, buffer size; overlaps upstream G-01) and Linux/Wayland/gamescope compatibility: write-ups, since they need the real machine.
- Code areas: `Cargo.toml` profiles, `crates/audio-io/src/peaks.rs`, `crates/dsp`, `crates/mix`, `crates/playback`, `crates/ui-egui` (waveform/meter drawing)
- Anything needing the real GPU or audio hardware goes under "Needs Z13 testing" with exact steps.

## 4. Waveform Editor design (write-up only until Ian approves it)

An Adobe Audition-style Waveform Editor in a new repo, depending on SoundCraft's crates: one file at a time; waveform plus spectral frequency display; time and spectral selection; destructive edits; undo history; Effects Rack; noise print → noise reduction; Match Loudness to LUFS targets; markers; Audition layout and shortcuts. Keep design notes in `STATE.md`. No code for it in this fork.

- Crate-reuse audit: which crate/API covers each v1 piece, what's missing, what makes the crates hard to use as a library (public API surface, egui coupling, whole-file-in-RAM).
- How the new repo should depend on SoundCraft (git dependency pinned to a rev/tag vs. alternatives), given upstream's near-daily releases.
- Noise reduction approach (e.g. STFT spectral subtraction/gating from a captured noise profile) and the parameters Audition exposes.
- Match Loudness on `dsp/src/meter.rs`: gain to target plus true-peak limiting.
- One-click AI speech enhancement (like Adobe Podcast "Enhance Speech": noise, echo and hum out, a studio-mic sound, strength/blend controls): feasibility, open models whose code AND weights are MIT/Apache-compatible (e.g. DeepFilterNet and its Rust implementation), running locally via `wgpu`, rough effort.

## 5. Help upstream's goals

Their ranked gaps in `docs/gaps.md`, skipping work that needs real hardware (G-01, G-02, G-03, G-10, G-11, G-12) or an owner decision (G-22).
