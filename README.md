# mica

A browser grid for the T-system. Single Rust binary, single Docker container, scales 0 → N globally, portable across cloud providers via pluggable isolation drivers. Speaks the W3C WebDriver protocol so existing test clients work unchanged.

> **Status:** scaffolding. Phase 1 in progress. See `docs/plans/2026-04-26-mica-strategy.md` for the strategy and `docs/plans/2026-04-26-mica-phase1.md` for the detailed Phase 1 plan.

## Local development

```bash
# One-time setup: install prek (Rust-native pre-commit) and the hooks
brew install prek               # or: cargo install prek
prek install                    # writes .git/hooks/pre-commit

# Build, test, run
cargo build
cargo test --all
cargo run                       # listens on 0.0.0.0:4444; curl /ping
```

`prek` runs the checks defined in `.pre-commit-config.yaml` on every commit:
whitespace / EOF / YAML / TOML / merge-conflict / large-file checks, plus
`cargo fmt --all` and `cargo clippy --all-targets -- -D warnings`. CI runs
`prek run --all-files`, so the same checks gate every PR. Bypass once with
`git commit --no-verify` if you must.
