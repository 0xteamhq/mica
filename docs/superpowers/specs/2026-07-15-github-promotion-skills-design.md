# GitHub promotion toolkit — design

Date: 2026-07-15
Status: tweet skills implemented; daily-housekeeping pending

Goal: promote mica legitimately — keep the project verifiably healthy and visibly
alive, and produce promotion content the maintainer reviews and posts themselves.
Explicitly out of scope: anything that fakes activity (fake stars, sock puppets,
padding commits).

## Component 1: `/daily-housekeeping` (project skill — NOT YET BUILT)

Location: `.claude/skills/daily-housekeeping/SKILL.md`, checked into this repo.

Run flow:

1. **Health check.** `cargo test --all`; UI build (`npm ci --prefix ui && npm run
   build --prefix ui`); quick-start smoke test — run the published
   `ghcr.io/0xteamhq/mica:latest` exactly as the README Docker quick start says
   (with `tests/fixtures/browsers.json`), curl `/ping`, create + delete one
   WebDriver session, tear down. Failures are recorded, not fatal; triage still runs.
   A quick-start failure is the highest-priority finding (it's what a newcomer hits).
2. **Triage.** Last-run timestamp lives in `.claude/housekeeping-state.json`
   (gitignored) so skipped days drop nothing. Fetch issues/PRs/discussions updated
   since then via `gh`. Mechanical actions (labels, milestones) applied directly;
   anything with a public voice (issue replies, PR comments) or repo content
   (README/roadmap freshness edits, issues for broken health checks) is drafted
   and shown for explicit approval before posting/committing.
3. **Morning report.** Prioritized summary: health status, applied actions,
   approved/declined drafts, open items. Then write the new timestamp.

## Component 2: `/tweet-problem`, `/tweet-engaging`, `/tweet-visibility` (global skills — BUILT)

Location: `~/.claude/skills/tweet-{problem,engaging,visibility}/SKILL.md`.
Project-agnostic: subject repo is detected from the cwd's GitHub remote, falling
back to `0xteamhq/mica`.

Shared behavior:
- Context gathered fresh per run: README, repo description, recent merged
  PRs/releases via `gh`.
- Anti-repetition state shared across all three skills, one file per repo:
  `~/.claude/tweet-state/<owner>-<repo>.json` with
  `{ "used": [ { date, skill, angle, tweet } ] }`. The chosen tweet is logged
  after the user picks.
- Output: 2–3 candidates, ≤280 chars, hook-first, repo link, no hashtag walls,
  no auto-posting (no Twitter API) — the user posts manually.
- Optional topic argument overrides angle selection.
- Honesty guardrail: claims must be verifiable from the repo's own docs; for mica
  that means the CLAUDE.md phase table gates what counts as "shipped".

Per-command voice:
- `tweet-problem` — pain-point storytelling; soft or no sell.
- `tweet-engaging` — product marketing; intrigue hooks, incumbent comparisons.
- `tweet-visibility` — announcements of merged/released work only, benefit-first.

## Testing plan

Each skill is verified by running it end-to-end for real (application test):
tweet skills generate real candidates from live repo state; daily-housekeeping
runs one full health-check + triage pass after it's built.
