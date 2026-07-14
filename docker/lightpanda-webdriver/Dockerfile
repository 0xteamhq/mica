# syntax=docker/dockerfile:1.7
#
# mica/lightpanda-webdriver — Lightpanda (CDP-native) fronted by a thin
# WebDriver-classic bridge so it plugs into mica's `POST /session`
# create flow with ZERO changes to mica itself (Option A).
#
# Why a bridge at all: Lightpanda speaks Chrome DevTools Protocol, not
# W3C WebDriver. mica's create handler does `POST {upstream}/session`
# and reads `value.sessionId` — a contract raw CDP does not satisfy.
# The bridge (bridge/server.js, puppeteer-core over CDP) presents that
# W3C surface on port 4444 and drives Lightpanda on 9222 underneath.
#
# Build (amd64 is Lightpanda's primary target; arm64 if a matching
# release asset exists):
#   docker buildx build \
#     --build-arg LIGHTPANDA_VERSION=nightly \
#     -t ghcr.io/0xteamhq/lightpanda:nightly \
#     docker/lightpanda-webdriver/
#
# Air-gapped / pinned builds: override LIGHTPANDA_URL to point at a
# vendored binary instead of the GitHub release.

ARG LIGHTPANDA_VERSION=nightly

# ---- fetch the Lightpanda binary ----
FROM debian:bookworm-slim AS fetch
ARG LIGHTPANDA_VERSION
ARG TARGETARCH
RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*

# Map Docker's TARGETARCH onto Lightpanda's release asset naming.
RUN case "$TARGETARCH" in \
      amd64) echo "x86_64-linux"  > /tmp/lp_arch ;; \
      arm64) echo "aarch64-linux" > /tmp/lp_arch ;; \
      *) echo "unsupported arch: $TARGETARCH" >&2 && exit 1 ;; \
    esac

# Default to the published GitHub release asset; operators can override
# the whole URL for a pinned or internally-mirrored binary.
ARG LIGHTPANDA_URL=
RUN ARCH=$(cat /tmp/lp_arch) && \
    URL="${LIGHTPANDA_URL:-https://github.com/lightpanda-io/browser/releases/download/${LIGHTPANDA_VERSION}/lightpanda-${ARCH}}" && \
    echo "downloading $URL" && \
    curl -fsSL "$URL" -o /tmp/lightpanda && \
    chmod +x /tmp/lightpanda && \
    /tmp/lightpanda version || echo "note: 'lightpanda version' probe failed (non-fatal at build time)"

# ---- install the bridge's node deps ----
FROM node:20-bookworm-slim AS bridge-deps
WORKDIR /opt/bridge
COPY bridge/package.json bridge/package-lock.json* ./
# puppeteer-core pulls NO browser binary (unlike puppeteer), so the
# image stays lean. PUPPETEER_SKIP_DOWNLOAD belt-and-suspenders.
ENV PUPPETEER_SKIP_DOWNLOAD=1
RUN npm install --omit=dev --no-audit --no-fund

# ---- runtime ----
FROM node:20-bookworm-slim
ARG LIGHTPANDA_VERSION
LABEL org.opencontainers.image.source="https://github.com/0xteamhq/mica" \
      org.opencontainers.image.description="Lightpanda + WebDriver-classic bridge for mica" \
      org.opencontainers.image.version="${LIGHTPANDA_VERSION}"

RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates tini \
    && rm -rf /var/lib/apt/lists/*

COPY --from=fetch /tmp/lightpanda /usr/local/bin/lightpanda
COPY --from=bridge-deps /opt/bridge/node_modules /opt/bridge/node_modules
COPY bridge/package.json /opt/bridge/package.json
COPY bridge/server.js    /opt/bridge/server.js
COPY entrypoint.sh       /usr/local/bin/mica-lightpanda-entrypoint

RUN chmod +x /usr/local/bin/mica-lightpanda-entrypoint

# Two surfaces, both following the Selenoid/Aerokube container contract:
#   4444 — W3C WebDriver (the bridge); mica's create + proxy flow
#   7070 — Lightpanda CDP; mica's /devtools/{session} relay passes it
#          through so clients get raw CDP too. (VNC 5900 / fileserver
#          8080 / clipboard 9090 are N/A for a non-rendering engine.)
ENV LIGHTPANDA_SERVE_HOST=0.0.0.0 \
    LIGHTPANDA_HOST=127.0.0.1 \
    LIGHTPANDA_PORT=7070 \
    BRIDGE_PORT=4444
EXPOSE 4444 7070
USER 1000:1000

# tini reaps the Lightpanda child and forwards SIGTERM so mica's
# DockerStopper (SIGTERM -> 10s -> SIGKILL) shuts the pair down cleanly.
ENTRYPOINT ["/usr/bin/tini", "--", "/usr/local/bin/mica-lightpanda-entrypoint"]
