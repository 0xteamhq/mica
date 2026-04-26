# Multi-host routing for mica

**Decision:** mica does not ship its own L7 router. Phase 7 was a "decide first" ticket; we considered porting `aerokube/ggr` to Rust and chose to use existing infrastructure instead.

## Why we don't ship a router

1. **K8s Ingress is the right primitive on managed clouds.** `K8sBackend` already labels every Pod and Service with `mica/owner=<replica_id>` (P3.5 / 0XT-68). Standard `nginx-ingress`, `Traefik`, `AWS Load Balancer Controller`, or `Istio` configurations route session-proxy traffic by header / cookie back to the owning mica replica with no mica-specific code.

2. **Cloudflare / Fastly / HAProxy / Envoy** all do header-based hashing and weighted backends out of the box. Ops already know these tools.

3. **Fly Machines** is the natural fit for the Phase-4 Firecracker path — Fly's anycast routes traffic to the closest region without any router we'd ship.

4. **One less moving part.** A Rust port of ggr is real surface area to maintain (XML quotas, file-watching, weighted random, /status aggregation). The cost is hard to justify when Ingress controllers solve the same problem.

5. **Sticky sessions, not load balancing.** Mica's session map is per-replica. The "router" we'd build is really just a sticky-session reverse proxy — ingress controllers do this with one annotation.

## Example configs

- `nginx-ingress.yaml` — K8s Ingress with cookie-based session affinity matched to the `mica/owner` label.
- `haproxy.cfg` — HAProxy stick-table on the `X-Selenoid-Session-Id` header for non-K8s deployments.
- `cloudflare.tf` — Cloudflare Load Balancer with origin pools per region; sticky on the session-id header.

## Aggregated `/status` across the fleet (formerly P7.2)

Instead of porting `ggr-ui`, point a Prometheus or Datadog scraper at every mica replica's `/status` endpoint. Mica already emits per-replica counters; the aggregation belongs in your existing observability stack, not in a new daemon. Sample Prometheus job in `prometheus.yaml`.

## When to revisit

If a customer deploys mica without K8s Ingress / Cloudflare / HAProxy / Fly anycast — *and* needs cross-region session routing — open a follow-up ticket and we'll consider it. The strategy doc puts this at "likely unnecessary if you pick Fly/K8s with built-in LB"; we hold to that until evidence says otherwise.
