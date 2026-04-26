# Security policy

## Reporting a vulnerability

**Please do not open public GitHub issues for security vulnerabilities.**

Instead, use [GitHub's private security advisory flow](https://github.com/0xteamhq/mica/security/advisories/new). This routes the report directly to the maintainers and lets us coordinate a fix and disclosure timeline with you.

If GitHub Security Advisories aren't available to you for some reason, email the maintainers at `security@0xteam.dev` with:

- A description of the vulnerability and the impact
- Steps to reproduce (proof-of-concept code, request payloads, or a minimal failing test case)
- The version(s) affected (commit SHA preferred)
- Whether you've already disclosed elsewhere

We aim to acknowledge reports within 72 hours and provide a fix or remediation plan within 14 days for high-severity issues.

## Supported versions

mica is pre-1.0. We support **the latest commit on `main`** and the most recent tagged release. Older releases get fixes only for severe issues at maintainer discretion.

## Scope

In scope:
- The mica binary (HTTP handlers, Docker / K8s backends, isolation drivers, plugin host, S3 uploader)
- The Helm chart (`deploy/k8s/charts/mica/`) and example values
- The published OCI images (`ghcr.io/0xteamhq/mica*`)
- Documented CLI flags, environment variables, and the `mica:options` / `X-Mica-No-Wait` wire surface

Out of scope:
- Vulnerabilities in upstream dependencies — please report those upstream, then file an advisory here once the dependency has a fix
- Misconfigurations of the operator's deployment (e.g. running mica as `root`, exposing `4444` to the internet without auth, granting `--plugin-grants` capabilities to untrusted plugins)
- DoS via legitimate WebDriver traffic — use `--limit` and `--disable-queue` to bound load

## Disclosure

We default to coordinated disclosure. Once a fix is shipped, we'll publish a GitHub Security Advisory crediting the reporter (unless you ask otherwise).
