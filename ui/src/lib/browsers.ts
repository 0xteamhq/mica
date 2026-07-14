// Suggested browser catalog + safe merge helper for the browsers.json
// editor. Lets the dashboard insert a structurally-correct version
// entry (matching src/config/browser.rs) instead of hand-editing JSON.

export type VersionStatus = "available" | "coming-soon";

export interface CatalogVersion {
  version: string;
  /** `coming-soon` versions are shown but not yet insertable. */
  status: VersionStatus;
}

export interface BrowserTemplate {
  /** Display name in the picker. */
  label: string;
  /** Top-level key in browsers.json. */
  key: string;
  /** Docker image with `{version}` substituted at insert time. */
  image: string;
  /** Container port mica proxies to (Browser::port default is 4444). */
  port: string;
  /** WebDriver base path — Chromium-family serve `/`, Firefox `/wd/hub`. */
  path: string;
  /** Static, hardcoded version list for THIS browser, shown in the
   *  picker. Mark a not-yet-published image `coming-soon`. */
  versions: CatalogVersion[];
}

/** Aerokube/Selenoid image conventions — the images mica pulls by default. */
export const BROWSER_CATALOG: BrowserTemplate[] = [
  {
    label: "Chrome",
    key: "chrome",
    image: "selenoid/chrome:{version}",
    port: "4444",
    path: "/",
    versions: [
      { version: "128.0", status: "coming-soon" },
      { version: "127.0", status: "available" },
      { version: "126.0", status: "available" },
      { version: "125.0", status: "available" },
      { version: "124.0", status: "available" },
    ],
  },
  {
    label: "Firefox",
    key: "firefox",
    image: "selenoid/firefox:{version}",
    port: "4444",
    path: "/wd/hub",
    versions: [
      { version: "129.0", status: "coming-soon" },
      { version: "128.0", status: "available" },
      { version: "127.0", status: "available" },
      { version: "126.0", status: "available" },
    ],
  },
  {
    label: "Opera",
    key: "opera",
    image: "selenoid/opera:{version}",
    port: "4444",
    path: "/",
    versions: [
      { version: "112.0", status: "coming-soon" },
      { version: "111.0", status: "available" },
      { version: "110.0", status: "available" },
    ],
  },
  {
    label: "Edge",
    key: "edge",
    image: "browsers/edge:{version}",
    port: "4444",
    path: "/",
    versions: [
      { version: "128.0", status: "coming-soon" },
      { version: "127.0", status: "available" },
      { version: "126.0", status: "available" },
    ],
  },
];

/** Brand accent per browser for the registry cards (falls back to muted). */
export const BROWSER_ACCENT: Record<string, string> = {
  chrome: "#4285F4",
  firefox: "#FF7139",
  opera: "#FF1B2D",
  edge: "#22A0E0",
  safari: "#1B88CA",
};

/** The version entry a template + version expands to. */
export function buildVersionEntry(t: BrowserTemplate, version: string) {
  return { image: t.image.replace("{version}", version), port: t.port, path: t.path };
}

/** First selectable (available) version for a browser — the picker's default. */
export function firstAvailableVersion(t: BrowserTemplate): string {
  return t.versions.find((v) => v.status === "available")?.version ?? "";
}

/**
 * Merge a new `version` for template `t` into a browsers.json document
 * and return the re-serialized (pretty, 2-space) text. Creates the
 * browser key if absent and seeds `default` on the first version.
 * Throws on malformed JSON or a non-object root so the caller can
 * surface the error instead of corrupting the file.
 */
export function addVersion(jsonText: string, t: BrowserTemplate, version: string): string {
  const doc: unknown = jsonText.trim() ? JSON.parse(jsonText) : {};
  if (typeof doc !== "object" || doc === null || Array.isArray(doc)) {
    throw new Error("browsers.json must be a JSON object");
  }
  const root = doc as Record<string, { default?: string; versions?: Record<string, unknown> }>;
  const entry = root[t.key] ?? {};
  const versions = entry.versions ?? {};
  versions[version] = buildVersionEntry(t, version);
  root[t.key] = {
    default: entry.default ?? version,
    versions,
  };
  return JSON.stringify(root, null, 2) + "\n";
}

/** Set `version` as the `default` for `key` (no-op if it doesn't exist). */
export function setDefaultVersion(text: string, key: string, version: string): string {
  const root = parseRoot(text);
  const entry = root[key];
  if (entry?.versions && version in entry.versions) {
    entry.default = version;
  }
  return serialize(root);
}

/**
 * Remove `version` from `key`. If it was the last version the browser
 * is dropped entirely; if it was the default, the default falls back to
 * the lowest remaining version.
 */
export function removeVersion(text: string, key: string, version: string): string {
  const root = parseRoot(text);
  const entry = root[key];
  if (!entry?.versions) return serialize(root);
  delete entry.versions[version];
  const remaining = Object.keys(entry.versions);
  if (remaining.length === 0) {
    delete root[key];
  } else if (entry.default === version) {
    entry.default = remaining.sort()[0];
  }
  return serialize(root);
}

type Root = Record<string, { default?: string; versions?: Record<string, unknown> }>;

function parseRoot(text: string): Root {
  const doc: unknown = text.trim() ? JSON.parse(text) : {};
  if (typeof doc !== "object" || doc === null || Array.isArray(doc)) {
    throw new Error("browsers.json must be a JSON object");
  }
  return doc as Root;
}

function serialize(root: Root): string {
  return JSON.stringify(root, null, 2) + "\n";
}

/**
 * Parse browsers.json text into the `{default, versions[]}` shape the
 * registry table renders. Throws on malformed JSON so form mode can
 * prompt the user to switch to JSON mode and fix it.
 */
export function parseBrowsers(
  text: string,
): Record<string, { default: string; versions: string[] }> {
  const doc: unknown = text.trim() ? JSON.parse(text) : {};
  if (typeof doc !== "object" || doc === null || Array.isArray(doc)) {
    throw new Error("browsers.json must be a JSON object");
  }
  const root = doc as Record<string, { default?: string; versions?: Record<string, unknown> }>;
  const out: Record<string, { default: string; versions: string[] }> = {};
  for (const key of Object.keys(root)) {
    out[key] = {
      default: root[key].default ?? "",
      versions: Object.keys(root[key].versions ?? {}).sort(),
    };
  }
  return out;
}
