'use strict';
//
// WebDriver-classic → CDP bridge for Lightpanda.
//
// mica's session-create (handlers/create.rs) does `POST {upstream}/session`
// and reads `value.sessionId`, then reverse-proxies every subsequent
// `/session/{id}/...` call to this server. Lightpanda speaks CDP, not
// WebDriver, so this process presents the W3C surface mica expects and
// drives Lightpanda over CDP via puppeteer-core.
//
// Scope: the commonly-used subset of the W3C protocol — session
// new/delete, navigation, element find/click/sendKeys/text/attribute,
// executeScript, title/source/screenshot. Anything unimplemented returns
// a well-formed W3C `unknown command` error rather than crashing, so a
// client hitting an unsupported endpoint fails cleanly. Lightpanda does
// not render pixels, so /screenshot surfaces `unsupported operation`.

const express = require('express');
const puppeteer = require('puppeteer-core');

// The bridge dials Lightpanda over loopback. Lightpanda's CDP listens
// on port 7070 — Selenoid/Aerokube's devtools port convention — so
// mica's built-in `/devtools/{session}` relay can pass raw CDP through
// to the same endpoint the bridge uses.
const LIGHTPANDA_HOST = process.env.LIGHTPANDA_HOST || '127.0.0.1';
const LIGHTPANDA_PORT = process.env.LIGHTPANDA_PORT || '7070';
const BRIDGE_PORT = parseInt(process.env.BRIDGE_PORT || '4444', 10);
const BROWSER_URL = `http://${LIGHTPANDA_HOST}:${LIGHTPANDA_PORT}`;
const CONNECT_TIMEOUT_MS = parseInt(process.env.CONNECT_TIMEOUT_MS || '30000', 10);

// W3C element-reference key. Clients round-trip this exact string.
const ELEMENT_KEY = 'element-6066-11e4-a52e-4f735466cecf';

// sessionId -> { browser, context, page, elements: Map<elementId, ElementHandle> }
const sessions = new Map();
let elementSeq = 0;

// ---- W3C error plumbing -------------------------------------------------

// Each W3C error code carries a fixed HTTP status (spec §16).
const ERROR_STATUS = {
  'invalid argument': 400,
  'invalid selector': 400,
  'no such element': 404,
  'no such session': 404,
  'no such window': 404,
  'unknown command': 404,
  'stale element reference': 404,
  'javascript error': 500,
  'unsupported operation': 500,
  'session not created': 500,
  'timeout': 500,
  'unknown error': 500,
};

class WdError extends Error {
  constructor(code, message) {
    super(message || code);
    this.code = code;
  }
}

function sendError(res, err) {
  const code = err instanceof WdError ? err.code : 'unknown error';
  const status = ERROR_STATUS[code] || 500;
  res.status(status).json({
    value: {
      error: code,
      message: err.message || String(err),
      stacktrace: err.stack || '',
    },
  });
}

function ok(res, value) {
  res.status(200).json({ value: value === undefined ? null : value });
}

// Wrap an async handler so any throw becomes a W3C error response.
const h = (fn) => (req, res) => fn(req, res).catch((e) => sendError(res, e));

function getSession(req) {
  const s = sessions.get(req.params.sid);
  if (!s) throw new WdError('no such session', `no such session: ${req.params.sid}`);
  return s;
}

function getElement(sess, eid) {
  const el = sess.elements.get(eid);
  if (!el) throw new WdError('no such element', `no such element: ${eid}`);
  return el;
}

function storeElement(sess, handle) {
  const id = `node-${++elementSeq}`;
  sess.elements.set(id, handle);
  return { [ELEMENT_KEY]: id };
}

// ---- element lookup (locator strategies) --------------------------------

