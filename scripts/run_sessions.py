#!/usr/bin/env python3
"""Drive recording sessions against a mica node — for demos and for
validating the recording / dashboard flow end to end.

Each session creates a WebDriver session with ``enableVideo``, navigates
to a Wikipedia profile (cycled from a list), scrolls a little for visual
motion, then stops. Stopping makes mica finalize the recording, rename it
to ``{session_id}.mp4``, list it on the Recordings page, and (if
configured) upload it to S3/MinIO.

Requirements: Python 3.8+ (standard library only — no pip installs).
The mica node must have a recording-capable browser configured (see
``docker/chromium-recorder``) and be started with ``--video-output-dir``.

Examples::

    scripts/run_sessions.py                       # one session
    scripts/run_sessions.py 20                     # twenty, 4 in parallel
    scripts/run_sessions.py 5 --url http://localhost:4455
    scripts/run_sessions.py 10 --concurrency 2 --browser chrome
"""
from __future__ import annotations

import argparse
import concurrent.futures
import json
import sys
import time
import urllib.error
import urllib.request

# Wikipedia profiles cycled through, one per session.
PAGES = [
    "Alan_Turing", "Ada_Lovelace", "Grace_Hopper", "Marie_Curie",
    "Nikola_Tesla", "Albert_Einstein", "Isaac_Newton", "Charles_Darwin",
    "Rosalind_Franklin", "Katherine_Johnson", "Linus_Torvalds",
    "Dennis_Ritchie", "Ken_Thompson", "Tim_Berners-Lee", "Donald_Knuth",
    "Barbara_Liskov", "John_von_Neumann", "Claude_Shannon", "Vint_Cerf",
    "Margaret_Hamilton_(software_engineer)",
]


def _request(method: str, url: str, body: dict | None = None, timeout: float = 30.0):
    """Minimal JSON HTTP helper over urllib. Returns the parsed response
    (or ``None`` on any error/non-JSON body)."""
    data = json.dumps(body).encode() if body is not None else None
    headers = {"content-type": "application/json"} if data else {}
    req = urllib.request.Request(url, data=data, method=method, headers=headers)
    try:
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            raw = resp.read()
    except (urllib.error.URLError, TimeoutError, OSError):
        return None
    try:
        return json.loads(raw)
    except (ValueError, TypeError):
        return None


def session_body(browser: str, resolution: str, chrome_bin: str) -> dict:
    """W3C create body. ``mica:options.enableVideo`` tells mica to record;
    the ``goog:chromeOptions`` keep chromedriver happy."""
    return {"capabilities": {"alwaysMatch": {
        "browserName": browser,
        "mica:options": {"enableVideo": True, "screenResolution": resolution},
        "goog:chromeOptions": {
            "binary": chrome_bin,
            "args": [
                "--no-sandbox", "--disable-dev-shm-usage", "--disable-gpu",
                "--window-size=" + resolution.replace("x", ","),
            ],
        },
    }}}


def run_one(page: str, args: argparse.Namespace) -> tuple[bool, str]:
    """Run a single recording session against ``page``. Returns
    ``(ok, message)``."""
    base = args.url.rstrip("/")
    body = session_body(args.browser, args.resolution, args.chrome_bin)
    created = _request("POST", f"{base}/wd/hub/session", body, timeout=args.create_timeout)
    sid = (created or {}).get("value", {}).get("sessionId") if created else None
    if not sid:
        return False, f"FAIL {page} (no session created)"

    def nav(url: str) -> None:
        _request("POST", f"{base}/wd/hub/session/{sid}/url", {"url": url})

    try:
        nav("https://en.wikipedia.org/wiki/Main_Page")
        time.sleep(1)
        nav(f"https://en.wikipedia.org/wiki/{page}")
        time.sleep(2)
        _request("POST", f"{base}/wd/hub/session/{sid}/execute/sync",
                 {"script": "window.scrollBy(0,500);return document.title;", "args": []})
        time.sleep(1)
        title_resp = _request("GET", f"{base}/wd/hub/session/{sid}/title", timeout=10)
        title = (title_resp or {}).get("value", "") if title_resp else ""
    finally:
        # Stop via the admin API so the artifact is finalized + uploaded.
        _request("DELETE", f"{base}/admin/api/sessions/{sid}")
    return True, f"OK   {page} -> {title or '<no title>'} ({sid})"


def main() -> int:
    p = argparse.ArgumentParser(
        description="Drive recording sessions against a mica node.")
    p.add_argument("count", nargs="?", type=int, default=1,
                   help="number of sessions to run (default: 1)")
    p.add_argument("--url", default="http://localhost:4444",
                   help="mica base URL (default: http://localhost:4444)")
    p.add_argument("--browser", default="chrome",
                   help="browserName to request (default: chrome)")
    p.add_argument("--concurrency", type=int, default=4,
                   help="sessions to run in parallel (default: 4)")
    p.add_argument("--resolution", default="1280x800",
                   help="recording resolution WxH (default: 1280x800)")
    p.add_argument("--chrome-bin", default="/usr/bin/chromium",
                   help="chromium binary inside the image (default: /usr/bin/chromium)")
    p.add_argument("--create-timeout", type=float, default=90.0,
                   help="seconds to wait for session create (default: 90)")
    args = p.parse_args()

    if args.count < 1:
        p.error("count must be >= 1")

    pages = [PAGES[i % len(PAGES)] for i in range(args.count)]
    print(f"Running {args.count} session(s) against {args.url} "
          f"(browser={args.browser}, concurrency={args.concurrency})…", flush=True)

    ok = 0
    with concurrent.futures.ThreadPoolExecutor(max_workers=args.concurrency) as pool:
        futures = [pool.submit(run_one, pg, args) for pg in pages]
        for fut in concurrent.futures.as_completed(futures):
            success, msg = fut.result()
            ok += success
            print(msg, flush=True)

    fail = args.count - ok
    print(f"\nDone: {ok} ok, {fail} failed. "
          f"View recordings at {args.url.rstrip('/')}/admin/recordings")
    return 0 if fail == 0 else 1


if __name__ == "__main__":
    sys.exit(main())
