# PudduScroll

Desktop Instagram carousel composer. Photos sit on a horizontal strip of canvases and can cross the boundaries.

## Stack

- `eframe` 0.37 for the window
- `image` 0.25 with `png`, `jpeg`, and `webp`
- `rfd` 0.17 for file dialogs
- `pudduscroll-core` owns placement, crop, layering, clipping, and export

## Commands

- `cargo test -p pudduscroll-core` after a core change
- `cargo fmt --all`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- `cargo audit` when `Cargo.lock` changes
- `git config core.hooksPath .githooks` once per clone so the pre-commit hook runs fmt, Clippy, and tests
- Before the first edit of a request, check simplicity, reuse, maintainability, and https://refactoring.guru/design-patterns/rust, then run `powershell -NoProfile -File .cursor/hooks/mark-design.ps1 -Note "<one-line summary>"`

## Boundary

Geometry and encoded export output change in `pudduscroll-core`, with a behavior test in `crates/core/src/tests.rs`. The egui shell in `crates/app` calls that crate.
