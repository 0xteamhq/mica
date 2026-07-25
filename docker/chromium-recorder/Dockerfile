# A mica-compatible, self-recording browser image.
#
# WebDriver (chromedriver) on :4444, Chromium rendered on an Xvfb display,
# and ffmpeg capturing that display to /video/${FILE_NAME}.mp4 when mica
# sets ENABLE_VIDEO=true. Everything here (chromium, chromium-driver,
# ffmpeg, xvfb) is packaged for arm64 and amd64 by Debian, so it runs
# natively on Apple Silicon.
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
      chromium \
      chromium-driver \
      ffmpeg \
      xvfb \
      x11-utils \
      ca-certificates \
      dumb-init \
  && rm -rf /var/lib/apt/lists/*

# chromedriver in Debian is /usr/bin/chromedriver; make sure it's on PATH.
ENV CHROME_BIN=/usr/bin/chromium

COPY entrypoint.sh /usr/local/bin/entrypoint.sh
RUN chmod +x /usr/local/bin/entrypoint.sh

EXPOSE 4444
# dumb-init as PID 1 for zombie reaping. --single-child forwards the
# stop signal ONLY to the entrypoint (not the whole process group), so
# ffmpeg receives exactly one SIGINT from the entrypoint's trap and
# finalizes the mp4 (moov atom) instead of aborting on a double signal.
ENTRYPOINT ["/usr/bin/dumb-init", "--single-child", "--", "/usr/local/bin/entrypoint.sh"]
