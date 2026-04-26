# mica Helm chart

Deploys [mica](https://github.com/0xteamhq/mica) — a Rust browser grid that speaks W3C WebDriver — onto Kubernetes.

## Install

```bash
# From a checkout
helm install mica ./deploy/k8s/charts/mica \
  --namespace mica --create-namespace
```

## Values

See `values.yaml` for the full surface. Common knobs:

| Key | Default | What it does |
|---|---|---|
| `image.repository` | `ghcr.io/0xteamhq/mica` | Image repo |
| `image.tag` | chart `appVersion` | Image tag |
| `replicaCount` | `3` | Replicas (anti-affinity by hostname) |
| `backend.mode` | `k8s` | `k8s` (RBAC for future K8sBackend) or `docker` (mounts /var/run/docker.sock) |
| `mica.limit` | `5` | Max parallel sessions per replica |
| `mica.warmPool.min` | `0` | Warm pool size; >0 enables (P2.3) |
| `s3.bucket` | empty | Enable S3 artifact upload |
| `rbac.scope` | `namespace` | `namespace` or `cluster` |
| `rbac.extraNamespaces` | `[]` | Extra namespaces to grant the namespace-scoped Role |
| `ingress.enabled` | `false` | Toggle Ingress |
| `service.sessionAffinity` | `ClientIP` | Pin sessions to a replica until 0XT-68 |
| `persistence.video.type` | `emptyDir` | `emptyDir` / `pvc` / `hostPath` |

## RBAC

The chart provisions one of:

- **Namespace scope** (`rbac.scope=namespace`) — `Role` + `RoleBinding` per release namespace, replicated to `rbac.extraNamespaces`.
- **Cluster scope** (`rbac.scope=cluster`) — `ClusterRole` + `ClusterRoleBinding`.

Verbs:

| Resource | Verbs | Why |
|---|---|---|
| `pods` | create, delete, get, list, watch | spawn / reap browser pods (K8sBackend) |
| `pods/log` | get | tail container logs |
| `services` | create, delete, get | per-session DNS handle (P3.3) |
| `configmaps` | get | mount `browsers.json` |
| `events` | create | k8s event emission for observability |

## Caveats

- **K8sBackend (0XT-64) is not implemented yet.** With `backend.mode=k8s` the chart deploys mica but session creates fail until the backend lands. Use `backend.mode=docker` to mount the node's `/var/run/docker.sock` (works on docker-shim / sysbox nodes; broken on containerd-only).
- **Multi-replica session ownership (0XT-68) not implemented.** With `replicaCount > 1`, sessions are bound to the replica that created them — the Service uses `sessionAffinity: ClientIP` so a client keeps hitting the same replica.
- **`terminationGracePeriodSeconds` should match `mica.gracefulPeriod`.** Default of 330s (mica defaults to 300s + 30s headroom) lets in-flight sessions drain before SIGKILL.

## Render to raw YAML

For users who don't run Helm in production:

```bash
helm template mica ./deploy/k8s/charts/mica \
  -n mica \
  -f deploy/k8s/examples/values-production.yaml \
  > mica.yaml
kubectl apply -f mica.yaml
```

## Examples

- `deploy/k8s/examples/values-basic.yaml` — single replica, dev cluster
- `deploy/k8s/examples/values-production.yaml` — 3 replicas, ingress + TLS, warm pool, S3
