# State

Last updated: 2026-10-12 (nightly run 2). Based on upstream `acb08ff`.

## Review cursor

2026-10-12 run 2: the first full pass of WISHLIST item 1 is complete. Every file in its code areas has been read once (run 2 read the five skimmed areas: `flac.rs` encoder, `offline.rs` time_stretch/detect_transients/non_silent_ranges, `engine/src/io.rs` remaining lines, `cmd/clip_more.rs`, `cmd/file.rs` 1-195). Next pass: re-read after upstream changes (diff `acb08ff..upstream/main` in the item 1 areas), then the mixer's offline render paths (`mix/src/lib.rs` render_range/render_bus/render_clips) and `model/src/clip.rs` stretch handling.

## Open threads

- **Delete Fades moves the edit point** (`engine/src/edit.rs` `delete_fades`): trims a crossfade back to its midpoint, not the original cut; an asymmetric crossfade's Delete shifts the edit. Needs the original boundary stored. Design choice for upstream.
- **Input monitoring latency creeps to 0.5 s** (`playback/src/record.rs` `InputRing`): no target fill level. Fix: skip to the newest data when the backlog exceeds a small target. Needs a latency target decision and real-device testing.
- **Realtime mixer on rayon's global pool** (`mix/src/lib.rs` `run_strips`, 4+ strips): the audio callback waits on the shared pool that offline jobs also use. Fix: a dedicated realtime pool. Upstream design decision.
- **True-peak over-read** (`dsp/src/meter.rs`): +0.11 dB at 15 kHz/48 kHz, +0.23 dB at 15 kHz/96 kHz (EBU allows +0.2/-0.4). Low. (`perf/fast-loudness` keeps the same filter, bit-identical.)
- **Stretched clips trim in the wrong units** (`model/src/clip.rs` `trim_start_to` adds a timeline delta to a source offset; `cmd/edit.rs` trims use unscaled offsets): audio slides on any trim of a time-stretched clip. Needs stretch-aware trim in `model`; affects Elastic/TCE users, not plain VO.
- **Conform to Tempo stale length** (`cmd/clip_more.rs` ~243-252, `clip.len0.{id}` never updated after split/trim). Music feature; low priority for us.
- **Remove Pitch Shift** refuses clips over 1 h and skips the edit-lock check (`cmd/more_util.rs` ~137-145). Low.
- **Effects Render on a bypassed clip** deletes its effect settings (`cmd/clip_more.rs` ~207-213). Suspected unintended. Low.
- **Offline DSP details** (`dsp/src/offline.rs`): `time_stretch` first ~19 samples faded (sample 0 always 0); WSOLA picks the earliest candidate in silence; `detect_transients` uses one global threshold from the loudest flux in the file and can place onsets up to one window early. Low/medium; matters if Strip Silence/transient tools are used on long files.
- **FLAC encoder**: possible -2^31 residual at 32-bit (spec excludes it); reserves half the PCM size up front (2 GB for 3 h Int32 stereo). Low; FLAC isn't in our formats.
- **Bounce Stems on web** uses `std::fs` directly (fails in the browser). Low.
- **Track rename** checks uniqueness before truncating to 64 chars (duplicates possible). Low.
- **Upstream issues in our areas not fixed by us**: #244 (pan automation stepped per block, so bounce ≠ playback; PR #210 is rewriting that code), #247 (locate keeps plugin tails), #240/#241 (unreachable from the app).
- **MP3 export (G-21)**: waiting on Ian's choice (see design note).

## Design notes

### G-21 MP3 export (2026-10-12)

No MP3 encoder that is both permissively licensed and production-ready exists in Rust today.
- `oxideav-mp3` 0.1.3 (crates.io, MIT, pure Rust, says clean-room from ISO 11172-3/13818-3; depends on `oxideav-core`, MIT). Tested on test file B (2 min excerpt, 16-bit stereo 44.1 kHz): output decodes with symphonia and runs to the right length, but encode speed is 1.4x realtime plain CBR and 0.8x with its "High" preset (a 2-hour audiobook would take 85-150 min), and waveform SNR is 12-13 dB at 128 kbps (low). No LAME/Info gapless tag by default (symphonia reported 1057 samples of leading delay). 2 GitHub stars, single author. Not usable as is.
- `encoRust` (GitHub only, MIT OR Apache-2.0): self-described experimental research project; no crates.io release, no VBR, no bit reservoir. Not usable.
- LAME, Shine (and their Rust ports/bindings) are LGPL: excluded.
- MP3 patents have expired, so writing our own is a licensing non-issue.

