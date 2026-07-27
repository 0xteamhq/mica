#!/usr/bin/env python3
"""Drive recording sessions against a mica node — for demos and for
validating the recording / dashboard flow end to end.

Uses **Selenium** (the idiomatic client for a W3C WebDriver grid, which
is what mica is). Each session connects a Remote WebDriver to mica with
``enableVideo`` set, navigates to a Wikipedia profile (cycled from a
list), scrolls a little for visual motion, then quits — which makes mica
finalize the recording, rename it to ``{session_id}.mp4``, list it on the
Recordings page, and (if configured) upload it to S3/MinIO.

Note: mica speaks W3C WebDriver, so Selenium is the natural client.
Playwright drives browsers over its own protocol/CDP and does not
natively connect to a WebDriver grid, which is why this uses Selenium.

Requirements (version floor lives in ``scripts/requirements.txt``)::

    pip install -r scripts/requirements.txt

The mica node must have a recording-capable browser configured (see
``docker/chromium-recorder``) and be started with ``--video-output-dir``.

Examples::

    scripts/run_sessions.py                       # one session
    scripts/run_sessions.py 20                     # twenty, 4 in parallel
    scripts/run_sessions.py 5 --url http://localhost:4455
    scripts/run_sessions.py 10 --concurrency 2
"""
from __future__ import annotations

import argparse
import concurrent.futures
import sys
import time

try:
    from selenium import webdriver
    from selenium.webdriver.chrome.options import Options
    from selenium.webdriver.remote.client_config import ClientConfig
except ImportError as e:
    # client_config.py first ships in 4.26.0, so a too-old 4.x lands here —
    # but so does any other broken/partial selenium install. Keep the real
    # cause visible instead of always blaming the version.
    sys.exit(f"cannot import selenium ({e}).\n"
             f"Install it with: pip install -r scripts/requirements.txt")

# Wikipedia profiles cycled through, one per session.
PAGES = [
    "Alan_Turing", "Ada_Lovelace", "Grace_Hopper", "Marie_Curie",
    "Nikola_Tesla", "Albert_Einstein", "Isaac_Newton", "Charles_Darwin",
    "Rosalind_Franklin", "Katherine_Johnson", "Linus_Torvalds",
    "Dennis_Ritchie", "Ken_Thompson", "Tim_Berners-Lee", "Donald_Knuth",
    "Barbara_Liskov", "John_von_Neumann", "Claude_Shannon", "Vint_Cerf",
    "Margaret_Hamilton_(software_engineer)",
]


def build_options(args: argparse.Namespace) -> Options:
    """Chrome options for the recorder image, with mica's recording
    capability attached under the ``mica:options`` namespace."""
    opts = Options()
    # ChromeOptions defaults browserName to "chrome"; override it so a
    # recorder registered in browsers.json under another key (e.g.
    # "chromium") is still reachable. goog:chromeOptions is kept either way.
    opts.set_capability("browserName", args.browser)
    opts.binary_location = args.chrome_bin
    for a in ("--no-sandbox", "--disable-dev-shm-usage", "--disable-gpu",
              "--window-size=" + args.resolution.replace("x", ",")):
        opts.add_argument(a)
    # mica reads these to start display-recording the session.
    opts.set_capability("mica:options", {
        "enableVideo": True,
        "screenResolution": args.resolution,
    })
    return opts


def _first_line(e: BaseException) -> str:
    """First line of an exception's message, or a ``"<no message>"``
    placeholder when it carries none (a bare ``TimeoutError()`` stringifies
    to ``""``, whose ``splitlines()`` is empty). Callers pair this with
    ``type(e).__name__`` so the class is always shown."""
    lines = str(e).strip().splitlines()
    return lines[0] if lines else "<no message>"


