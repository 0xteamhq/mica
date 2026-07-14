# Multi-host routing for mica

**Decision (2026-07, supersedes the 2026-04 decision below):** mica ships a built-in
**router mode** — a stateless GGR-equivalent tier in the same binary:

```bash
mica --router --nodes nodes.json
```

The owner reversed the earlier call as part of the grid-completeness push
(admin control plane + multi-node): consolidating the whole Aerokube suite
(Selenoid + Moon + GGR + ggr-ui) into one binary is the adoption story, and
non-K8s / cross-region deployments — the "when to revisit" case named below —
are now in scope. The Ingress/HAProxy/Cloudflare configs in this directory
remain fully supported alternatives for K8s-native deployments that don't
want a router tier.

## Router mode

- **Stateless.** The session id returned to clients embeds the owning node:
  `base64url(node_name) + "." + upstream_id`. Any router replica routes any
  request by decoding the prefix — no shared state, no sticky LB needed in
  front of the routers (a dumb L4 balancer is enough). See
  `src/router/session_id.rs` for the wire contract. Session ids are **not**
  UUIDs in router mode; clients must treat ids as opaque (W3C-compliant).
- **Placement.** `POST /wd/hub/session` picks weighted-random among healthy
  nodes whose cached `/status` advertises the requested browser/version, and
  fails over to the next node on connect error / timeout / 5xx / 429 (other
  4xx return verbatim). No router-side queue — nodes queue, the router's
  per-attempt timeout (`--router-create-timeout`, default 5m) rides it out.
- **Health.** A background poller GETs each node's `/status` every
  `--router-health-interval` (5s). Nodes that fail
  `--router-unhealthy-threshold` consecutive polls (2) — or that report
  `"draining": true` — are excluded from *new placements only*; existing
  sessions still proxy to them. Router `/readyz` is 503 when zero nodes are
  healthy.
- **Everything proxies.** WebDriver HTTP (streamed), `/vnc` + `/session/{id}/bidi`
  (WebSocket relay), `/devtools`, `/clipboard`, `/download`, and artifacts
  (`/video/{id}.mp4`, `/logs/{id}.log`) — artifact names carry the routed id.
- **Auth.** Client auth terminates at the router (`--users`, same htpasswd
  format). Per-node credentials from nodes.json are injected on every
  forwarded request; the client's `Authorization` header never reaches nodes.
- **Aggregated `/status`** (the ggr-ui equivalent) merges the poller's cached
  node snapshots: counters summed, browsers unioned, sessions concatenated
  with a `node` field and router-prefixed ids, plus `"router": true` and a
  `nodes: [...]` array with per-node health detail. Staleness ≤ one poll
  interval.

### nodes.json

```json
{
  "nodes": [
    {
      "name": "node-a",
      "endpoint": "https://mica-a.internal:4444",
      "weight": 2,
      "region": "us-east-1",
      "labels": { "tier": "spot" },
      "username": "router",
      "password": "s3cret"
    },
    { "name": "node-b", "endpoint": "http://10.0.0.2:4444" }
  ]
}
```

- `name` — required, `[A-Za-z0-9_.-]+`, unique, **stable**: it is embedded in
  live session ids. Renaming or removing a node orphans its sessions.
- `endpoint` — required `http(s)://host:port`.
- `weight` — default 1. `0` = route-only (never placed, still proxied).
- `region` / `labels` — surfaced in `/status` for dashboards; no routing
  semantics in v1.
- `username` / `password` — optional Basic credentials the router presents to
  that node.

Hot-reload: edit the file and `kill -HUP <router pid>`. Added nodes start
`unknown` and join placement after their first successful poll.

### Ops runbook

- **Remove a node safely:** set `"weight": 0` (SIGHUP), wait until the
  aggregated `/status` shows zero sessions on it, then delete the entry.
  Alternatively `POST /admin/api/drain {"active":true}` on the node itself —
  the router sees `draining: true` on the next poll.
- **Router restarts/deploys** sever in-flight VNC/BiDi WebSocket streams
  (bounded by `--graceful-period`); WebDriver HTTP traffic is unaffected —
  the next request lands on another router replica and routes by session-id
  prefix.
- **Scaling routers:** run N replicas behind any L4/L7 balancer; no affinity
  required.
- **Plugins do not run router-side** (v1): HTTP-intercept/artifact/lifecycle
  hooks stay on the nodes.

## Alternative: existing infrastructure (2026-04 decision, still valid for K8s)

The original Phase 7 decision was to not ship a router and delegate to
existing infra. For K8s-native deployments this remains a fine choice:

1. **K8s Ingress.** `K8sBackend` labels every Pod/Service with
   `mica/owner=<replica_id>` (P3.5 / 0XT-68); standard nginx-ingress /
   Traefik / ALB / Istio configs route session traffic back to the owning
   replica. See `nginx-ingress.yaml`.
2. **HAProxy** stick-tables on `X-Mica-Session-Id` for non-K8s deployments —
   `haproxy.cfg`.
3. **Cloudflare / Fly anycast** for region routing — `cloudflare.tf`.
4. **Fleet observability:** point Prometheus/Datadog at every replica's
   `/status` — `prometheus.yaml` — or point the mica admin dashboard at a
   router's aggregated `/status`.
