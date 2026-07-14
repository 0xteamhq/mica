# mica/lightpanda-webdriver

[Lightpanda](https://lightpanda.io) — an ultra-light, CDP-native headless
browser built for AI/automation — packaged with a thin **WebDriver-classic
bridge** so it plugs into mica with **zero changes to mica** (the "Option A"
integration).

## Why the bridge exists

Lightpanda speaks the **Chrome DevTools Protocol**, not W3C WebDriver.
mica's session-create handler does:

```
POST {upstream}/session      ->  expects value.sessionId   (src/handlers/create.rs)
```

A raw CDP endpoint doesn't serve `/session`, so mica can't create a
session against Lightpanda directly. This image runs Lightpanda (CDP on
`:9222`) behind a small Node/`puppeteer-core` server (`bridge/server.js`)
that presents the W3C surface on `:4444` and drives Lightpanda over CDP.
From mica's point of view it's an ordinary WebDriver browser.

```
mica  --POST /session-->  bridge :4444  --CDP-->  lightpanda :9222
```

## Build

```bash
docker buildx build \
  --build-arg LIGHTPANDA_VERSION=nightly \
  -t ghcr.io/0xteamhq/lightpanda:nightly \
  docker/lightpanda-webdriver/
```

Published to GitHub Container Registry as `ghcr.io/0xteamhq/lightpanda`.

### Releasing via CI

All browser images are described by one manifest at the repo root,
[`browser-images.json`](../../browser-images.json), and built by the shared
[`browser-images`](../../.github/workflows/browser-images.yml) workflow (a
matrix over the manifest). Lightpanda's entry:

```json
{ "name": "lightpanda", "context": "docker/lightpanda-webdriver",
  "versionArg": "LIGHTPANDA_VERSION", "version": "nightly" }
```

The workflow builds + pushes `ghcr.io/0xteamhq/<name>:<version>` (plus
`:latest`) for `linux/amd64,linux/arm64` whenever the manifest or anything
under `docker/**` changes on `main`, or on manual dispatch. So cutting a new
Lightpanda image is a one-line change: bump `version` in the manifest.

- `LIGHTPANDA_VERSION` — GitHub release tag (default `nightly`).
- `LIGHTPANDA_URL` — override the full binary URL for pinned / air-gapped
  builds (mirrors the `chrome-headless-shell` image's `CHROME_URL` knob).
- amd64 is Lightpanda's primary target; arm64 builds only if a matching
  `lightpanda-aarch64-linux` release asset exists.

## Use from mica

Add to `browsers.json` — note `port: "4444"` (the **bridge**, not
Lightpanda's 9222) and `path: ""`:

```json
{
  "lightpanda": {
    "default": "nightly",
    "versions": {
      "nightly": {
        "image": "ghcr.io/0xteamhq/lightpanda:nightly",
        "port": "4444",
        "path": ""
      }
    }
  }
}
```

> **`path` must be empty, not `"/"`.** mica builds the upstream URL as
> `host:port + path` and then appends `/session`. A `path` of `"/"` yields
> `host:port//session` (double slash), which the bridge's router won't
> match. Leave `path` empty so the URL is `host:port/session`.

Then drive it like any other browser:

```python
from selenium import webdriver
opts = webdriver.ChromeOptions()          # any W3C client works
opts.set_capability("browserName", "lightpanda")
driver = webdriver.Remote("http://localhost:4444/wd/hub", options=opts)
driver.get("https://example.com")
print(driver.title)
driver.quit()
```

## Supported commands

The bridge implements the commonly-used slice of WebDriver-classic:

| Area | Commands |
|---|---|
| Session | `POST /session`, `DELETE /session/{id}` |
| Navigation | `POST/GET /session/{id}/url`, `/title`, `/source` |
| Elements | `POST /element` + `/elements` (css selector, xpath, tag name, link text, partial link text) |
| Interaction | element `/click`, `/value` (sendKeys), `/clear`, `/text`, `/attribute/{n}`, `/property/{n}` |
| Scripting | `POST /session/{id}/execute/sync` and `/async` (JSON-serializable args only) |

Anything else returns a well-formed W3C `unknown command` error rather
than failing opaquely.

### Known limitations

- **No screenshots.** Lightpanda does not render pixels;
  `GET /screenshot` returns W3C `unsupported operation`.
- **CDP subset.** The bridge is only as capable as Lightpanda's CDP
  implementation — DOM + JS execution is solid; rich interaction/rendering
  is not. See the [Lightpanda CDP coverage docs](https://lightpanda.io).
- **executeScript args** must be JSON-serializable — passing element
  handles as script arguments is not supported.
- puppeteer-core and Lightpanda must be CDP-compatible; if a Lightpanda
  release changes protocol behavior, bump `puppeteer-core` in
  `bridge/package.json`.

## Design: the Aerokube / Selenoid container contract

This image deliberately follows the pattern [Aerokube](https://github.com/aerokube)
pioneered with Selenoid — a browser container that exposes a fixed set
of ports the grid knows how to consume:

| Port | Surface | Consumed by mica via |
|---|---|---|
| `4444` | W3C WebDriver (the bridge) | `POST /session` create + `/session/{id}/*` proxy |
| `7070` | Lightpanda **CDP** | the built-in `/devtools/{session}` relay (`src/handlers/relay.rs`) |

mica already probes exactly these ports (`src/backend/docker.rs` — VNC
`5900`, devtools `7070`, fileserver `8080`, clipboard `9090`), which are
Selenoid's conventions. By putting Lightpanda's CDP on `7070`, clients get
**both** a WebDriver session *and* raw per-session CDP through mica —
Selenoid's signature feature — with no changes to mica.

VNC/video (`5900`), fileserver (`8080`) and clipboard (`9090`) are not
applicable to a non-rendering headless engine, so they're left unexposed.

## Relationship to `chrome-headless-shell`

The sibling `docker/chrome-headless-shell` image exposes **raw CDP** on
`:9222`. That only fits mica's warm-pool / relay / BiDi paths — not the
WebDriver `POST /session` create flow. This image adds the WebDriver
translation layer, which is what makes the standard create flow work for a
CDP-only engine.
