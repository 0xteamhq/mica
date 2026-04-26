# mica

A browser grid for the T-system. Single Rust binary, single Docker container, scales 0 → N globally, portable across cloud providers via pluggable isolation drivers. Speaks the W3C WebDriver protocol so existing test clients work unchanged.

> **Status:** scaffolding. Phase 1 in progress. See `docs/plans/2026-04-26-mica-strategy.md` for the strategy and `docs/plans/2026-04-26-mica-phase1.md` for the detailed Phase 1 plan.

## Local development

```bash
# One-time: activate the versioned git hooks
git config core.hooksPath .githooks

# Build, test, run
cargo build
cargo test --all
cargo run                       # listens on 0.0.0.0:4444; curl /ping
```

Pre-commit hook runs `cargo fmt --all` + `cargo clippy --all-targets -- -D warnings` whenever staged changes touch Rust or Cargo files. See `.githooks/README.md`.