Options for Ian:
1. **Write a clean-room encoder in `audio-io`** (like its FLAC encoder), from ISO/IEC 11172-3 only: polyphase analysis filterbank, MDCT with block switching, Huffman tables and scalefactor bands (factual tables from the standard), CBR with bit reservoir, then a simple psychoacoustic model, then joint stereo and a Xing/LAME-style Info tag for gapless and seeking. Estimate: 3-5 nights to a CBR encoder good enough for speech at 96-128 kbps, more for transparency. Parallel per-frame analysis keeps it fast.
2. Contribute speed/quality work to `oxideav-mp3` and depend on it (MIT is fine). Less work if the author is active; risky single-maintainer dependency.
3. Ship Opus (in Ogg) for podcast feeds that accept it, MP3 later. Pure-Rust Opus encoders have the same maturity problem; libopus is BSD but C (breaks "pure Rust").
Licensing note (run 2): minimp3 (lieff/minimp3, C decoder) is CC0 (public domain): its Huffman, scalefactor-band and window tables can be ported into an MIT/Apache crate, and it can serve as a second reference decoder in tests. symphonia's MP3 decoder is MPL-2.0: never copy its tables (using it as a test decoder through the existing dependency is fine).
Recommendation: option 1, starting with a deliberately simple CBR encoder (no psychoacoustic model, like early encoders) behind `encode(.., FileFormat::Mp3)`, measured with the bench harness and a decoder round-trip test, then improve quality. Upstream PR #159 (ID3v2.4 tagging and chapters) pairs with it.

### G-06 disk streaming (2026-10-12)

Today every source is decoded whole into `SourcePool` (`model/src/source.rs`: `SourceAudio { buffer: AudioBuffer, peaks }`) as planar f32. Measured on test file C (69 min, 16-bit stereo 44.1 kHz WAV, 732 MB on disk): decode 10.1 s, peak RSS 4.4 GB for decode + peaks + encode round trips; one hour of 48 kHz stereo is 1.38 GB of f32, plus peaks (about 6 %: 8 bytes per 32 samples at level 0, ~43 MB per channel-hour). A 3-hour session file needs ~4 GB before any edit; audio-io's decode cap (2^30 samples) is reached at about 3 h 06 min of 48 kHz stereo.

