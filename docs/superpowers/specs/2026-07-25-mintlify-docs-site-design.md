# Mintlify docs site under /mintlify — design

Date: 2026-07-25. Status: approved.

## Goal

Full user-facing documentation site for mica, authored as a Mintlify project in
`/mintlify`, local-only (no deployment wiring yet). API reference is generated
from the OpenAPI spec.

## Decisions

- Single **Documentation** tab of prose MDX, grouped (Get Started / Concepts /
  Guides / Reference) because guide depth (router, isolation, admin, plugins)
  needs grouping.
- **API Reference tab dropped** (owner call, 2026-07-25, superseding the
  earlier OpenAPI-tab decision): the OpenAPI spec stays at
  `deploy/openapi/mica.yaml` and is served by the binary at `/openapi.yaml`;
  the docs link to it instead of mirroring it, so the prek drift hook was
  removed too.
- Phase status is respected: scaffolded features (Firecracker, Cloud
  Hypervisor, BiDi streaming, WASM plugin host) get explicit "in development"
  callouts and are never documented as shipped.
- Content sourced from README.md, deploy/routing/README.md, Helm chart README,
  docker/*/README.md, deploy/compose, examples/plugins, and code (`--help`
  output, `mica:options` handling). No invented wire formats.

## Structure

```
mintlify/
├── docs.json
├── index.mdx                  # landing: what/why
├── quickstart.mdx             # Docker / Helm / source tabs
├── concepts/{architecture,backends,isolation}.mdx
├── guides/{docker,kubernetes,router,admin,warm-pool,artifacts,
│           browser-images,clients,plugins}.mdx
└── reference/{browsers-json,cli,capabilities,endpoints}.mdx
```

(No `api-reference/` directory — see the dropped-tab decision above.)

## Verification

`mint broken-links` (or `npx mint`) passes; every `docs.json` nav entry
resolves to a file and every `.mdx` file appears in the nav; spot-check
rendered pages with `mint dev` if available.
