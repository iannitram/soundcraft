# Cloud sandbox facts

From an access check and a setup rehearsal on 2026-10-12 (Default Cloud Environment). The rehearsal stalled before timing the build, so the first nightly run should add build and test timings here.

| | |
|---|---|
| Machine | Ubuntu 24.04, 4 cores, 15 GB RAM, no swap, ~30 GB free disk. No GPU |
| Rust | 1.97 preinstalled (workspace `rust-version` is 1.90) |
| crates | `cargo fetch` works. Bare `curl` to crates.io gets 403, which doesn't matter |
| System packages | Passwordless `sudo apt-get`. Verified install (8 s): `sudo apt-get update && sudo apt-get install -y libasound2-dev pkg-config libudev-dev libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev libx11-dev libxcursor-dev libxrandr-dev libxi-dev libxext-dev libgl1-mesa-dev libfontconfig1-dev libssl-dev` (ALSA headers are missing by default) |
| GitHub, our fork | `git push` works (dry run verified). `gh auth status` says the token is invalid; `gh api` should work for repos attached to the session |
| GitHub, other repos | Repos not attached to the session: the API and `codeload.github.com` tarballs are refused ("GitHub access to this repository is not enabled for this session"); `git ls-remote` / `git fetch` still work. Upstream is attached to the nightly routine for this reason |
| Test audio | Private test audio may come through a read-only connector named in the routine prompt. It stays in `/tmp`, is never committed, and is referred to as "test file A/B/C" |
| Waiting | Never start a long build in the background and end the turn to wait: nothing wakes an unattended run. Run builds and tests in the foreground with a long timeout; split long ones per package |
| Build timings | 2026-10-12, cold `target/`: apt install 12 s; `cargo fetch` 12 s; `cargo build -p soundcraft-audio-io -p soundcraft-dsp --tests` 1 m 17 s; `cargo test -p soundcraft-model` 21 s; `cargo test -p soundcraft-engine` ~2 m (first build); `cargo clippy --workspace --all-targets` 2 m 34 s (deps partly built); `cargo test --workspace --no-run` 9 m 37 s, then the 677 tests run in 42 s; `cargo xtask wasm` 2 m 14 s (needs `rustup target add wasm32-unknown-unknown`, which works); release `--example bench` 55 s incremental, `--profile max` 1 m 42 s. Whole `cargo xtask ci` fits in a session if run step by step (each step under the 10-minute tool limit). Run 2 (fresh container, cold `target/`): `cargo test --no-run -p soundcraft-audio-io -p soundcraft-dsp -p soundcraft-model -p soundcraft-engine -p soundcraft-playback` 2 m 52 s; `cargo test -p soundcraft-engine` then ~20 s per branch; release `--example bench` 59 s first build, 28 s incremental; wasm32 target needs `rustup target add wasm32-unknown-unknown` again in a new container. Run 3 (fresh container): `cargo test -p soundcraft-mix` first build 1 m 14 s; `cargo test -p soundcraft-engine` 2 m 17 s first, ~1 min after a mix change; `cargo check -p soundcraft-audio-io --target wasm32-unknown-unknown` 24 s; `target/` stayed under 6 GB |
| Disk | `target/` reached ~26 GB (of a ~30 GB allowance) after the workspace test build plus release and max profiles |
| Other tools | No `xxd`, no `/usr/bin/time`; use Python or `/proc/<pid>/status` (VmHWM) for peak memory |
| Safety check | The sandbox's auto-mode check refuses compiling or running downloaded third-party code (it blocked building minimp3 in C, and then a Python script that walked minimp3's decoder tables). Reading such files is fine; plan table ports so a person approves them |