Proposed shape (reviewable increments):
1. **Source trait in `model`**: `SourceAudio` becomes either `InMemory(AudioBuffer)` or `Disk(DiskSource)` behind a `read(ch, start, out: &mut [f32])` method; the mixer (`mix/src/lib.rs` ~1265-1275 reads `buffer.channels` slices directly) switches to `read` into per-strip scratch. Peaks stay in memory (small) and get cached to a sidecar file.
2. **`DiskSource` for PCM/float WAV, RF64 and AIFF** (the native readers already know `data_start`/`data_len`/`PcmKind`; upstream PR #43 adds a `DiskWavReader` doing bounded frame-window reads, a good base): memory-mapped or `pread` windows, decoding to f32 on read. Compressed formats keep decoding to memory (or decode once to a float WAV cache file in the session's Audio Files).
3. **Realtime side**: the audio thread must not touch the disk. A reader thread pre-fills a per-source ring of the next ~2 s around the playhead (and after locate), with the audio callback reading only from the ring and outputting silence plus a "disk too slow" flag on underrun.
4. **Edits**: destructive edits (Waveform Editor) write new float files (as AudioSuite already does) rather than editing in place, so undo stays a pointer swap.
5. **Recording to disk**: already streams to a WAV via `TakeWriter`; the take could become a `DiskSource` directly instead of being re-read into RAM.
Increment 1+2 with a test that a disk source and an in-memory source mix identically is contained (~1 night). The realtime ring (3) needs the Z13 for underrun testing.
Run 2 check: the mixer reads source audio in exactly one place (`mix/src/lib.rs` `render_clip`, ~1259-1310, which already takes an unused `_scratch` buffer) plus the sampler bridge (~1363-1369); engine reads `audio.buffer` for encoding in 4 places and `health.rs`. So increment 1 (a `SourceAudio::read(ch, start, out)` API used by `render_clip`) is small, but on its own it only adds a copy per block; it pays off only together with increment 2/3 and upstream's PR #43. Not started unreviewed: propose to Ian/upstream first.

### B0-B2 measurements (2026-10-12, cloud CPU, not the Z13; relative only)

Bench example (`perf/bench-harness`), 60 min synthetic 48 kHz float stereo, `release` profile: wav encode 1.9 s, wav decode 2.1 s, FLAC 24-bit 20.1 s, peaks 0.75 s, STFT (2048/512, mono, `dsp::spectrum::magnitude_db`) 17.8 s, integrated loudness 16.1 s, 7-band EQ render 11.6 s, one-track bounce 7.7 s, 44.1 -> 48 kHz resample 114.7 s.
- B1 `max` profile (fat LTO, cgu 1): within noise of `release`; no recommendation to use it.
- B2 resample on scoped threads: 114.7 -> 30.8 s, bit-identical (`perf/parallel-resample`).
- B2 loudness (run 2, `perf/fast-loudness`): 20 min stereo, whole-file call 5.15 -> 1.26 s, 4096-frame blocks 4.81 -> 2.64 s, bit-identical. Remaining cost: K-weighting biquads ~1.1 s (serial per channel), true peak ~1.5 s. Explicit AVX2/AVX-512 would need `#[target_feature]` + `unsafe` calls, which the crates forbid; stays compiler-vectorised SSE2.
- Next B2 candidates: FLAC encode (frames are independent: encode in parallel, concatenate in order; identical output), loudness (per-channel K-filter + true-peak in parallel, combine block energies in channel order; identical), and an STFT that reuses its FFT plan (`magnitude_db` builds a new `FftPlanner` per call; no production caller yet, so only matters once a spectrogram exists). Peaks are already fast.
- AVX-512 SIMD: none of the measured stages is dominated by a loop that would vectorize better by hand except the resampler's sinc dot product (table lookup with linear interpolation per tap prevents auto-vectorization). A polyphase table (precomputed taps per output phase) would let the compiler vectorize and cut work ~2x, but changes output in the last bits: needs an accuracy test instead of bit-identity.

### B3 GPU spectrogram and waveform (2026-10-12)

SoundCraft has no spectrogram today, and the waveform is drawn from CPU `Peaks` min/max columns. For the Waveform Editor's spectral display: compute STFT magnitude tiles (e.g. 2048/512, log-frequency rows) once per source on CPU threads (17.8 s per mono hour here; ~4x with threads; ~1-2 s on the Z13's 16 cores), store as u8/f16 dB tiles, upload as `wgpu` textures and let a fragment shader map to colour and zoom (mip levels for zoomed-out views). A `wgpu` compute FFT is possible but the CPU path is already fast enough on 16 cores and simpler to test; do GPU drawing first, GPU FFT only if measurements say so. Needs the Z13 for RADV/Vulkan timing.

### B4/B5 Linux audio and desktop (2026-10-12, write-up only)

- Audio: playback uses cpal's default host. On Linux that is ALSA; with PipeWire running, cpal's ALSA backend goes through `pipewire-alsa`, which works but gives PipeWire's default quantum (often 1024 at 48 kHz, ~21 ms) and no control from the app. cpal also has a JACK host (feature `jack`) which PipeWire serves via `pipewire-jack`, giving app-chosen buffer sizes and proper graph scheduling. Upstream issue #196 asks for exactly this; G-01 covers device/buffer selection. Suggested: build with cpal's `jack` feature on Linux, prefer the JACK host when a server is reachable, fall back to ALSA; expose buffer size (G-01). Realtime priority: PipeWire's RT module grants it to JACK clients; for the ALSA path, request `SCHED_FIFO` via rtkit (D-Bus) rather than requiring `ulimit -r`.
- Desktop: SteamOS desktop mode is KDE Plasma on Wayland; gamescope (game mode) is a nested compositor that suits games, not DAWs: recommend desktop mode. Upstream PR #150 defaults to XWayland for file drag-and-drop; issue #45/#107 cover font and llvmpipe problems. Check on the Z13: wgpu picks RADV Vulkan (not llvmpipe), fractional scaling, file dialogs via the XDG portal (Flatpak).

