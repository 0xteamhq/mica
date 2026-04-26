# syntax=docker/dockerfile:1.7
#
# mica/chrome-headless-shell — minimal Chrome image for the warm pool.
#
# Builds against Chrome for Testing's chrome-headless-shell binary
# (Chrome >= 120). No X11, no Wayland, no D-Bus — Chrome's native
# headless shell. Image is roughly 80 MB vs ~300 MB for full chrome.
#
# Build: docker buildx build --platform linux/amd64,linux/arm64 -t mica/chrome-headless-shell:<ver> docker/chrome-headless-shell/
#
# IMPORTANT: operator must run with --shm-size=2g (or pass shmSize in
# browsers.json). mica's DockerBackend defaults shmSize to 2 GiB
# (P2.4) so this is automatic when launched through mica.

ARG CHROME_VERSION=126.0.6478.182

FROM debian:bookworm-slim AS download
ARG CHROME_VERSION
ARG TARGETARCH
RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates curl unzip jq \
    && rm -rf /var/lib/apt/lists/*

# Resolve the platform identifier Chrome for Testing uses.
RUN case "$TARGETARCH" in \
      amd64)  echo "linux64" > /tmp/cft_platform ;; \
      arm64)  echo "linux-arm64" > /tmp/cft_platform ;; \
      *) echo "unsupported arch: $TARGETARCH" >&2 && exit 1 ;; \
    esac

# Pull the binary from the Chrome for Testing downloads JSON.
# (Operators in air-gapped environments can override the URL via the
# CHROME_URL build arg.)
ARG CHROME_URL=
RUN PLATFORM=$(cat /tmp/cft_platform) && \
    URL="${CHROME_URL:-$(curl -fsSL https://googlechromelabs.github.io/chrome-for-testing/known-good-versions-with-downloads.json \
        | jq -r --arg v "$CHROME_VERSION" --arg p "$PLATFORM" \
            '.versions[] | select(.version == $v) | .downloads."chrome-headless-shell"[] | select(.platform == $p) | .url')}" && \
    test -n "$URL" || (echo "no download URL for $CHROME_VERSION on $PLATFORM" >&2; exit 1) && \
    curl -fsSL "$URL" -o /tmp/cft.zip && \
    unzip -q /tmp/cft.zip -d /opt && \
    mv /opt/chrome-headless-shell-* /opt/chrome-headless-shell

FROM debian:bookworm-slim
ARG CHROME_VERSION
LABEL org.opencontainers.image.source="https://github.com/0xteamhq/mica" \
      org.opencontainers.image.description="Chrome headless-shell for mica's warm pool" \
      org.opencontainers.image.version="${CHROME_VERSION}" \
      mica.shm.recommended="2g"

# Runtime deps only — no X11. fonts-liberation keeps page rendering
# sane; ca-certificates so HTTPS works inside the container.
RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates fonts-liberation libnss3 libnspr4 libgbm1 \
        libxkbcommon0 libdbus-1-3 \
    && rm -rf /var/lib/apt/lists/*

COPY --from=download /opt/chrome-headless-shell /opt/chrome-headless-shell
RUN ln -s /opt/chrome-headless-shell/chrome-headless-shell /usr/local/bin/chrome-headless-shell

# Standard WebDriver port — existing legacy `browsers.json` entries
# expecting these ports plug in unchanged.
EXPOSE 4444 5900 7070 8080 9090
USER 1000:1000
ENTRYPOINT ["/usr/local/bin/chrome-headless-shell", "--remote-debugging-port=9222", "--remote-debugging-address=0.0.0.0", "--no-sandbox"]
