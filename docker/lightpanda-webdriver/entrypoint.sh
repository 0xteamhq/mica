#!/bin/sh
# Start Lightpanda (CDP, port 9222) and the WebDriver bridge (W3C, port
# 4444) as a pair. mica only talks to the bridge; the bridge drives
# Lightpanda over CDP on localhost inside the container.
#
# tini (the ENTRYPOINT) is PID 1 and handles signal forwarding + zombie
# reaping. Here we just launch both processes and exit if either dies so
# the container never lingers half-alive.
set -eu

# Lightpanda's CDP binds to 0.0.0.0 on the Selenoid devtools port (7070)
# so mica can publish it and relay CDP via /devtools/{session}. The
# bridge dials it back over loopback (LIGHTPANDA_HOST below).
: "${LIGHTPANDA_SERVE_HOST:=0.0.0.0}"
: "${LIGHTPANDA_HOST:=127.0.0.1}"
: "${LIGHTPANDA_PORT:=7070}"
: "${BRIDGE_PORT:=4444}"
export LIGHTPANDA_HOST LIGHTPANDA_PORT BRIDGE_PORT

echo "[entrypoint] starting lightpanda serve on ${LIGHTPANDA_SERVE_HOST}:${LIGHTPANDA_PORT} (CDP)"
lightpanda serve --host "${LIGHTPANDA_SERVE_HOST}" --port "${LIGHTPANDA_PORT}" &
LP_PID=$!

echo "[entrypoint] starting webdriver bridge on 0.0.0.0:${BRIDGE_PORT}"
node /opt/bridge/server.js &
BR_PID=$!

# Exit as soon as either child exits; tini then tears down the rest.
# `wait -n` needs a POSIX-ish shell; node:bookworm ships bash-compatible
# dash where -n is unavailable, so fall back to a simple poll.
if wait -n 2>/dev/null; then :; else
  while kill -0 "$LP_PID" 2>/dev/null && kill -0 "$BR_PID" 2>/dev/null; do
    sleep 1
  done
fi

echo "[entrypoint] a child exited; shutting down"
kill "$LP_PID" "$BR_PID" 2>/dev/null || true
exit 1
