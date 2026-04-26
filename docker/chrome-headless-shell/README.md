# mica/chrome-headless-shell

Minimal Chrome image for mica's warm pool. Built around Chrome for Testing's `chrome-headless-shell` binary (Chrome ≥ 120) — no X11, no Wayland, no D-Bus, ~80 MB.

## Build

```bash
docker buildx build \
  --platform linux/amd64,linux/arm64 \
  --build-arg CHROME_VERSION=126.0.6478.182 \
  -t mica/chrome-headless-shell:126.0.6478.182 \
  -t mica/chrome-headless-shell:126 \
  docker/chrome-headless-shell/
```

In CI we publish to `ghcr.io/0xteamhq/mica/chrome-headless-shell` via the release workflow once a tag is cut.

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
