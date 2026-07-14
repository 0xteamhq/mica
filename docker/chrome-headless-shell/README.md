# mica/chrome-headless-shell

Minimal Chrome image for mica's warm pool. Built around Chrome for Testing's `chrome-headless-shell` binary (Chrome ≥ 120) — no X11, no Wayland, no D-Bus, ~80 MB.

## Build

```bash
docker buildx build \
  --platform linux/amd64 \
  --build-arg CHROME_VERSION=126.0.6478.182 \
  -t mica/chrome-headless-shell:126.0.6478.182 \
  -t mica/chrome-headless-shell:126 \
  docker/chrome-headless-shell/
```

> **amd64 only.** Chrome for Testing does not publish a `linux-arm64`
> `chrome-headless-shell` binary, so this image is x86-64 only (the
> `browser-images.json` entry pins `platforms: linux/amd64`).

In CI it's built + pushed to `ghcr.io/0xteamhq/chrome-headless-shell` by the
[`browser-images`](../../.github/workflows/browser-images.yml) workflow,
driven by the root `browser-images.json` manifest.

## Use from mica

Add to `browsers.json`:

```json
{
  "chrome": {
    "default": "126",
    "versions": {
      "126": {
        "image": "ghcr.io/0xteamhq/mica/chrome-headless-shell:126",
        "port": "9222",
        "path": "",
        "shmSize": 2147483648
      }
    }
  }
}
```

`shmSize` is optional — `DockerBackend` defaults `/dev/shm` to 2 GiB (P2.4). Browserless documents 64 MB (Chrome's default) as the floor that crashes under load.

## License

Chromium is BSD-3 + LGPL where applicable — redistribution as a downstream image is permitted; verify with your legal team if you ship to customers under a different name.
