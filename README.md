# mica

A browser grid for the T-system. Single Rust binary, single Docker container, scales 0 → N globally, portable across cloud providers via pluggable isolation drivers. Speaks the W3C WebDriver protocol so existing test clients work unchanged.

> **Status:** Phase 1 complete — Docker backend, W3C wire protocol, idle/cancel/retry teardown, status & VNC & artifact endpoints, S3 uploader, graceful shutdown, SIGHUP config reload. Phase 2+ (warm pool, K8s, microVM isolation, WASM plugins, BiDi) tracked in Linear and `docs/plans/2026-04-26-mica-strategy.md`.

## Quick start

```bash
docker run --rm -p 4444:4444 \
  -v /var/run/docker.sock:/var/run/docker.sock \
  -v $PWD/browsers.json:/etc/mica/browsers.json:ro \
  -v $PWD/video:/video \
  -v $PWD/logs:/logs \
  ghcr.io/0xteamhq/mica:latest \
  --conf /etc/mica/browsers.json \
  --video-output-dir /video \
  --log-output-dir /logs

curl http://localhost:4444/ping
curl http://localhost:4444/status
```

Then point any WebDriver client at `http://localhost:4444/wd/hub` (Selenium / Playwright / Cypress / WebdriverIO / `thirtyfour`).

## Endpoints

| Path | What |
|---|---|
| `GET /ping` | Liveness + uptime + session/queue counters |
| `GET /status` | Capacity counters + browsers map + live sessions |
| `POST /wd/hub/session` | Create a WebDriver session |
| `GET/POST/PUT/DELETE /wd/hub/session/{id}/...` | Proxy session traffic |
| `GET /vnc/{id}` (WebSocket) | Bridge to the container's VNC port |
| `GET/DELETE /video/{name}` | Serve / remove finalized video files |
| `GET/DELETE /logs/{name}` | Serve / remove finalized logs |
| `* /devtools/{id}/...` | Reverse proxy to the container's CDP port |
| `* /clipboard/{id}` | Reverse proxy to the container's clipboard port |
| `* /download/{id}/...` | Reverse proxy to the container's fileserver |

## CLI flags

Highlights:

| Flag | Default | Notes |
|---|---|---|
| `--listen` | `:4444` | Listen address. `:N` expands to `0.0.0.0:N`. |
| `--conf` | `config/browsers.json` | Browser registry. |
| `--limit` | `5` | Max parallel sessions. |
| `--timeout` | `60s` | Session idle timeout. |
| `--service-startup-timeout` | `30s` | How long to wait for the WebDriver port. |
| `--retry-count` | `1` | Upstream `POST /session` retries on 5xx. |
| `--container-network` | `default` | Docker network name. |
| `--cpu` / `--memory` | _empty_ | Default per-session limits; per-browser overrides via `browsers.json`. |
| `--enable-file-upload` | `false` | Allow upload via the fileserver port. |
| `--disable-queue` | `false` | Reject (instead of block) when full. |
| `--graceful-period` | `300s` | Drain window on SIGTERM/SIGINT. |
| `--save-all-logs` | `false` | Stream every container's stdout/stderr to `<log_output_dir>/<container_id>.log`. |
| `--s3-bucket` / `--s3-region` / `--s3-prefix` | _empty_ | S3 artifact upload (env: `MICA_S3_*`). |

`MICA_LISTEN`, `MICA_CONF`, `MICA_S3_*` env vars set the same values.

## `browsers.json`

```json
{
  "firefox": {
    "default": "126.0",
    "versions": {
      "126.0": {
        "image": "selenoid/firefox:126.0",
        "port": "4444",
        "path": "/wd/hub",
        "shmSize": 268435456
      }
    }
  }
}
```

`image` must be a string (legacy driver-mode arrays are dropped — see Phase 1 plan T7). On `SIGHUP` mica reloads the file via `arc-swap` with no service interruption.

## Local development

```bash
brew install prek               # or: cargo install prek
prek install                    # one-time

cargo build
cargo test --all
cargo run -- --conf tests/fixtures/browsers.json
```

`prek` runs the checks defined in `.pre-commit-config.yaml` on every commit
(whitespace, EOF, YAML/TOML, large-file guard, merge-conflict guard,
`cargo fmt --all`, `cargo clippy --all-targets -- -D warnings`). CI runs
`prek run --all-files`, so the same checks gate every PR.

Docker integration tests are gated:

```bash
MICA_DOCKER_TESTS=1 cargo test --test docker_integration -- --ignored
```

## Capabilities

- **Docker backend** with W3C + legacy capabilities, per-session container, idle teardown, retry on transient upstream failures
- **`browsers.json`** registry with `SIGHUP` hot reload
- **VNC / DevTools / clipboard / download** reverse proxies
- **Video / log file server** + built-in S3 artifact upload
- **K8s native** backend (Phase 3 — done)
- **Pluggable microVM isolation** (Phase 4 — Firecracker / Cloud Hypervisor / Kata / gVisor / runc)
- **WASM plugin host** (Phase 5 — partial)
- **BiDi + streaming artifacts** (Phase 6 — scaffolded)

## Roadmap

`docs/plans/2026-04-26-mica-strategy.md` lists Phases 2 → 7. Tracked in Linear under [Mica — Rust Browser Grid](https://linear.app/0xhq/project/mica-rust-browser-grid-3a5252071073).

## License

Apache-2.0.
