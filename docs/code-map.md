# Code map (our fork)

The code is upstream's Cargo workspace (`Cargo.toml` members). Upstream's own design notes are in `docs/` (start with `docs/control-protocol.md`, `docs/hardware-parity.md`, `docs/plugin-parity.md`). This map is ours: what each module owns, how they connect, and what differs per platform (R04). The quality gate for every module is `cargo xtask ci`.

Describes commit: 33bfcca (2026-10-28). The workspace is 14 crates, 3 apps and `xtask` (version 0.13.1), on branch `main`. Upstream owns every crate and file except the changes listed under "Where our delta lives"; at this commit upstream `main` is 59 commits ahead of ours (the next fork sync brings them in).

## Modules

| Path | Crate | Owns | Platform notes |
|---|---|---|---|
| `crates/time` | soundcraft-time | Timebases: samples, Bars\|Beats, Min:Secs, Timecode, Feet+Frames, tempo and meter maps, grid | pure, all |
| `crates/audio-io` | soundcraft-audio-io | Audio file I/O: WAV/BWF/RF64, AIFF/AIFF-C, FLAC encoder, symphonia decoders; streamed PCM reads for the disk-backed sources (`pcm.rs`, ours in part) | pure, all |
| `crates/midi` | soundcraft-midi | Note/controller sequences, SMF import/export, MIDI editing, GM names. No device I/O yet (G-02) | pure, all; device layer will be per OS (CoreMIDI, WinMM, ALSA) |
| `crates/model` | soundcraft-model | The session document: tracks, playlists, clips, mixer, routing, automation, markers (`.scraft`); the `SampleSource` trait and the disk-backed source behind clips (`source.rs`, ours in part) | pure, all |
| `crates/dsp` | soundcraft-dsp | Built-in realtime plugins, meters, loudness, offline processing (reads sources through `dsp`'s own `SampleSource` trait; streamed offline passes are ours); the safe plugin trait the hosts implement | pure, all; SIMD paths must dispatch at runtime |
| `crates/clap-host` | soundcraft-clap-host | Third-party CLAP hosting behind the dsp plugin trait | all; editors embedded on Windows and macOS |
| `crates/vst3-host` | soundcraft-vst3-host | Third-party VST3 hosting | all; editors on macOS and Windows, not Linux (upstream PR #1127 adds X11) |
| `crates/au-host` | soundcraft-au-host | Audio Unit (AUv2) hosting | **macOS only**; no editor views yet (upstream PR #1050) |
| `crates/video` | soundcraft-video | MP4/MOV demux, H.264/ProRes/MJPEG decode, thumbnails | pure, all |
| `crates/mix` | soundcraft-mix | Renders a session (tracks, plugins, sends, busses, automation) to audio, offline and realtime; read-ahead for streamed sources over a lock-free ring (`readahead.rs`, ours) | all |
| `crates/engine` | soundcraft-engine | Command engine: commands, undo, editing, import/export, bounce, parity catalog; `examples/bench.rs` benchmark (ours) | all |
| `crates/playback` | soundcraft-playback | Realtime device I/O through cpal: playback, recording, input monitoring | per OS: CoreAudio, WASAPI (ASIO behind the opt-in `asio` feature), ALSA/PulseAudio; no JACK yet (#24) |
| `crates/automation` | soundcraft-automation | Agent control: MCP server (stdio JSON-RPC), headless backend, control channel | all |
| `crates/ui-egui` | soundcraft-ui-egui | Edit and Mix windows, transport, dialogs, docking, control channel | all; macOS system menu bar, Windows DX12 default, Linux fontconfig |
| `apps/soundcraft` | soundcraft | Desktop app binary | all; packaging in `packaging/` (DMG, MSI, Flatpak, AppImage) |
| `apps/soundcraft-cli` | soundcraft-cli | Headless editing, convert, bounce, MCP server | all |
| `apps/soundcraft-web` | soundcraft-web | Browser build (WebGPU/WebGL2, WebAudio) | wasm |
| `xtask` | xtask | `cargo xtask ci`, `layers`, `assets`, `parity` report, packaging helpers; the opt-in `--profile max` (fat LTO) is in the root `Cargo.toml` | all |

## How they connect

```mermaid
flowchart TD
  app[apps/soundcraft] --> ui[ui-egui] & engine & playback
  cli[apps/soundcraft-cli] --> engine & automation & audioio[audio-io] & dsp
  ui --> engine & playback & mix & model & video
  automation --> engine
  playback --> mix & audioio & dsp & model
  engine --> mix & model & audioio & midi & video & dsp & hosts[clap/vst3/au hosts]
  mix --> model & dsp & hosts & audioio & midi
  model --> audioio & midi & time
  hosts --> dsp
```

Upstream's layer rule (checked by `cargo xtask ci`): lower crates never depend on higher ones; nothing depends on `ui-egui`.

## Where our delta lives

`git diff upstream/main...main` touches 18 files under `crates/` and `.github/` (about +2,300 / -130), beyond docs, pipeline files and the lock file: `crates/model/src/source.rs` and `clip.rs` (disk-backed sources), `crates/mix/src/readahead.rs` and `lib.rs` (read-ahead), `crates/audio-io/src/pcm.rs`, `crates/dsp/src/offline.rs` and `meter.rs`, `crates/engine/src/{edit,io}.rs`, `cmd/edit.rs`, `cmd/audiosuite.rs` and `examples/bench.rs`, `crates/playback/src/{lib,record}.rs`, `.github/workflows/pr-ci.yml`, and the `rtrb` dependency and `max` profile in `Cargo.toml`. Not yet in the tree: the MP3 encoder, the Linux handheld work (JACK/PipeWire, SIMD, GPU waveforms) and the gamepad/pen/NPU items in `SPEC.md`.

## Mac, PC and Linux

- CI: `pr-ci.yml` (ours) runs `cargo xtask ci` on Linux and `cargo test --workspace` on macOS (macos-14) and Windows (windows-2022) for every pull request that touches more than docs; on the last upstream-sync pull request the Windows test leg failed. Upstream's `ci.yml` (push to `main` and pull requests) is enabled on our fork but has not run yet; `sonarcloud.yml`, `nix.yml` and `packaging-lint.yml` run on push to `main` and on pull requests (the last two only for their own paths); `freebsd.yml` and `windows-arm64.yml` run nightly and on pushes to `release`; `release.yml` runs on pushes to `release`.
- Platform-specific code is behind `cfg(target_os)`: au-host (macOS), plugin editor windows (HWND, NSView, X11), audio backends (playback), menu bar (macOS). A change there needs the `platform:*` label.
- Paths: plugin search paths and preferences folders differ per OS; tests never load installed plugins.
- Runtime smoke tests (launch, draw a frame, play through the silent clock) don't exist yet on any OS (G-14).
