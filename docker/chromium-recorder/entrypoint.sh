#!/usr/bin/env bash
# mica-compatible recording browser: Xvfb + Chromium (via chromedriver) +
# optional ffmpeg screen capture. Honours mica's env contract:
#   ENABLE_VIDEO=true, FILE_NAME=<id>, VIDEO_SIZE, VIDEO_FRAME_RATE,
#   SCREEN_RESOLUTION, CODEC.
# When recording, ffmpeg grabs the X display to /video/${FILE_NAME}.mp4
# and is finalized cleanly (SIGINT) on container stop so the moov atom
# is written and the file is playable.
set -euo pipefail

SCREEN_RESOLUTION="${SCREEN_RESOLUTION:-1360x1020x24}"
VIDEO_FRAME_RATE="${VIDEO_FRAME_RATE:-15}"
DISPLAY_NUM="${DISPLAY_NUM:-99}"
export DISPLAY=":${DISPLAY_NUM}"

# Xvfb needs WxHxD; ensure a colour depth is present.
case "$SCREEN_RESOLUTION" in *x*x*) XVFB_SCREEN="$SCREEN_RESOLUTION" ;; *) XVFB_SCREEN="${SCREEN_RESOLUTION}x24" ;; esac
# ffmpeg -video_size needs WxH only (no depth). mica may send VIDEO_SIZE
# as WxH or WxHxD; fall back to the screen resolution. Keep the first
# two components either way.
GRAB_SIZE=$(printf '%s' "${VIDEO_SIZE:-$SCREEN_RESOLUTION}" | awk -F x '{print $1"x"$2}')

FFMPEG_PID=""
CD_PID=""
XVFB_PID=""

shutdown() {
  # Finalize the recording first: SIGINT tells ffmpeg to flush and write
  # the moov atom, producing a seekable/playable mp4.
  if [ -n "$FFMPEG_PID" ] && kill -0 "$FFMPEG_PID" 2>/dev/null; then
    kill -INT "$FFMPEG_PID" 2>/dev/null || true
    for _ in $(seq 1 50); do kill -0 "$FFMPEG_PID" 2>/dev/null || break; sleep 0.1; done
  fi
  [ -n "$CD_PID" ] && kill "$CD_PID" 2>/dev/null || true
  [ -n "$XVFB_PID" ] && kill "$XVFB_PID" 2>/dev/null || true
  exit 0
}
trap shutdown TERM INT

# Virtual display.
Xvfb "$DISPLAY" -screen 0 "$XVFB_SCREEN" -ac +extension RANDR -nolisten tcp >/tmp/xvfb.log 2>&1 &
XVFB_PID=$!
for _ in $(seq 1 50); do xdpyinfo -display "$DISPLAY" >/dev/null 2>&1 && break; sleep 0.1; done

# Recording (optional).
if [ "${ENABLE_VIDEO:-}" = "true" ] && [ -n "${FILE_NAME:-}" ]; then
  mkdir -p /video
  ffmpeg -nostdin -y -f x11grab -draw_mouse 0 -video_size "$GRAB_SIZE" \
    -framerate "$VIDEO_FRAME_RATE" -i "$DISPLAY" \
    -codec:v libx264 -preset veryfast -pix_fmt yuv420p -movflags +faststart \
    "/video/${FILE_NAME}.mp4" >/tmp/ffmpeg.log 2>&1 &
  FFMPEG_PID=$!
fi

# WebDriver server. chromedriver launches Chromium on $DISPLAY per session;
# allow remote connections so mica (another container/host) can reach it.
chromedriver --port=4444 --allowed-ips= --allowed-origins='*' --whitelisted-ips='' >/tmp/chromedriver.log 2>&1 &
CD_PID=$!
wait "$CD_PID"
