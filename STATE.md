# State

## Review cursor

Updated 2026-10-12 ~04:55 UTC (in progress; review stops when item 1's files are all read once or at ~06:00 UTC).

Read once:
- `crates/audio-io`: `lib.rs`, `buffer.rs`, `wav.rs`, `pcm.rs`, `aiff.rs`, `peaks.rs`, `symph.rs` (all lines); `flac.rs` lines 1-80 (encoder covered by round-trip proptests).
- `crates/dsp`: `meter.rs` (all), `spectrum.rs` (all), `offline.rs` (all but `time_stretch` body lines ~260-360 and `detect_transients`/`non_silent_ranges`), `biquad.rs` (coefficients and state type).
- `crates/engine`: `edit.rs`, `lib.rs` (undo/redo, execute), `cmd/edit.rs`, `cmd/edit_more.rs`, `cmd/clip.rs` (review agent); `io.rs` lines 15-45 and 725-750 (source resampling only).
- `crates/model/src/clip.rs` (trim, fades, gain envelope).
- `crates/playback`: `lib.rs`, `record.rs`, plus the per-block path of `crates/mix/src/lib.rs` (review agent).

Left: `audio-io/src/flac.rs` lines 80-379; `engine/src/io.rs` (rest); `engine/src/cmd/file.rs`; `engine/src/cmd/audiosuite.rs`; `engine/src/cmd/clip_more.rs`; `dsp/src/offline.rs` time_stretch/transients.

## Open threads

(none)

## Design notes

(none)

## Branches

| Branch | Purpose | Status |
|---|---|---|
