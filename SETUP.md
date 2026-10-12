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
| Build timings | TBD (first nightly run) |
