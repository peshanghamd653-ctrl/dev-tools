# Changelog

Notable changes to DevOS, newest first. Versions follow [SemVer](https://semver.org);
DevOS is pre-1.0, so the interface may still move between minor versions.

**This file is generated** from the commit history by `pnpm gen:changelog` —
do not edit it by hand. Only `feat`, `fix` and `perf` commits appear, plus
anything marked breaking: those are the ones that change something for a person
using the app. Each release says how many other commits it contains.

## 0.1.0 — unreleased

### Added

- self-update, release pipeline, and public-repo groundwork `d6fc8b4`
- **ai** — add Gemini as a provider `2707c8a`
- **audit** — write the audit log that has existed since M0 `4b3bbc8`
- **backups** — restore a snapshot from inside the app `80fa800`
- snippet library (M5) `fd3aa4f`
- **terminal** — derive the xterm scheme from theme tokens `e764c51`
- **ui** — theme system with Midnight, Daylight and Obsidian (M5) `7ec23b0`
- **index** — tree-sitter symbol extraction completes M2 retrieval `aa2d6bc`
- pick project folders from the file explorer `257c873`
- screenshot to GitHub issue (M4) — completes the milestone `f31fa61`
- **index** — hybrid lexical + vector code search `bedda09`
- **kernel** — automatic backups + boot phase timings `ae6d821`
- Vercel deployments (read-only) + secret manager UI (M4) `49127aa`
- system metrics + website uptime monitor (M4 watchers) `cfb0f12`
- database manager — SQLite browser, schema explorer, SQL editor (M3) `62ca122`
- Docker module + API client (M3) `38e940a`
- file explorer with tree, preview, and dual search `e75d0ba`
- automatic command-failure watcher via OSC 133 shell integration `3aa95c8`
- project memory, notification center, terminal AI diagnosis `5d21bab`
- write/execute AI tools with per-call approval + FTS5 project index `eedf4da`
- AI tool calling with read-only tools and explicit grant `1b0976a`
- DevOS foundation (M0) + daily-driver core (M1) `c1310e7`

### Fixed

- **security** — screenshot retention, path containment, prototype-chain themes `a4ddd9b`
- **plugin** — the sandbox did not hold on three axes it claimed to `1bd4b0e`
- **db** — give database errors a discriminant that survives IPC `4a7f8e8`
- screenshots follow DEVOS_DATA_DIR like the rest of the app's data `a3d6d1a`
- **security** — close the remaining findings from the tool-surface review `2b40f94`
- **db** — close a bypass of the SQL write gate `7bb7142`

### Performance

- **web** — defer the boot-blocking overlays, 650 kB -> 506 kB entry chunk `5952fb2`

_13 further commits in this release changed no user-facing behaviour (documentation, CI, tests, refactors) and are not listed. `git log` has them._