### Item 4: Waveform Editor design (write-up; no code until Ian approves)

**Crate-reuse audit** (upstream `acb08ff`):

| v1 piece | SoundCraft crate/API | Gap |
|---|---|---|
| Open/save WAV, AIFF, FLAC; read MP3 etc. | `audio-io` `decode`/`encode`/`probe` | MP3 out (G-21); whole file in RAM (G-06); `encode` returns a `Vec<u8>` (no streaming writer except `wav_float_header`) |
| Waveform overview | `audio-io::peaks::Peaks` | fine for v1; ~43 MB per channel-hour |
| Spectral display | `dsp::spectrum::magnitude_db` (single frame) | no STFT/spectrogram type; build one (see B3) |
| Time selection, destructive edits | `dsp::offline` (gain, normalize, fade, reverse, invert, DC, resample, stretch, pitch) on `&mut [Vec<f32>]` | good fit: these are plain functions on buffers. Clip/session edit ops in `engine` are multitrack-shaped; the single-file editor should do its own buffer edits |
| Spectral selection edits | none | new: STFT -> mask -> ISTFT |
| Undo history | `engine` undo is whole-`Session` `Arc` snapshots | for one big buffer, snapshot = copy of hours of audio; editor needs its own undo (store edited regions, or immutable chunked buffers with `Arc` sharing) |
| Effects Rack | `dsp` plugins (`create(id)`, `Plugin::process`), `offline::apply_plugin` | 26 processors; no noise reduction, no spectral tools; third-party hosting is in `clap-host`/`vst3-host`/`au-host` |
| Match Loudness | `dsp::meter::LoudnessMeter` (accurate to 0.02 LU vs an independent reference; tested tonight) + `offline::gain` | needs a true-peak limiter step (the `maximizer` plugin may serve; untested) |
| Playback | `playback` takes a whole `Session` | wrap the file as a one-track session (works today) or add a buffer-player API |
| Markers | `model` markers are session-level | editor keeps its own (and writes them as WAV `cue`/BWF; `audio-io` doesn't write `cue` yet) |

Library friction: crates are not on crates.io; public APIs are wide (`model` ~170, `engine` ~170 public items) and designed for the app; `engine` pulls in plugin hosts and video. The editor should depend on `audio-io`, `dsp`, `time` (and maybe `playback`/`model` for playback), not on `engine`. Nothing below L6 depends on egui, so no egui coupling to break.

**How to depend on SoundCraft**: a git dependency pinned to a tag (`soundcraft-audio-io = { git = "https://github.com/storytold/soundcraft", tag = "v0.5.0" }`), bumped deliberately, with our CI building against the pin. Upstream tags near-daily, so pin to a tag (not `main`), update weekly or when a needed fix lands, and keep a thin adapter module so API churn lands in one place. If we need unmerged fixes, point the git dependency at our fork's branch temporarily. Later, if upstream publishes to crates.io, switch to version requirements.

**Noise reduction**: Audition's Noise Reduction (process) captures a noise print from a selection and exposes Noise Reduction (%), Reduce by (dB), Spectral Decay Rate, Smoothing, Precision Factor, Transition Width, FFT size, and a "noise only" preview. Approach: STFT (Hann, 75% overlap, FFT 4096 at 48 kHz default) -> per-bin noise profile (mean and spread of magnitude over the print) -> per-bin gain = max(floor, 1 - k * noise/|X|) (spectral subtraction/gating) with temporal smoothing (attack/decay) and frequency smoothing to suppress musical noise -> ISTFT overlap-add. All in `dsp` as an offline function, testable with synthetic tone+noise (SNR improvement, tone level preserved).

**Match Loudness**: measure integrated LUFS (and true peak) per file with `LoudnessMeter`; gain = target - measured; if the gained true peak exceeds the ceiling (e.g. -1 dBTP for podcasts, -3 dBTP for audiobook platforms), apply a true-peak limiter (4x oversampled lookahead) after the gain, then re-measure and iterate once. Targets: -16 LUFS (podcast stereo), -19 (mono), -18 to -23 (audiobook platforms: e.g. ACX RMS -23 to -18 dB, peak -3 dB, noise floor -60 dB; our test files measured -13.1, -17.8, -18.3 LUFS with noise floors (5th-percentile 50 ms RMS) of -65, -59, -58 dBFS).

**One-click speech enhancement**: DeepFilterNet (2/3) is a real-time speech denoiser whose code and published weights are MIT OR Apache-2.0; its Rust crate (`deep_filter`, libDF) runs the model with `tract` (pure-Rust inference, MIT/Apache) on CPU, about real time on one core, so a 2-hour file is minutes on 16 cores (chunked in parallel with overlap). It removes noise and some reverb but does not do "studio mic" EQ/timbre transfer; that part of Adobe Podcast Enhance has no open MIT/Apache equivalent we know of. Plan: v1 = DeepFilterNet denoise with strength/blend (dry/wet mix and attenuation limit, which DFN supports) + a de-hum notch and a gentle "voice EQ" preset; GPU via `wgpu` is unnecessary for v1 (CPU is fast enough), could come later via `burn`'s wgpu backend. Effort: ~2-3 nights to integrate and test DFN offline; verify the weights' license file in the release we ship.

## Branches

All on `iannitram/soundcraft`, based on upstream `acb08ff` unless noted. None has an upstream PR.

| Branch | Purpose | Status |
|---|---|---|
| `fix/offline-long-file-truncation` | Offline resample/stretch/tail renders cut files at ~93 min | Ready; fmt/clippy/tests green |
| `fix/split-keeps-gain-envelope` | Split/trim start lost the clip gain envelope | Ready |
| `fix/playback-resample-block-edge` | Click per block when device rate != session rate | Ready; may textually conflict with upstream PR #210 (same file) |
| `fix/copy-drops-outside-fades` | Copy kept fades of the uncopied part | Ready |
| `fix/crossfade-short-handles` | Crossfade with a missing handle dipped to silence | Ready |
| `fix/audiosuite-bakes-clip-gain` | AudioSuite dropped clip gain | Ready |
| `fix/float-writes-keep-headroom` | 32-bit float writes clipped at 0 dBFS | Ready, but changes documented behaviour: needs upstream agreement |
| `feat/pr-ci` | G-14: `cargo xtask ci` on Linux + tests on macOS/Windows per PR | Ready; workflow file push was accepted |
| `fix/paste-keeps-crossfades` | Paste/Duplicate/Repeat destroyed crossfades in the pasted material | Ready |
| `perf/bench-harness` | B0 benchmark example | Ready |
| `perf/max-profile` | B1 `[profile.max]` (on top of bench-harness) | Measured: no gain; keep for Z13 measurement, not for upstream |
| `perf/parallel-resample` | B2 resample on scoped threads (on top of bench-harness) | Ready |
| `fix/long-render-truncation` | Bounce/render/consolidate/commit/export/strip-silence silently cut at ~93 min stereo | Ready (run 2); raises the 64-bit render cap to 2^31 samples |
| `fix/shuffle-ripples-automation` | Shuffle Clear/Paste left automation behind (upstream #232) | Ready (run 2) |
| `fix/trim-to-file-stays-in-bounds` | Trim to File Boundaries / Fill Selection overlapped neighbours (upstream #229, #228) | Ready (run 2) |
| `fix/move-keeps-crossfades` | Moving a crossfaded pair broke the crossfade | Ready (run 2); trivial test-block conflict with `fix/paste-keeps-crossfades` |
| `fix/unique-take-paths` | Two armed tracks could record into one file | Ready (run 2) |
| `fix/unique-stem-names` | Stems of clashing track names overwrote each other | Ready (run 2) |
| `fix/bwf-time-reference-resampled` | BWF timestamp not rescaled on import resample | Ready (run 2) |
| `fix/bundle-size-checked-on-save` | Web bundles over 1 GiB saved but never reopened; missing-source path reuse | Ready (run 2) |
| `perf/fast-loudness` | B2 loudness meter 4x faster, bit-identical (on top of bench-harness; also adds `loudness_whole` bench stage) | Ready (run 2) |
