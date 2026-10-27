# SoundCraft (our fork): spec

**Approval:** 2026-10-26, the owner: "The specs/roadmaps were developed by someone else. Nomalize them to our system" (spec = upstream roadmap + our delta, R18).

## The spec is upstream's roadmap

SoundCraft is storytold's clean-room, pure-Rust DAW aiming at parity with Avid Pro Tools. Its plan is upstream's, and we follow it:

- One-page roadmap: https://github.com/storytold/soundcraft/blob/main/ROADMAP.md
- Ranked work list (G-01 ... G-22): https://github.com/storytold/soundcraft/blob/main/docs/gaps.md
- Milestones, alpha gate and beta plan: https://github.com/storytold/soundcraft/blob/main/docs/roadmap.md
- Parity assessment: https://github.com/storytold/soundcraft/blob/main/docs/target-app-parity.md

**1.0 for us** is upstream's alpha gate passed on our build: the six core workflows in docs/roadmap.md (record from a chosen interface, record MIDI, edit, mix, bounce and reopen, exchange a session) work end to end on Mac, Windows and Linux.

## Our delta

What our fork has or wants that upstream does not. "Send upstream" items still need the owner's yes, one PR at a time (pinned issue #33).

| Item | Where | Disposition |
|---|---|---|
| Disk-backed sample source and read-ahead for streamed sources (offline mix reads through `SampleSource`) | commits e85de96, de5a3b5, 274f788 | send upstream; also reused by WavCraft |
| Correctness fixes found by our reviews (fades, crossfades, resample length, bounce caps, take files, BWF timestamps, Shuffle automation, ...) | ~40 commits ahead of upstream | send upstream, one at a time (#18 is the queue) |
| Behaviour changes upstream documented differently: float headroom kept above full scale, mono-bus unity gain, render cap 2^31 samples | #17, #15 | keep on our fork until the owner decides |
| PR CI on Linux, macOS and Windows (`.github/workflows/pr-ci.yml`) | e32bdc4 | send upstream (upstream G-14) |
| MP3 export with our own clean-room encoder (craft-mp3) | #26 | keep on our fork |
| Linux handheld track: benchmark baseline, build tuning, SIMD, GPU waveform/spectrogram, JACK/PipeWire, compatibility pass | #20-#25 | build tuning keeps on our fork; the rest send upstream where it measures faster |
| Gamepad transport, pen and touch, NPU offload, speech enhancement, edit by transcript | #27-#31 | keep on our fork |
| Open bugs found by us | #4, #6-#12, #14, #46 | send upstream when fixed and tested |

## Won'ts (ours)

- No GPL code in the tree (reference only); MIT/Apache only.
- No ROCm/HIP; GPU work goes through wgpu.
- No credit beyond the username in upstream files.
