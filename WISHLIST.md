# SoundCraft: our wishlist

Our priorities for this fork, in order. **Ian owns this file.** The nightly agent reads it at the start of every run and never edits it; it suggests changes in its log under "Needs Ian". Upstream's own roadmap is in the code branch (`ROADMAP.md`, `docs/roadmap.md`, `docs/gaps.md`), not here.

Use: spoken-word editing (podcast and audiobook voice-over). Long files: 1–3 hours, 48 kHz, often 32-bit float stereo WAV. Formats in and out: WAV, AIFF, MP3. Hardware: AMD Strix Halo laptop (Ryzen AI Max+ 395, 16 cores, full AVX-512, Radeon 8060S, SteamOS, RADV Vulkan, unified memory) and an Apple-Silicon Mac.

*Last changed: 2026-10-12 (first upstream PR, #324; two more rules for later runs, on flaky tests and Rust versions; see "Done locally" and Decisions).*

## 1. Correctness of the crates we'll build on

We plan an Audition-style single-file Waveform Editor (a separate repo) on top of SoundCraft's crates, so their correctness matters most to us.

- Upstream gaps: G-04 (open correctness bugs)
- Code areas: `crates/audio-io` (WAV/BWF/RF64, AIFF read/write, peaks), `crates/dsp` (`meter.rs` incl. the BS.1770 loudness meter, `offline.rs`, `spectrum.rs`), `crates/engine` (edit ops, undo, offline processing, file commands), `crates/playback`, `crates/mix` (offline render paths: `render_range`, `render_bus`, `render_clips`), `crates/model/src/clip.rs`
- The first full pass is done (2026-10-12). From now on: first re-review what upstream changed in these areas since the review cursor's base commit, then the areas the cursor lists as next.
- Look for: off-by-one in frame math, overflow on 1–3 hour files, sample-rate/channel mismatches, fades and gain at clip edges, undo that doesn't restore exactly, WAV/AIFF header edge cases, allocation or locks on the audio thread. Round-trip WAV and AIFF writes and compare samples.
- One `fix/<slug>` branch per confirmed bug, with a regression test.

## 2. Upstream gaps that block our use

- **G-21 compressed export:** we need MP3 out. Only with a permissively licensed encoder (no LAME/LGPL). **Decided 2026-10-12: go with option 1 in `STATE.md`, our own clean-room encoder in `crates/audio-io`.** Work from ISO/IEC 11172-3 only. Start deliberately simple: CBR, no psychoacoustic model, behind `encode(.., FileFormat::Mp3)`, with a decoder round-trip test and a bench stage. Then the bit reservoir, a Xing/Info tag for gapless and seeking, joint stereo, and quality work. Tables may be ported from minimp3 (CC0); never from symphonia (MPL-2.0), LAME or Shine. Reviewable increments on `feat/mp3-export`, one or two per night; say in each log what a listener would hear at 96 and 128 kbps mono/stereo speech.
- **G-06 disk streaming:** every source is decoded whole into RAM; long episodes are heavy. Design write-up first; code only if it's contained.
- **G-14 PR CI** (`cargo xtask ci` on Linux/macOS/Windows): if pushing a workflow file is refused, put it in the log.

## 3. AMD / Strix Halo performance

Upstream rates performance "~35 %, unmeasured". Measure first; every speed-up dispatches at runtime so Apple Silicon and other PCs keep working; GPU work stays in `wgpu`.

- B0: a benchmark harness (synthetic audio sized like a 1-hour 48 kHz stereo episode): decode, waveform peaks, full-file spectrogram, integrated-LUFS analysis, one offline effect render. Branch `perf/bench-harness`.
- B1: an opt-in max-optimization profile (fat LTO, `codegen-units = 1`) measured against `release`. Don't change `release`.
- B2: SIMD (runtime-dispatched) and `rayon` for work that splits cleanly: peaks, spectrogram, offline analysis and renders. Identical output, tested.
- B3: spectrogram and waveform drawn from GPU data (`wgpu` compute + texture).
- Long-file import speed and memory: decode and resample on all cores with identical output, and lower peak memory when opening 1–3 hour files (ties into G-06).
- B4/B5: Linux audio path (PipeWire/JACK, realtime thread priority, buffer size; overlaps upstream G-01) and Linux/Wayland/gamescope compatibility: write-ups, since they need the real machine.
- Code areas: `Cargo.toml` profiles, `crates/audio-io/src/peaks.rs`, `crates/dsp`, `crates/mix`, `crates/playback`, `crates/ui-egui` (waveform/meter drawing)
- Anything needing the real GPU or audio hardware goes under "Needs Z13 testing" with exact steps.

## 4. Waveform Editor design (write-up only until Ian starts the design phase)

An Adobe Audition-style Waveform Editor in a new repo, depending on SoundCraft's crates: one file at a time; waveform plus spectral frequency display; time and spectral selection; destructive edits; undo history; Effects Rack; noise print → noise reduction; Match Loudness to LUFS targets; markers; Audition layout and shortcuts. Keep design notes in `STATE.md`. No code for it in this fork.

- Crate-reuse audit: which crate/API covers each v1 piece, what's missing, what makes the crates hard to use as a library (public API surface, egui coupling, whole-file-in-RAM).
- How the new repo should depend on SoundCraft (git dependency pinned to a rev/tag vs. alternatives), given upstream's near-daily releases.
- Noise reduction approach (e.g. STFT spectral subtraction/gating from a captured noise profile) and the parameters Audition exposes.
- Match Loudness on `dsp/src/meter.rs`: gain to target plus true-peak limiting.
- One-click AI speech enhancement (like Adobe Podcast "Enhance Speech": noise, echo and hum out, a studio-mic sound, strength/blend controls): feasibility, open models whose code AND weights are MIT/Apache-compatible (e.g. DeepFilterNet and its Rust implementation), running locally via `wgpu`, rough effort.

## 5. Help upstream's goals

Their ranked gaps in `docs/gaps.md`, skipping work that needs real hardware (G-01, G-02, G-03, G-10, G-11, G-12) or an owner decision (G-22).

## Done locally (don't redo)

Daytime sessions on our own machines work in this repo too. Check this list and the fork's branches before starting an item.

- `integration/nightly-2026-10-12`: every branch marked Ready through run 3 (not `perf/max-profile`, not `feat/mp3-export`), merged, plus the two local branches below, the review fixes in the next item, and upstream `main` up to `6cf896e` (0.6.1; 0.6.0 and 0.6.1 changed version numbers only). The full `cargo xtask ci` passes on it on Windows (714 tests, run at `c3296e2`, before the 0.6.1 version-number merge). Never push to it or to `main`; keep basing your branches on `upstream/main` so each one can become a clean upstream PR.
- **Local review of runs 1 to 3 (2026-10-12).** Four Ready branches had defects. Each now has a follow-up commit on the same branch, so fetch before you touch them and never force-push them:
  - `fix/offline-long-file-truncation` (`d6010cb`): the "grow 2x at most" rule still cut long low-rate files (two hours at 22.05 kHz lost ten minutes; three hours at 8 kHz stopped at 93 minutes). An output may now grow 8x within 2^30 samples per channel, and imports refuse a file that would still be cut (`resample_fits`).
  - `perf/import-resample-memory` (`f82fb6c`): `resample_in_place` had its own copy of the old cap, so once merged with the fix above, imports were cut at 93 minutes again. Both resamplers now share `resampled_len`. (Merging the two branches conflicts in `resample`; the resolution on the integration branch is `resampled_len` calling `scaled_len`.)
  - `fix/trim-to-file-stays-in-bounds` (`35b96c6`): `room_for` skipped a neighbour that overlaps the clip, which is what every crossfade leaves, so the trim grew across it.
  - `perf/parallel-wav-decode` (`86f0867`): `Scope::spawn` panics when the OS refuses a thread. Now `Builder::spawn_scoped` with a fallback on the calling thread.
- **What to take from that review, on every run:**
  1. A branch is tested alone, but the build we use is all of them merged. When a branch adds a second path through a calculation that another open branch changes (a cap, a length, a rounding rule), share one helper instead of copying the expression, and name in the log every other open branch that touches the same function.
  2. Clip-edit fixes: test with a crossfaded (overlapping) neighbour, not only clips that abut.
  3. Length and rate arithmetic: test 8, 16 and 22.05 kHz inputs and 1 to 3 hour lengths, not only 44.1 and 48 kHz.
  4. No `Scope::spawn` or `thread::spawn` outside tests: `Builder::spawn_scoped` and a fallback, as `fill_parallel` in `dsp/src/offline.rs` does.
  5. Two upstream tests are flaky (upstream issue #323): `edit_window::tests::shift_click_adds_another_track_to_the_selection` and `theme::tests::application_frames_follow_system_appearance_and_preserve_manual_palettes` in `crates/ui-egui`, about one run in twenty when tests run in parallel. If a gate run fails on one of them only, run it again before reporting a failure. Don't fix them on our branches.
  6. Say in the log which Rust version every result came from. The `collapsible_match` lint error in `crates/video/src/isobmff/codec.rs` is raised by Rust 1.95 only (any platform); Rust 1.99 does not raise it. A branch that fails lint on that line alone, with 1.95, is not at fault.
- `fix/ci-lint-fails-on-windows`: upstream's gate failed on Windows (an unused variable in a test, the VST3 test fixture's bus-flag type, a path-string test, the late-exit test's order) and, with Rust 1.95 only, on one clippy error on any platform. Fixed, test code and one match guard only. **Sent upstream as PR #324 (2026-10-12)** from `fix/gate-on-windows-and-rust-1.95`, which holds the same changes rebuilt on upstream `main`. Never push to that branch; if #324 has merged by the time you run, say so in the log.
- `fix/input-monitoring-latency-creeps`: the open thread "Input monitoring latency creeps to 0.5 s" is fixed (`playback/src/record.rs`, `InputRing` catches up; two tests).
- **MP3 Huffman tables:** a local session will generate `huffman.rs` and commit it to `feat/mp3-export`. Don't try the conversion in the sandbox again. Until the file is there, do the MP3 parts that don't need it (bit reservoir, Xing/Info tag, wiring behind `encode`, test scaffolding with a decoder round trip), or move on to the next item.

## Decisions

Answers to "Needs Ian" items, newest first. Don't raise these again unless something changes.

- 2026-10-12, **mono track into a mono bus at unity gain:** keep `fix/mono-bus-unity-gain` on our side; not proposed upstream yet (it stays in the "behaviour change" list with the float-headroom fix).
- 2026-10-12, **upstream design calls** (Write-mode automation, Delete Fades, the realtime thread pool and the like): write our proposed fix on a branch when it is contained and has a test, as the monitoring-latency one was; otherwise report only.
- 2026-10-12, **MP3 export (G-21):** option 1, our own clean-room encoder. See item 2.
- 2026-10-12, **render memory cap:** accepted as `fix/long-render-truncation` has it (2^31 samples on 64-bit targets, 2^29 on 32-bit, a clear error beyond that).
- 2026-10-12, **float writes keep headroom:** keep `fix/float-writes-keep-headroom` on its branch. Not proposed upstream yet; it stays in the "ready for an upstream PR, behaviour change" list.
- 2026-10-12, **Waveform Editor direction:** accepted as the working assumption: depend on `audio-io`, `dsp` and `time` (not `engine`), as a git dependency pinned to an upstream tag, with the editor's own undo and markers. Still write-up only; no editor code here.
- 2026-10-12, **wishlist suggestions:** both taken (`crates/mix` under item 1; long-file import under item 3).
- 2026-10-12, **upstream PRs:** the first is open (#324, the gate fix). Approved next, once it lands: `fix/offline-long-file-truncation`, then `fix/unique-take-paths`. A local session opens them; you never open PRs, issues or comments upstream. Keep the draft titles current in the log, and tell us when an upstream PR by someone else lands that overlaps one of our branches (#292 and #290 already overlap `fix/trim-to-file-stays-in-bounds` and `fix/shuffle-ripples-automation`; #210 and #295 touch the code of `fix/playback-resample-block-edge` and `fix/automation-block-independent`).
