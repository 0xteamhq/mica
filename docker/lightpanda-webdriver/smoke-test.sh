#!/bin/sh
# Smoke-test the WebDriver bridge directly (no mica in the loop) by
# exercising the same W3C endpoints mica's create + proxy flow uses.
#
#   docker run -d --rm -p 4444:4444 --name lp-smoke ghcr.io/0xteamhq/lightpanda:nightly
#   ./smoke-test.sh
#   docker rm -f lp-smoke
set -eu
BASE="${1:-http://localhost:4444}"

echo "1) status"
curl -fsS "$BASE/status"; echo

echo "2) new session"
SID=$(curl -fsS -X POST "$BASE/session" \
  -H 'content-type: application/json' \
  -d '{"capabilities":{"alwaysMatch":{"browserName":"lightpanda"}}}' \
  | sed -n 's/.*"sessionId":"\([^"]*\)".*/\1/p')
test -n "$SID" || { echo "FAIL: no sessionId"; exit 1; }
echo "   sessionId=$SID"

echo "3) navigate"
curl -fsS -X POST "$BASE/session/$SID/url" \
  -H 'content-type: application/json' \
  -d '{"url":"https://example.com"}' >/dev/null

echo "4) title"
curl -fsS "$BASE/session/$SID/title"; echo

echo "5) find h1 + read text"
EID=$(curl -fsS -X POST "$BASE/session/$SID/element" \
  -H 'content-type: application/json' \
  -d '{"using":"css selector","value":"h1"}' \
  | sed -n 's/.*"element-6066-11e4-a52e-4f735466cecf":"\([^"]*\)".*/\1/p')
echo "   elementId=$EID"
curl -fsS "$BASE/session/$SID/element/$EID/text"; echo

echo "6) executeScript"
curl -fsS -X POST "$BASE/session/$SID/execute/sync" \
  -H 'content-type: application/json' \
  -d '{"script":"return document.title;","args":[]}'; echo

echo "7) delete session"
curl -fsS -X DELETE "$BASE/session/$SID" >/dev/null
echo "OK — all steps passed"
