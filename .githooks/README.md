# Git hooks

Versioned hooks for mica. Activate them in your local clone:

```bash
git config core.hooksPath .githooks
```

## What runs

| Hook | What it does |
|------|--------------|
| `pre-commit` | `cargo fmt --all` (auto-fix, re-stage) + `cargo clippy --all-targets -- -D warnings` |

The hook only runs when staged changes touch `*.rs` or `Cargo.{toml,lock}`.

To bypass once: `git commit --no-verify`. Don't make a habit of it — CI runs the same checks.
