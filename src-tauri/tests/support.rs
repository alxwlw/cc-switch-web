//! Fork-only top-level test binaries (`web_api_*`, `bootstrap*`, ...) share the
//! helper module that upstream keeps under `tests/integration/support/`.
//! `include!` instead of `#[path]` so each binary keeps its plain `mod support;`.
include!("integration/support/mod.rs");