def run_one(page: str, args: argparse.Namespace) -> tuple[bool, str]:
    """Run a single recording session against ``page`` via Selenium.
    Returns ``(ok, message)``. ``driver.quit()`` performs the standard
    WebDriver session delete, which triggers mica's teardown + recording
    finalize."""
    hub = args.url.rstrip("/") + "/wd/hub"
    # Bound every HTTP request — above all the session create, which
    # blocks while mica queues for a free slot. Without this, Selenium
    # inherits the (unset) global socket timeout and a worker thread can
    # hang forever against a saturated or wedged node.
    driver = None
    sid = "?"
    try:
        # Inside the try: on a selenium that ships client_config.py but
        # predates the 4.26 signature, this raises TypeError — report it as
        # a failed session instead of aborting the whole run.
        client_config = ClientConfig(remote_server_addr=hub, timeout=args.timeout)
        driver = webdriver.Remote(command_executor=hub, options=build_options(args),
                                  client_config=client_config)
        sid = driver.session_id
        # Keep the server-side page-load budget strictly under the HTTP
        # timeout, so a slow navigation comes back as a clean WebDriver
        # timeout instead of urllib3 aborting the request first. min(), not
        # max(): a floor would invert the ordering for small --timeout.
        driver.set_page_load_timeout(min(30.0, args.timeout * 0.5))
        driver.get("https://en.wikipedia.org/wiki/Main_Page")
        time.sleep(1)
        driver.get(f"https://en.wikipedia.org/wiki/{page}")
        time.sleep(2)
        driver.execute_script("window.scrollBy(0, 500);")
        time.sleep(1)
        title = driver.title
    except Exception as e:  # noqa: BLE001 — report any failure, keep going
        if driver is not None:
            try:
                driver.quit()  # best-effort; we are already failing
            except Exception:  # noqa: BLE001
                pass
        return False, f"FAIL {page} ({type(e).__name__}: {_first_line(e)}) ({sid})"

    # Teardown is not cleanup here — it is the behavior under test. The
    # session delete is what makes mica finalize the recording, rename it,
    # and upload it, so a failed quit means no artifact was produced. Report
    # it as a failed session rather than swallowing it after an "OK".
    try:
        driver.quit()
    except Exception as e:  # noqa: BLE001
        return False, f"FAIL {page} (teardown {type(e).__name__}: {_first_line(e)}) ({sid})"
    return True, f"OK   {page} -> {title or '<no title>'} ({sid})"


def main() -> int:
    p = argparse.ArgumentParser(
        description="Drive recording sessions against a mica node (Selenium).")
    p.add_argument("count", nargs="?", type=int, default=1,
                   help="number of sessions to run (default: 1)")
    p.add_argument("--url", default="http://localhost:4444",
                   help="mica base URL (default: http://localhost:4444)")
    p.add_argument("--browser", default="chrome",
                   help="browserName to request, as keyed in browsers.json "
                        "(default: chrome)")
    p.add_argument("--concurrency", type=int, default=4,
                   help="sessions to run in parallel (default: 4)")
    p.add_argument("--resolution", default="1280x800",
                   help="recording resolution WxH (default: 1280x800)")
    p.add_argument("--chrome-bin", default="/usr/bin/chromium",
                   help="chromium binary inside the image (default: /usr/bin/chromium)")
    p.add_argument("--timeout", type=float, default=90.0,
                   help="per-request HTTP timeout in seconds, including the "
                        "queued session create (default: 90)")
    args = p.parse_args()

    if args.count < 1:
        p.error("count must be >= 1")

    pages = [PAGES[i % len(PAGES)] for i in range(args.count)]
    print(f"Running {args.count} session(s) against {args.url} "
          f"(browser={args.browser}, concurrency={args.concurrency})…",
          flush=True)

    ok = 0
    with concurrent.futures.ThreadPoolExecutor(max_workers=args.concurrency) as pool:
        futures = [pool.submit(run_one, pg, args) for pg in pages]
        # as_completed, not pool.map: map yields in submission order, so one
        # slow session withholds every later result line. This is a live
        # progress feed — print each session the moment it finishes.
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
