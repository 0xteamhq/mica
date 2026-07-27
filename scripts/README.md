# scripts

Helper scripts for demos and validating mica end to end.

## `run_sessions.py` — drive recording sessions

Runs one or more real browser sessions against a mica node using
**Selenium** (the native client for a W3C WebDriver grid, which is what
mica is). Each session records video (`enableVideo`), browses a couple of
Wikipedia pages, then quits — exercising the full pipeline: session →
record → finalize → rename to `{session_id}.mp4` → list on the Recordings
page → upload to S3/MinIO (if configured).

### Requirements

```bash
pip install -r scripts/requirements.txt
```

`scripts/requirements.txt` is the single source of truth for the Selenium
version floor.

A mica node with a **recording-capable browser** configured (see
`docker/chromium-recorder`) and started with `--video-output-dir`, e.g.:

```bash
mica --conf browsers.json --video-output-dir "$PWD/video" --timeout 15m
```

`--video-output-dir` must be an **absolute** path. mica reuses the value
verbatim as the host side of the recorder container's bind mount, and Docker
does not resolve client-relative sources: `./video` is rejected outright,
while a bare relative path — including mica's own default, `video` — is
interpreted as a *named volume*, so recordings are written somewhere mica
never reads and vanish with no error. If mica itself runs in a container, the
same path must also be mounted identically inside and outside it.

### Usage

```bash
scripts/run_sessions.py                       # one session
scripts/run_sessions.py 20                     # twenty, 4 in parallel
scripts/run_sessions.py 5  --url http://localhost:4455
scripts/run_sessions.py 10 --concurrency 2
```

Options: `--url`, `--browser`, `--concurrency`, `--resolution`,
`--chrome-bin`, `--timeout` (`--help` for details). Results stream as each
session finishes; watch them appear live at `<url>/admin/recordings`.

`--browser` is the `browserName` requested, and must match a key in the
node's `browsers.json` (default `chrome`; use e.g. `--browser chromium` if
the recorder is registered under that name).

A session counts as `OK` only if its teardown also succeeds — the session
delete is what makes mica finalize and upload the recording, so a failed
`quit()` is reported as a failure rather than a phantom success.

`--timeout` (default 90s) bounds every HTTP request to the node, including
the session create — which blocks while mica queues for a free slot. Raise
it when you deliberately oversubscribe the grid (`--concurrency` above the
node's `--limit`), since queued creates then wait longer.

A timeout that fires mid-create does not leak anything on the node: mica
cancels on client disconnect, so a still-queued create never takes a permit,
and a container started just before the disconnect is torn down by its
stopper guard. The session simply counts as a failure here.

### Why Selenium and not Playwright?

mica speaks **W3C WebDriver** (`/wd/hub/session`). Selenium is a
WebDriver client, so it connects directly and passes mica's
`mica:options` capabilities (like `enableVideo`) natively. Playwright
drives browsers over its own protocol/CDP and does not natively connect
to a WebDriver grid, so it isn't a drop-in client here.
