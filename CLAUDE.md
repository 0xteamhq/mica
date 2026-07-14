# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

mica is a W3C-WebDriver-compatible browser grid in Rust. Single binary, single Docker container, scales 0→N globally. Roadmap and phase status live in `docs/plans/2026-04-26-mica-strategy.md` and the Linear project [Mica — Rust Browser Grid](https://linear.app/0xhq/project/mica-rust-browser-grid-3a5252071073).

Rust edition 2024, MSRV 1.88. Single crate (`mica`) with one library + two binaries (`mica`, `mica-rootfs`).

## Commands

```bash
# Local dev
cargo build
cargo test --all                                            # unit + integration (no docker)
cargo run -- --conf tests/fixtures/browsers.json
cargo test --test <name> -- <pattern>                       # single test file / name filter

# Docker integration tests — gated by env var, marked #[ignore]
MICA_DOCKER_TESTS=1 cargo test --test docker_integration -- --ignored

# Admin dashboard (ui/ — React+Vite, embedded via rust-embed).
# The `ui` cargo feature is OFF by default so cargo test needs no Node.
npm ci --prefix ui && npm run build --prefix ui             # produces ui/dist
cargo build --features ui                                   # embeds ui/dist at /admin
npm run dev --prefix ui                                     # UI dev server, proxies to :4444

# Pre-commit (uses prek, NOT classic pre-commit)
brew install prek                                           # or: cargo install prek
prek install                                                # writes .git/hooks/pre-commit (one-time)
prek run --all-files                                        # what CI runs

# CI runs five jobs in parallel: prek (fmt + clippy + lint),
# cargo test --all --locked, admin UI build, cargo build --release
# --locked --features ui (builds ui/dist first), docker build.
# RUSTFLAGS=-D warnings is set globally in CI; clippy is -D warnings too.
```

`prek` is a Rust reimpl of pre-commit — config is in `.pre-commit-config.yaml` and Helm templates under `deploy/k8s/charts/**/templates/` are excluded from `check-yaml` because they contain Go template directives.

## Architecture

### Layered backend stack

```
HTTP handler (handlers/create.rs)
   └── AppState.queue.acquire()         queue.rs       — bounded permit + counters
   └── AppState.backend.start()         backend/       — Backend trait
         ├── DockerBackend                              — local docker.sock
         ├── K8sBackend (Phase 3)                       — kube-rs
         └── MockBackend                                — tests
```

`PooledBackend` (`pool.rs`) wraps any `Backend` and serves warm sandboxes from a per-`(image, screen_resolution, env_hash)` pool, falling through to the real backend on miss. Enabled by `--warm-pool-min > 0`.

Backends only know how to **start** — stopping is a per-session capability returned in `StartedSession`, because Firecracker / Kata snapshots need to capture state at start time. Never add a `backend.stop(id)` free function.

### Isolation is orthogonal to backend

`src/isolation/` defines an `Isolation` trait separate from `Backend`. Drivers (`runc`, `gvisor`, `kata`, `firecracker`, `cloud_hypervisor`) feed either:
- a K8s `runtimeClassName` (gvisor / kata via K8sBackend), or
- a direct microVM lifecycle (firecracker / cloud_hypervisor — scaffolded, not yet wired).

`isolation/capability.rs` probes the host at boot (`KVM`, `runsc` binary, `kata-runtime`, in-cluster k8s) and `select_driver()` picks the most-isolated driver that's actually runnable. Operator override via `--isolation=<x>`.

The same OCI rootfs feeds every driver — that's the no-vendor-lock-in fence. `src/bin/mica-rootfs.rs` is the rootfs+kernel builder (currently a scaffold).

### Application state

`AppState` (`state.rs`) is the dependency container handed to every axum handler. Notable fields:
- `config_swap: Arc<ArcSwap<Config>>` — hot-reloadable browser registry; SIGHUP triggers `config_swap.store(...)` with no service interruption.
- `queue: Queue` — single tokio semaphore for the hard cap, three `AtomicUsize` counters (`queued`, `pending`, `used`) surfaced via `/status` and `/ping`.
- `sessions: SessionMap` — `DashMap`-backed; each session owns its idle-watcher tokio task with a oneshot cancel channel.
- `events: EventBus` — fan-out for `FileCreated` and `SessionStopped` events. Each emit is `tokio::spawn`-per-listener so a slow uploader cannot stall mica.

### Plugin extensibility

Two extension points share the same WIT contract (`wit/`, package `mica:plugin@0.1.0`):
- `Uploader` trait (`src/upload/mod.rs`) — built-in `S3Uploader`; plugins implement the same trait.
- WASM Component Model plugins via `wasmtime` 26 (`src/plugins/mod.rs`) — `lifecycle / session / artifact / http` exports + capability-gated host imports (`host-log`, `clock`, `http-client`, `s3-write`, `state`).

Capability gating is fail-closed: a plugin importing a non-granted host capability fails to instantiate at startup.

### Router mode (Phase 7)

`mica --router --nodes nodes.json` runs the same binary as a stateless GGR-equivalent tier (`src/router/`). `main.rs` branches to `router::serve::run` **before** backend/isolation/wasmtime init. `RouterState` (registry + reqwest client) replaces `AppState` — no Queue, no SessionMap. Session ids returned to clients are `base64url(node_name).upstream_id` (`src/router/session_id.rs`) so any router replica routes any request statelessly. A background poller caches each node's `/status` for health + capability placement; aggregated `/status` is a strict superset of the node shape (`"router": true`, `nodes: [...]`). `deploy/routing/README.md` is the ops doc (nodes.json reference, drain runbook). The node-side `draining` flag (`AppState.draining`, `/readyz` 503, `/status.draining`) is what the router's placement respects.

### Admin control plane (Phase 8)

`/admin` serves a React/Vite SPA from `ui/dist`, embedded by rust-embed behind the off-by-default `ui` cargo feature (`src/handlers/admin/assets.rs`); feature-off builds serve a placeholder so route shape is identical. `/admin/api/*` (`src/handlers/admin/`): `state` (dashboard snapshot), `events` (SSE from the `AdminEvent` broadcast on EventBus + 2s stats frames), kill/drain/reload ops, raw-bytes browsers.json editing (file stays source of truth; never round-trip through serde — unknown fields would drop), users CRUD (htpasswd v2 `name:hash[:admin]`), quotas. Mutating routes take the `RequireAdmin` extractor (`src/auth.rs`); role comes from the htpasswd third column. Per-user quotas (`src/quota.rs`) are enforced in `create.rs` BEFORE `queue.acquire()` — the `QuotaGuard` rides in the same holder as the queue `Permit`. `src/reload.rs::reload_all` is the single reload path for SIGHUP and `POST /admin/api/config/reload`.

### Graceful shutdown

`shutdown::signal_future()` resolves on SIGTERM / SIGINT. `axum::serve(...).with_graceful_shutdown(...)` stops accepting new connections; `shutdown::drain` walks `SessionMap` and removes every session, which fires each session's cancel hook (release queue permit, kill upstream container, emit `SessionStopped`). `--graceful-period` bounds the drain.

## Phase status (read this before assuming a feature is shipped)

| Phase | What | Status |
|---|---|---|
| 1 | Docker backend, W3C wire, idle/cancel/retry, /vnc, /video, /logs, S3 uploader, SIGHUP reload | done |
| 2 | `chrome-headless-shell` image + warm pool + 2 GiB `/dev/shm` | done |
| 3 | K8sBackend (kube-rs) + Helm chart at `deploy/k8s/charts/mica/` | done |
| 4 | Isolation drivers — runc/gvisor/kata wired, firecracker/cloud_hypervisor scaffolded | partial |
| 5 | WASM plugin host — WIT contract done (`wit/`), wasmtime host scaffolded (`src/plugins/`) | partial |
| 6 | BiDi + streaming artifacts — `src/streaming/` traits defined, CDP/ffmpeg/S3-multipart wiring pending (0XT-82) | scaffold |
| 7 | Router mode (`--router`, GGR equivalent) + aggregated /status (ggr-ui equivalent) — `src/router/` | done |
| 8 | Admin control plane — `/admin` React dashboard (embedded, `--features ui`) + `/admin/api/*` (state, SSE events, kill/drain/reload, registry editing, users, quotas) — `src/handlers/admin/`, `ui/` | done |

Comments like "Phase X" and ticket IDs (`P4.6`, `0XT-NN`, `T<n>`) reference Linear and the strategy doc — search there for context, not training data.

## Conventions

- The Backend trait surface is deliberately small (`start()` returning `StartedSession`). Resist adding methods; bake the new capability into `StartParams` or `StartedSession` instead.
- The HTTP wire format and `browsers.json` schema are the public contract. Document any wire change in the source comment with the impacted client-side keys / headers.
- `WdError` (`src/error.rs`) is the canonical W3C error type. Backend / queue / config errors all map into it via `From` impls; handlers return `Result<..., WdError>` and `IntoResponse` does the JSON encoding.
- Tests in `tests/` use `MockBackend` for anything that doesn't specifically need Docker. `tests/fixtures/browsers.json` is the canonical config fixture.
