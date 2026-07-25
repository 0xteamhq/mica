#!/usr/bin/env bash
#
# run-sessions.sh — drive one or more recording sessions against a mica
# instance, for demos and validating the recording / dashboard flow.
#
# Each session: creates a WebDriver session with `enableVideo`, navigates
# to a Wikipedia page (a scientist/engineer profile, cycled from a list),
# scrolls a little for visual motion, then stops — which makes mica
# finalize the recording, rename it to `{session_id}.mp4`, list it on the
# Recordings page, and (if configured) upload it to S3/MinIO.
#
# Usage:
#   scripts/run-sessions.sh [COUNT]
#
# Environment overrides:
#   MICA_URL      base URL of the mica node        (default http://localhost:4444)
#   BROWSER       browserName to request           (default chrome)
#   CONCURRENCY   sessions to run in parallel       (default 4)
#   RESOLUTION    recording resolution WxH          (default 1280x800)
#   CHROME_BIN    chromium binary inside the image  (default /usr/bin/chromium)
#
# Requires: curl, python3. The mica node must have a recording-capable
# browser configured (see docker/chromium-recorder) and be started with
# --video-output-dir. Example node:
#   mica --conf browsers.json --video-output-dir ./video --timeout 15m
#
# Examples:
#   scripts/run-sessions.sh                 # one session
#   scripts/run-sessions.sh 20              # twenty, 4 at a time
#   MICA_URL=http://localhost:4455 scripts/run-sessions.sh 5
set -euo pipefail

COUNT="${1:-1}"
MICA_URL="${MICA_URL:-http://localhost:4444}"
BROWSER="${BROWSER:-chrome}"
CONCURRENCY="${CONCURRENCY:-4}"
RESOLUTION="${RESOLUTION:-1280x800}"
CHROME_BIN="${CHROME_BIN:-/usr/bin/chromium}"

for bin in curl python3; do
  command -v "$bin" >/dev/null || { echo "error: '$bin' is required" >&2; exit 1; }
done

# Wikipedia profiles cycled through, one per session.
PAGES=(
  Alan_Turing Ada_Lovelace Grace_Hopper Marie_Curie Nikola_Tesla
  Albert_Einstein Isaac_Newton Charles_Darwin Rosalind_Franklin
  Katherine_Johnson Linus_Torvalds Dennis_Ritchie Ken_Thompson
  Tim_Berners-Lee Donald_Knuth Barbara_Liskov John_von_Neumann
  Claude_Shannon Vint_Cerf "Margaret_Hamilton_(software_engineer)"
)

# JSON body for a recording session. `mica:options.enableVideo` tells
# mica to record; the goog:chromeOptions keep chromedriver happy.
session_body() {
  python3 - "$BROWSER" "$RESOLUTION" "$CHROME_BIN" <<'PY'
import json, sys
browser, res, chrome_bin = sys.argv[1], sys.argv[2], sys.argv[3]
print(json.dumps({"capabilities": {"alwaysMatch": {
    "browserName": browser,
    "mica:options": {"enableVideo": True, "screenResolution": res},
    "goog:chromeOptions": {"binary": chrome_bin, "args": [
        "--no-sandbox", "--disable-dev-shm-usage", "--disable-gpu",
        f"--window-size={res.replace('x', ',')}"]},
}}}))
PY
}
BODY="$(session_body)"

# Run one session against $1 (a Wikipedia page slug). Prints OK/FAIL.
run_one() {
  local slug="$1"
  local sid
  sid=$(curl -s -m 90 -X POST "$MICA_URL/wd/hub/session" \
          -H 'content-type: application/json' -d "$BODY" \
        | python3 -c "import sys,json;print(json.load(sys.stdin).get('value',{}).get('sessionId',''))" 2>/dev/null || true)
  if [ -z "$sid" ]; then echo "FAIL $slug (no session created)"; return 1; fi

  nav() { curl -s -m 30 -X POST "$MICA_URL/wd/hub/session/$sid/url" \
            -H 'content-type: application/json' -d "{\"url\":\"$1\"}" >/dev/null 2>&1 || true; }

  nav "https://en.wikipedia.org/wiki/Main_Page"; sleep 1
  nav "https://en.wikipedia.org/wiki/$slug"; sleep 2
  curl -s -m 20 -X POST "$MICA_URL/wd/hub/session/$sid/execute/sync" \
    -H 'content-type: application/json' \
    -d '{"script":"window.scrollBy(0,500);return document.title;","args":[]}' >/dev/null 2>&1 || true
  sleep 1
  local title
  title=$(curl -s -m 10 "$MICA_URL/wd/hub/session/$sid/title" \
          | python3 -c "import sys,json;print(json.load(sys.stdin).get('value',''))" 2>/dev/null || true)
  curl -s -X DELETE "$MICA_URL/wd/hub/session/$sid" -o /dev/null 2>&1 || true
  echo "OK ${slug} -> ${title:-<no title>} (${sid})"
}
export -f run_one nav 2>/dev/null || true
export MICA_URL BODY

echo "Running $COUNT session(s) against $MICA_URL (browser=$BROWSER, concurrency=$CONCURRENCY)…"

# Emit COUNT page slugs (cycling the list) and run them with bounded
# concurrency via xargs -P.
for i in $(seq 0 $((COUNT - 1))); do
  echo "${PAGES[$((i % ${#PAGES[@]}))]}"
done | xargs -P "$CONCURRENCY" -I {} bash -c 'run_one "$@"' _ {}

echo "Done. View recordings at ${MICA_URL}/admin/recordings"