async function findAll(page, using, value) {
  switch (using) {
    case 'css selector':
    case 'tag name':
      return page.$$(value);
    case 'xpath':
      // puppeteer >= 22 exposes xpath via the `xpath/` query prefix.
      return page.$$(`xpath/${value}`);
    case 'link text':
    case 'partial link text': {
      const anchors = await page.$$('a');
      const out = [];
      for (const a of anchors) {
        const txt = (await page.evaluate((e) => e.textContent || '', a)).trim();
        const match = using === 'link text' ? txt === value : txt.includes(value);
        if (match) out.push(a);
        else await a.dispose();
      }
      return out;
    }
    default:
      throw new WdError('invalid argument', `unsupported locator strategy: ${using}`);
  }
}

// ---- app ----------------------------------------------------------------

const app = express();
app.use(express.json({ limit: '10mb' }));

// Liveness — handy for humans and for `docker healthcheck`. mica itself
// only needs the TCP port open, which Express provides once listening.
app.get('/status', (req, res) =>
  ok(res, { ready: true, message: 'lightpanda webdriver bridge up' }),
);

// New session: connect to Lightpanda over CDP, open a page.
app.post('/session', h(async (req, res) => {
  let browser;
  try {
    browser = await puppeteer.connect({
      browserURL: BROWSER_URL,
      protocolTimeout: CONNECT_TIMEOUT_MS,
    });
  } catch (e) {
    throw new WdError('session not created', `cannot reach lightpanda at ${BROWSER_URL}: ${e.message}`);
  }
  // Lightpanda's documented puppeteer flow: create a fresh browser
  // context, then a page inside it. Grabbing a pre-existing target
  // (browser.pages()[0]) yields a page with no proper execution
  // context under Lightpanda, so goto/title/evaluate hang. Fall back
  // to a plain newPage() if contexts aren't supported.
  let context = null;
  let page;
  try {
    context = await browser.createBrowserContext();
    page = await context.newPage();
  } catch (_) {
    page = await browser.newPage();
  }
  const sid = `lp-${Date.now()}-${Math.floor(Math.random() * 1e6)}`;
  sessions.set(sid, { browser, context, page, elements: new Map() });

  ok(res, {
    sessionId: sid,
    capabilities: {
      browserName: 'lightpanda',
      browserVersion: 'nightly',
      platformName: 'linux',
      'lightpanda:cdpUrl': BROWSER_URL,
      // Advertise that this is a headless, non-rendering engine so
      // clients don't expect screenshots to work.
      'lightpanda:rendering': false,
    },
  });
}));

// Delete session: close the page and drop the CDP connection.
app.delete('/session/:sid', h(async (req, res) => {
  const sess = sessions.get(req.params.sid);
  if (sess) {
    sessions.delete(req.params.sid);
    try { await sess.page.close(); } catch (_) { /* best effort */ }
    if (sess.context) { try { await sess.context.close(); } catch (_) { /* best effort */ } }
    try { sess.browser.disconnect(); } catch (_) { /* best effort */ }
  }
  ok(res, null);
}));

// Navigate.
app.post('/session/:sid/url', h(async (req, res) => {
  const sess = getSession(req);
  const url = req.body && req.body.url;
  if (typeof url !== 'string' || !url) throw new WdError('invalid argument', 'missing "url"');
  await sess.page.goto(url, { waitUntil: 'load' });
  ok(res, null);
}));

app.get('/session/:sid/url', h(async (req, res) => ok(res, getSession(req).page.url())));

app.get('/session/:sid/title', h(async (req, res) => ok(res, await getSession(req).page.title())));

// Full serialized DOM.
app.get('/session/:sid/source', h(async (req, res) => ok(res, await getSession(req).page.content())));

// Find a single / multiple elements.
app.post('/session/:sid/element', h(async (req, res) => {
  const sess = getSession(req);
  const { using, value } = req.body || {};
  const matches = await findAll(sess.page, using, value);
  if (!matches.length) throw new WdError('no such element', `no element for ${using}=${value}`);
  ok(res, storeElement(sess, matches[0]));
}));

app.post('/session/:sid/elements', h(async (req, res) => {
  const sess = getSession(req);
  const { using, value } = req.body || {};
  const matches = await findAll(sess.page, using, value);
  ok(res, matches.map((m) => storeElement(sess, m)));
}));

