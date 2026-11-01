# Contributing

LockIn is a Windows-only study focus tracker. Issues and pull requests are welcome.

## Setup

- Rust stable with the GNU toolchain (`x86_64-pc-windows-gnu`); MSVC also builds.
- MinGW-w64 (`gcc`, `windres`) on `PATH`: `tract` compiles assembly kernels and `build.rs` compiles
  the icon resource.
- A webcam is optional; without one the camera layer stays idle.

```
cargo build --release
cargo test --release
```

`scripts/dev.ps1` rebuilds and relaunches on every change (see README → Development).

## Pull requests

- Keep one change per PR and describe what you tested by hand (camera, timer bar, main window).
- Run `cargo fmt`, `cargo clippy --release` and `cargo test --release` before pushing; CI runs the
  same on Windows.
- Privacy is a hard rule: no network calls, telemetry, or saving camera frames or screenshots.
- Keep memory and CPU use low; mention any measurable change in the PR.
- New models, fonts, images or vendored binaries need a compatible license. Add the license file and
  an entry in `THIRD_PARTY_NOTICES.md` (and `models/NOTICE.md` for models).

By contributing you agree your work is released under the MIT License (see `LICENSE`).
