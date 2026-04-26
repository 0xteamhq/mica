# Kubernetes deploy

mica on Kubernetes — Helm chart and reference values.

```
deploy/k8s/
├── charts/mica/             Helm chart (primary deliverable)
└── examples/
    ├── values-basic.yaml    Single replica, Docker backend, dev cluster
    └── values-production.yaml  3 replicas, ingress + TLS, warm pool, S3
```

## Quick install

```bash
helm install mica ./deploy/k8s/charts/mica \
  -n mica --create-namespace \
  -f deploy/k8s/examples/values-basic.yaml

kubectl -n mica port-forward svc/mica 4444:4444
curl http://localhost:4444/ping
```

## Without Helm

```bash
helm template mica ./deploy/k8s/charts/mica \
  -n mica \
  -f deploy/k8s/examples/values-production.yaml \
  > mica.yaml
kubectl apply -f mica.yaml
```

## What you get

- ServiceAccount + Role / ClusterRole for `pods`, `pods/log`, `services`, `configmaps`, `events` (forward-looking — see [chart README](./charts/mica/README.md#caveats))
- Deployment with anti-affinity across nodes
- ClusterIP Service with `sessionAffinity: ClientIP`
- Optional Ingress
- Optional PodDisruptionBudget
- ConfigMap with `browsers.json`

## Status

Tracks Linear ticket [0XT-67](https://linear.app/0xhq/issue/0XT-67/p34-rbac-manifest-helm-chart). Two follow-ups still required for end-to-end K8s execution:

- **0XT-64 K8sBackend skeleton** — until this lands, session creates fail. Use `backend.mode=docker` as an interim.
- **0XT-68 Multi-replica session ownership** — until this lands, `replicaCount > 1` requires `sessionAffinity: ClientIP` (the chart default).

See the [chart README](./charts/mica/README.md) for the full values reference.
