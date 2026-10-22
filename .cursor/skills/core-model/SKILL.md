---
name: core-model
description: Change placement, crop, layering, clipping, canvas ratio, or export in PudduScroll. Use when editing canvas geometry or export output.
---

# Core model

1. Change the behavior in `pudduscroll-core`.
2. Extend `crates/core/src/tests.rs` with a test of the observable behavior. Done when `cargo test -p pudduscroll-core` passes.
3. Update `crates/app` so the egui shell calls the core API. Done when the shell compiles against that API.
