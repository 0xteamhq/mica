// Client for mica's /admin/api surface. The wire shapes mirror
// src/handlers/admin/state.rs and events.rs.

export interface Capacity {
  total: number;
  used: number;
  queued: number;
  pending: number;
}

export interface BrowserInfo {
  default: string;
  versions: string[];
}

export interface SessionInfo {
  id: string;
  browser: string;
  version: string;
  started: string;
  owner: string | null;
  vnc: boolean;
  devtools: boolean;
  logs: boolean;
}

export interface StateResponse {
  capacity: Capacity;
  draining: boolean;
  browsers: Record<string, BrowserInfo>;
  sessions: SessionInfo[];
}

export interface Stats extends Capacity {
  sessions: number;
  draining: boolean;
}

export async function fetchState(): Promise<StateResponse> {
  const res = await fetch("/admin/api/state");
  if (!res.ok) throw new Error(`state: HTTP ${res.status}`);
  return res.json();
}

export interface LiveHandlers {
  onStats: (s: Stats) => void;
  onSessionCreated: (e: { id: string; browser: string; version: string; owner: string | null }) => void;
  onSessionStopped: (e: { id: string }) => void;
  onReset: () => void;
  onDrain: (e: { active: boolean }) => void;
  onConfigReloaded: () => void;
}

/** Subscribe to /admin/api/events. Returns a cleanup function. */
export function subscribe(handlers: LiveHandlers): () => void {
  const es = new EventSource("/admin/api/events");
  const on = <T>(name: string, fn: (data: T) => void) =>
    es.addEventListener(name, (ev) => fn(JSON.parse((ev as MessageEvent).data)));

  on<Stats>("stats", handlers.onStats);
  on("session_created", handlers.onSessionCreated);
  on("session_stopped", handlers.onSessionStopped);
  on<{ active: boolean }>("drain", handlers.onDrain);
  es.addEventListener("reset", handlers.onReset);
  es.addEventListener("config_reloaded", () => {
    handlers.onConfigReloaded();
  });
  // EventSource reconnects on its own; a reconnect may have missed
  // events, so treat open-after-error like a reset.
  let hadError = false;
  es.onerror = () => {
    hadError = true;
  };
  es.onopen = () => {
    if (hadError) {
      hadError = false;
      handlers.onReset();
    }
  };
  return () => es.close();
}

async function expect(res: Response, what: string): Promise<Response> {
  if (res.status === 403) throw new Error(`${what}: admin role required`);
  if (!res.ok) throw new Error(`${what}: HTTP ${res.status}`);
  return res;
}

export async function killSession(id: string): Promise<void> {
  await expect(await fetch(`/admin/api/sessions/${id}`, { method: "DELETE" }), "kill");
}

export async function setDrain(active: boolean): Promise<void> {
  await expect(
    await fetch("/admin/api/drain", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ active }),
    }),
    "drain",
  );
}

export async function reloadConfig(): Promise<{ browsers: number; users: number }> {
  const res = await fetch("/admin/api/config/reload", { method: "POST" });
  if (res.status === 400) {
    const body = await res.json().catch(() => ({ error: "invalid config" }));
    throw new Error(`reload: ${body.error}`);
  }
  return (await expect(res, "reload")).json();
}

export async function fetchBrowsersRaw(): Promise<string> {
  const res = await expect(await fetch("/admin/api/config/browsers"), "registry");
  return res.text();
}

export async function putBrowsersRaw(body: string): Promise<void> {
  const res = await fetch("/admin/api/config/browsers", {
    method: "PUT",
    headers: { "content-type": "application/json" },
    body,
  });
  if (res.status === 400) {
    const err = await res.json().catch(() => ({ error: "invalid registry" }));
    throw new Error(`registry: ${err.error}`);
  }
  await expect(res, "registry");
}

export interface UserEntry {
  name: string;
  admin: boolean;
}

export async function fetchUsers(): Promise<UserEntry[]> {
  return (await expect(await fetch("/admin/api/users"), "users")).json();
}

export async function putUser(
  name: string,
  body: { password?: string; admin: boolean },
): Promise<void> {
  const res = await fetch(`/admin/api/users/${encodeURIComponent(name)}`, {
    method: "PUT",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body),
  });
  if (res.status === 400 || res.status === 409) {
    const err = await res.json().catch(() => ({ error: `HTTP ${res.status}` }));
    throw new Error(`users: ${err.error}`);
  }
  await expect(res, "users");
}

export async function deleteUser(name: string): Promise<void> {
  await expect(
    await fetch(`/admin/api/users/${encodeURIComponent(name)}`, { method: "DELETE" }),
    "users",
  );
}

export interface QuotasResponse {
  default: number;
  users: Record<string, number>;
  inUse: Record<string, number>;
}

export async function fetchQuotas(): Promise<QuotasResponse> {
  return (await expect(await fetch("/admin/api/quotas"), "quotas")).json();
}

export async function putQuotas(q: { default: number; users: Record<string, number> }): Promise<void> {
  await expect(
    await fetch("/admin/api/quotas", {
      method: "PUT",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(q),
    }),
    "quotas",
  );
}

export interface RecordingInfo {
  id: string;
  video: boolean;
  videoBytes: number;
  log: boolean;
  modified: string;
  /** Session metadata from the `{id}.json` sidecar (may be empty for
   *  recordings captured before the sidecar was written). */
  browser: string;
  version: string;
  platform: string;
  owner: string | null;
  started: string;
}

export async function fetchRecordings(): Promise<RecordingInfo[]> {
  return (await expect(await fetch("/admin/api/recordings"), "recordings")).json();
}

export function videoUrl(id: string): string {
  return `/video/${encodeURIComponent(id)}.mp4`;
}

export function logDownloadUrl(id: string): string {
  return `/logs/${encodeURIComponent(id)}.log`;
}

export async function fetchLog(sessionId: string): Promise<string> {
  const res = await fetch(`/logs/${sessionId}.log`);
  if (!res.ok) throw new Error(`log: HTTP ${res.status}`);
  return res.text();
}

export function vncUrl(sessionId: string): string {
  const proto = location.protocol === "https:" ? "wss" : "ws";
  return `${proto}://${location.host}/vnc/${sessionId}`;
}
