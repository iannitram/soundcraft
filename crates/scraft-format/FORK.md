# Fork copy of craft-scraft-format

Upstream SoundCraft moved its session model (`crates/model`) into craft-libs as
`craft-scraft-format` (storytold/craft-libs, rev `45a8e16669e70dadb780dfbc759c882dd5b7ea1c`).
This fork had changes in that model that craft-libs does not have, so the workspace builds this
copy of the crate instead of the git one. It is the craft-libs source at that rev plus:

- `clip.rs`: fade-in and fade-out never overlap (`clamp_fades` shrinks both in proportion;
  `set_fade_in` / `set_fade_out`); `Fade::original_edit_point` for crossfades (#7).
- `source.rs`: the `SampleSource` trait and streamed sources in `SourcePool`
  (`insert_stream`, `get_source`), used by soundcraft-mix's read-ahead.
- `bundle.rs`: a source whose audio is missing keeps its path; no other source is given it.

When upstream's craft-libs gains these, or when the craft-scraft-format rev in the root
`Cargo.toml` moves, re-copy the crate from craft-libs and re-apply only what is still missing.
