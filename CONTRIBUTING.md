# Contributing to mica

Thanks for taking an interest. mica is a small project — issues, PRs, and design discussion are all welcome.

## Quick links

- [Open an issue](https://github.com/0xteamhq/mica/issues/new) — bugs, regressions, build failures
- [Start a discussion](https://github.com/0xteamhq/mica/discussions) — design questions, feature ideas, "why does X work this way"
- [`good first issue`](https://github.com/0xteamhq/mica/labels/good%20first%20issue) — entry points for new contributors

## Before you start

- Reach a rough agreement on the design before writing a large PR. Either:
  - Open a Discussion or RFC issue, or
  - Comment on an existing issue you want to take

We'd rather you spend an hour aligning than a weekend on a PR we can't accept.

## Development setup

```bash
brew install prek               # or: cargo install prek
prek install                    # one-time, registers .git/hooks/pre-commit

cargo build
cargo test --all                # unit + integration (no docker)

# Docker integration tests are gated:
MICA_DOCKER_TESTS=1 cargo test --test docker_integration -- --ignored
```

`prek` runs `cargo fmt`, `cargo clippy --all-targets -- -D warnings`, plus a few hygiene checks. CI runs `prek run --all-files` against your branch — there is no path to merge with these failing.

For an architectural map of the codebase, see [`CLAUDE.md`](CLAUDE.md). It documents the seams (Backend trait surface, AppState, EventBus, isolation drivers) without dumping the file tree.

## Commit / PR conventions

- **Conventional commit-style prefixes**: `feat(...):`, `fix(...):`, `docs(...):`, `refactor(...):`, `chore(...):`. We use them in CHANGELOG generation.
- **Sign your commits** (`git commit -S`). Unsigned commits are accepted but signed is preferred.
- **One topic per PR**. Bundle related work; split unrelated work.
- **Update tests** for any behavior change. New behavior gets new tests; bug fixes get a regression test.
- **Update docs** when you change public surface (CLI flags, HTTP endpoints, capability schema, plugin contract, environment variables).

## Coding style

- Idiomatic Rust 2024. Format with `cargo fmt`.
- Clippy is `-D warnings`. If a lint is genuinely wrong, document it and `#[allow(...)]` *that one site* — never globally.
- Prefer small, well-named types over deep generics. Mica's `Backend`, `Isolation`, `Uploader` traits are deliberately small surfaces.
- Public items want `///` doc comments; modules want `//!` headers explaining what they're for and how they fit. Match the prevailing style — see `src/queue.rs` and `src/backend/mod.rs` for examples.

## What we'll likely say no to

- Adding new dependencies for trivial reasons. `Cargo.toml` is already long; we'd rather inline 50 lines than add a crate.
- Backwards-compat shims for unreleased behavior.
- Performance "optimizations" without benchmarks demonstrating impact.
- Branding / marketing changes to source comments.

## Reporting security issues

Please don't open public issues for security vulnerabilities. See [`SECURITY.md`](SECURITY.md) for the disclosure process.

## License

By contributing you agree your contributions will be licensed under [Apache-2.0](LICENSE).
