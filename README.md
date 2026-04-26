# mica

A W3C-WebDriver-compatible browser grid in Rust. Single binary, single Docker container, scales 0 → N. Run it on a laptop, a single VM, Kubernetes, or behind any L7 load balancer.

[![CI](https://github.com/0xteamhq/mica/actions/workflows/ci.yml/badge.svg)](https://github.com/0xteamhq/mica/actions/workflows/ci.yml)
[![License: Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](#license)

## Why mica

- **One binary.** No companion services, no agents on the browser nodes. The same artifact you run locally is what runs in production.
- **W3C WebDriver wire format.** Selenium, Playwright, Cypress, WebdriverIO, and `thirtyfour` all work without changes.
- **Scales 0 → N.** Backends for local Docker and Kubernetes. Pluggable isolation drivers (runc, gVisor, Kata, and microVMs) so you decide the security/perf tradeoff per cluster.
- **Production surface built in.** `/healthz` + `/readyz` for orchestrators, `/metrics` for Prometheus, structured JSON logs, graceful drain on `SIGTERM`, hot config reload on `SIGHUP`, S3 artifact upload, warm pool for sub-second session starts.
- **Extensible.** WebAssembly Component Model plugins (`wit/`) hook the session lifecycle, artifact upload, and HTTP middleware paths.

## Quick start

### Docker

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
```

### Kubernetes (Helm)

```bash
helm install mica ./deploy/k8s/charts/mica \
  --namespace mica --create-namespace \
  -f deploy/k8s/examples/values-production.yaml
```

See [`deploy/k8s/charts/mica/README.md`](deploy/k8s/charts/mica/README.md) for values, RBAC scopes, and ingress patterns.

### From source

```bash
cargo run -- --conf tests/fixtures/browsers.json
```

## Use it

### `curl`

```bash
curl -X POST http://localhost:4444/wd/hub/session \
  -H 'Content-Type: application/json' \
  -d '{"capabilities":{"alwaysMatch":{"browserName":"chrome"}}}'
```

### Selenium (Python)

```python
from selenium import webdriver

opts = webdriver.ChromeOptions()
opts.set_capability("mica:options", {"enableVNC": True, "enableVideo": True})

driver = webdriver.Remote("http://localhost:4444/wd/hub", options=opts)
driver.get("https://example.com")
driver.quit()
```

### Playwright (Node)

```js
import { chromium } from "playwright";

const browser = await chromium.connect({
  wsEndpoint: "ws://localhost:4444/playwright/chromium",
});
```

## Endpoints

| Path | Purpose |
|---|---|
| `POST /wd/hub/session` | Create a WebDriver session |
| `GET/POST/PUT/DELETE /wd/hub/session/{id}/...` | Proxy session traffic |
| `GET /vnc/{id}` (WebSocket) | Live VNC feed of the session |
| `GET/DELETE /video/{name}` | Serve / delete recorded video |
| `GET/DELETE /logs/{name}` | Serve / delete container logs |
| `* /devtools/{id}/...` | Reverse proxy to Chrome DevTools (CDP) |
| `* /clipboard/{id}` | Reverse proxy to the container's clipboard |
| `* /download/{id}/...` | Reverse proxy to the container's download server |
| `GET /ping` | Liveness + uptime + queue counters |
| `GET /status`, `/wd/hub/status` | Capacity + browsers map + live sessions |
| `GET /healthz` | Kubernetes-style liveness probe |
| `GET /readyz` | Kubernetes-style readiness probe |
| `GET /metrics` | Prometheus metrics (text format) |

## Configuration

### `browsers.json`

```json
{
  "chrome": {
    "default": "126.0",
    "versions": {
      "126.0": {
        "image": "selenoid/chrome:126.0",
        "port": "4444",
        "path": "/",
        "shmSize": 268435456
      }
    }
  }
}
```

`SIGHUP` triggers a zero-downtime reload via `arc-swap`. The legacy driver-mode array form of `image` is not supported — use a string per the example above.

### CLI / environment

The most-used flags. Run `mica --help` for the full list.

| Flag | Default | Notes |
|---|---|---|
| `--listen` (`MICA_LISTEN`) | `:4444` | Listen address |
| `--conf` (`MICA_CONF`) | `config/browsers.json` | Browser registry |
| `--limit` | `5` | Max parallel sessions per replica |
| `--timeout` / `--max-timeout` | `60s` / `1h` | Per-session idle timeout, and the cap a client can request via `mica:options.sessionTimeout` |
| `--service-startup-timeout` | `30s` | How long to wait for the WebDriver port |
| `--retry-count` | `1` | Upstream `POST /session` retries on 5xx |
| `--graceful-period` | `300s` | Drain window on SIGTERM/SIGINT |
| `--container-network` | `default` | Docker network |
| `--cpu` / `--memory` | _empty_ | Default per-session limits; per-browser overrides via `browsers.json` |
| `--enable-file-upload` | `false` | Allow upload via the container's fileserver port |
| `--disable-queue` | `false` | Reject (instead of block) when full |
| `--save-all-logs` | `false` | Stream every container's stdout/stderr to `<log_output_dir>/<container_id>.log` |
| `--warm-pool-min` / `--warm-pool-max` / `--warm-pool-idle-ttl` | `0` / `16` / `5m` | Warm pool sizing per `(image, screen_resolution, env_hash)` |
| `--s3-bucket` (`MICA_S3_BUCKET`) | _empty_ | S3 artifact upload (also `--s3-region`, `--s3-prefix`) |
| `--isolation` | auto-detect | `runc` / `gvisor` / `kata` / `firecracker` / `cloud-hypervisor` |

### Vendor capabilities

mica recognizes a `mica:options` block in WebDriver capabilities:

```json
{
  "capabilities": {
    "alwaysMatch": {
      "browserName": "chrome",
      "mica:options": {
        "enableVNC": true,
        "enableVideo": true,
        "screenResolution": "1280x1024x24",
        "sessionTimeout": "10m",
        "name": "checkout-flow",
        "env": ["TZ=UTC"]
      }
    }
  }
}
```

The `X-Mica-No-Wait: 1` request header on `POST /wd/hub/session` returns `503` instead of blocking when the queue is full.

## Status

| Capability | State |
|---|---|
| Docker backend, W3C wire, idle/cancel/retry, VNC/video/logs, S3 upload, SIGHUP reload | ✅ shipped |
| Warm pool (sub-second session starts) | ✅ shipped |
| Kubernetes backend + Helm chart | ✅ shipped |
| Production health probes (`/healthz`, `/readyz`) and Prometheus `/metrics` | ✅ shipped |
| Isolation: runc, gVisor (`runtimeClassName: gvisor`), Kata (`runtimeClassName: kata`) | ✅ shipped |
| Isolation: Firecracker, Cloud Hypervisor (direct microVM lifecycle) | 🚧 scaffolded |
| WASM plugin host (`wasmtime` + Component Model, contract in `wit/`) | 🚧 partial |
| BiDi WebSocket multiplex, streaming video to S3 | 🚧 scaffolded |

## Architecture

`AppState` → `Queue` → `PooledBackend` → `Backend` (Docker / K8s / Mock). Stopping is a per-session capability, not a method on `Backend`, so backends that need to capture state at start time (Firecracker, Kata) compose cleanly. Isolation drivers are orthogonal to backends: they feed K8s `runtimeClassName` or microVM lifecycle, with the same OCI rootfs.

For more, see [`CLAUDE.md`](CLAUDE.md) (architectural seams) and [`docs/plans/`](docs/plans/) (design decisions).

## Development

```bash
brew install prek               # or: cargo install prek
prek install                    # one-time, registers .git/hooks/pre-commit

cargo build
cargo test --all                # unit + integration (no docker)
MICA_DOCKER_TESTS=1 cargo test --test docker_integration -- --ignored
```

`prek` runs `cargo fmt`, `cargo clippy --all-targets -- -D warnings`, and a few hygiene checks. CI runs the same set with `prek run --all-files`.

Bug reports, design discussion, and pull requests welcome via [GitHub Issues](https://github.com/0xteamhq/mica/issues) and PRs.

## License

Apache License 2.0. See [`LICENSE`](LICENSE).