// Element interactions.
app.post('/session/:sid/element/:eid/click', h(async (req, res) => {
  const sess = getSession(req);
  await getElement(sess, req.params.eid).click();
  ok(res, null);
}));

app.post('/session/:sid/element/:eid/value', h(async (req, res) => {
  const sess = getSession(req);
  const el = getElement(sess, req.params.eid);
  // W3C sends `text`; older clients send `value` as a char array.
  const text = req.body && (req.body.text != null
    ? req.body.text
    : Array.isArray(req.body.value) ? req.body.value.join('') : '');
  await el.type(String(text));
  ok(res, null);
}));

app.post('/session/:sid/element/:eid/clear', h(async (req, res) => {
  const sess = getSession(req);
  const el = getElement(sess, req.params.eid);
  await el.evaluate((node) => {
    if ('value' in node) node.value = '';
    else node.textContent = '';
  });
  ok(res, null);
}));

app.get('/session/:sid/element/:eid/text', h(async (req, res) => {
  const sess = getSession(req);
  const el = getElement(sess, req.params.eid);
  const text = await el.evaluate((n) => (n.innerText != null ? n.innerText : n.textContent) || '');
  ok(res, text.trim());
}));

app.get('/session/:sid/element/:eid/attribute/:name', h(async (req, res) => {
  const sess = getSession(req);
  const el = getElement(sess, req.params.eid);
  const name = req.params.name;
  const val = await el.evaluate((n, a) => n.getAttribute(a), name);
  ok(res, val);
}));

app.get('/session/:sid/element/:eid/property/:name', h(async (req, res) => {
  const sess = getSession(req);
  const el = getElement(sess, req.params.eid);
  const name = req.params.name;
  const val = await el.evaluate((n, p) => {
    const v = n[p];
    return v == null ? null : String(v);
  }, name);
  ok(res, val);
}));

// executeScript — body runs in page context with `arguments` bound.
// Only JSON-serializable args are supported (no element handle args).
async function execScript(req, res) {
  const sess = getSession(req);
  const script = (req.body && req.body.script) || '';
  const args = (req.body && req.body.args) || [];
  try {
    const result = await sess.page.evaluate(
      ({ script, args }) => {
        // eslint-disable-next-line no-new-func
        const fn = new Function(`${script}\n//# sourceURL=__webdriver_exec__`);
        return fn.apply(window, args);
      },
      { script, args },
    );
    ok(res, result === undefined ? null : result);
  } catch (e) {
    throw new WdError('javascript error', e.message);
  }
}
app.post('/session/:sid/execute/sync', h(execScript));
app.post('/session/:sid/execute/async', h(execScript));

// Screenshot — Lightpanda is a non-rendering engine, so this is
// deliberately unsupported rather than silently wrong.
app.get('/session/:sid/screenshot', h(async (req, res) => {
  getSession(req);
  try {
    const buf = await getSession(req).page.screenshot({ encoding: 'base64' });
    ok(res, buf);
  } catch (e) {
    throw new WdError('unsupported operation', `lightpanda does not render pixels: ${e.message}`);
  }
}));

// Anything we don't implement: well-formed W3C `unknown command`.
app.use((req, res) => sendError(res, new WdError('unknown command', `${req.method} ${req.path}`)));

const server = app.listen(BRIDGE_PORT, '0.0.0.0', () => {
  console.log(`[bridge] webdriver bridge listening on 0.0.0.0:${BRIDGE_PORT} -> lightpanda ${BROWSER_URL}`);
});

// Graceful teardown when mica's DockerStopper sends SIGTERM.
for (const sig of ['SIGTERM', 'SIGINT']) {
  process.on(sig, () => {
    console.log(`[bridge] ${sig} received, closing sessions`);
    for (const sess of sessions.values()) {
      try { sess.browser.disconnect(); } catch (_) { /* ignore */ }
    }
    server.close(() => process.exit(0));
    setTimeout(() => process.exit(0), 2000).unref();
  });
}
