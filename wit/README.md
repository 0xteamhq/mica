# `mica:plugin` WIT contract

The interface mica's WASM plugin host (Phase 5, [Linear 0XT-78](https://linear.app/0xhq/issue/0XT-78)) implements. Plugins are [WebAssembly Components](https://component-model.bytecodealliance.org/) targeting the `mica:plugin/plugin` world defined here.

## Files

| File | Direction | What's in it |
|---|---|---|
| `types.wit` | shared | Records / enums shared by plugin and host: `capabilities`, `file-info`, `session-info`, `header`, `http-request`, `http-response`, `plugin-error`, etc. |
| `lifecycle.wit` | plugin → host | `init(cfg)` / `shutdown()` |
| `session.wit` | plugin → host | `on-create(session, caps) → accept(caps') \| reject(reason)` and `on-end(info)` |
| `artifact.wit` | plugin → host | `on-file-created(file) → keep \| skip \| s3 \| custom-uri` |
| `http.wit` | plugin → host | `intercept-request` / `intercept-response` middleware |
| `host.wit` | host → plugin | Host imports — capability-gated (`host-log`, `clock`, `http-client`, `s3-write`, `state`) |
| `world.wit` | — | The `plugin` world tying it together |

## Hook semantics

### Order
Plugins run in **load order** (the order they appear in `--plugin <path>` flags or `[[plugins]]` config sections). Each plugin sees the previous plugin's output for `on-create`, `intercept-request`, and `intercept-response`.

### Short-circuiting
- `session.on-create` — first `reject(reason)` wins; subsequent plugins are not called.
- `artifact.on-file-created` — first non-`keep` decision wins; subsequent plugins are not called.
- `http.intercept-request` — first `short-circuit(response)` wins.

### Async / blocking
Hooks are synchronous in the WIT (no `wasi:io/poll` exposed yet). The host runs each hook on a dedicated tokio task with a per-hook timeout:

| Hook | Default timeout | Override flag |
|---|---|---|
| `lifecycle.init` | 10 s | `--plugin-init-timeout` |
| `lifecycle.shutdown` | 5 s | `--plugin-shutdown-timeout` |
| `session.on-create` | 5 s (sum across plugins) | `--plugin-on-create-timeout` |
| `session.on-end` | best-effort, no timeout | — |
| `artifact.on-file-created` | best-effort, no timeout | — |
| `http.intercept-request` | 200 ms (sum across plugins) | `--plugin-http-timeout` |
| `http.intercept-response` | 200 ms (sum across plugins) | `--plugin-http-timeout` |

Exceeding `on-create` or `intercept-request` returns 503 to the WebDriver client. Best-effort hooks are logged at warn level on timeout.

## Capability grants

Mica gates plugin imports at instantiation time. The operator declares grants via `--plugin-grants <name>=<caps>`:

```bash
mica \
  --plugin /etc/mica/plugins/quota.wasm \
  --plugin-grants quota=state \
  --plugin /etc/mica/plugins/audit.wasm \
  --plugin-grants audit=s3-write,http-client
```

| Capability | What it permits | Always granted |
|---|---|---|
| `host-log` | Structured logging via `host-log.log(level, msg)` | ✅ |
| `clock` | Wall-clock time via `clock.now()` | ✅ |
| `http-client` | Outbound HTTP via `http-client.send(req)` | — |
| `s3-write` | `PutObject` to S3 (no read, no delete) | — |
| `state` | Per-plugin scratch KV store | — |

A plugin importing a non-granted capability **fails to instantiate** at startup with a clear error — fail-closed by design.

## Building a plugin

Pick any language with a Component Model toolchain. A Rust skeleton:

```toml
# Cargo.toml
[package]
name = "my-mica-plugin"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
wit-bindgen = "0.37"
```

```rust
// src/lib.rs
wit_bindgen::generate!({
    path: "../mica/wit",
    world: "plugin",
});

struct Plugin;

impl exports::mica::plugin::lifecycle::Guest for Plugin {
    fn init(cfg: exports::mica::plugin::lifecycle::Config)
        -> Result<(), mica::plugin::types::PluginError> {
        mica::plugin::host_log::log(
            mica::plugin::host_log::Level::Info,
            &format!("loaded plugin {} v{}", cfg.name, cfg.version),
        );
        Ok(())
    }
    fn shutdown() {}
}

// Implement only the interfaces you care about.
// wit-bindgen requires all `export` impls; use the empty default for
// hooks you don't override (see `examples/` once they're shipped).

export!(Plugin);
```

Build:

```bash
cargo build --release --target wasm32-wasip2
wasm-tools component new \
  target/wasm32-wasip2/release/my_mica_plugin.wasm \
  -o my-plugin.wasm
```

Then deploy with `--plugin /etc/mica/plugins/my-plugin.wasm`.

## Validate the contract

```bash
wasm-tools component wit wit/             # resolve + pretty-print
wit-bindgen rust wit/ --world plugin --async none -o /tmp/_   # generate bindings
```

## Versioning

The package is `mica:plugin@0.1.0` while Phase 5 is in progress. SemVer applies:
- Patch = doc / clarification only.
- Minor = additive — new optional interfaces, new optional record fields wrapped in `option<...>`.
- Major = breaking. Old plugins refuse to load against an incompatible host (mica logs the version mismatch and either skips or refuses to start per `--plugin-load`).

## Status

- [0XT-79](https://linear.app/0xhq/issue/0XT-79) (this) — contract
- [0XT-78](https://linear.app/0xhq/issue/0XT-78) — host (wasmtime) integration
- [0XT-80](https://linear.app/0xhq/issue/0XT-80) — sample plugins
