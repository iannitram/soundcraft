# Changelog (our fork)

Newest first. One line per change of stage, release or decision that the next session needs. Upstream's history is in ROADMAP.md's progress log.

- 2026-11-02 Code map refreshed again: commit pointer, sync counts, crossfade edit point (#7), CI status.
- 2026-11-02 Upstream sync (PR #61): storytold main 8444edf (238 commits) merged; 0 behind, 117 ahead. The three Windows CLI tests that expect a refused connection now also accept the 800 ms connect timeout's "connection timed out" text on Windows only (no test skipped). Overlapping clip fades keep our proportional shrink; whether Audition does the same is open in #62.
- 2026-11-01 vst3-host: `Vst3Plugin::reactivate()` flushes a hosted plugin's internal state (#57), for wavcraft#1070.
- 2026-10-26 Delta marks approved by the owner as written; "to confirm" notes removed.
- 2026-10-26 Normalized to pipeline v2 at Dev Loop: SPEC.md (upstream roadmap + our delta), docs/code-map.md, PIPELINE.md, AGENTS.fork.md; open issues sized; build issues filed for the alpha gate items upstream has no PR for. Upstream main merged (fbcbabc).
- 2026-10-26 Branch sweep: `ours` folded into main; main is the integration branch.
- 2026-10-19 Disk-backed sample source and read-ahead for streamed sources merged (feat/streamed-readahead).
- 2026-10-12 PR CI on three OSes; render cap raised to 2^31 samples; MP3 export to use our own encoder (craft-mp3).
