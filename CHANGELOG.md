# Changelog

Notable changes to DevOS, newest first. Versions follow [SemVer](https://semver.org);
DevOS is pre-1.0, so the interface may still move between minor versions.

**This file is generated** from the commit history by `pnpm gen:changelog` —
do not edit it by hand. Only `feat`, `fix` and `perf` commits appear, plus
anything marked breaking: those are the ones that change something for a person
using the app. Each release says how many other commits it contains.

## 0.1.0 — unreleased

### Added

- self-update, release pipeline, and public-repo groundwork `19609fd`
- **ai** — add Gemini as a provider `b3d0ea8`
- **audit** — write the audit log that has existed since M0 `895f3e9`
- **backups** — restore a snapshot from inside the app `b85afd8`
- snippet library (M5) `540e645`
- **terminal** — derive the xterm scheme from theme tokens `4e1712d`
- **ui** — theme system with Midnight, Daylight and Obsidian (M5) `4738ef3`
- **index** — tree-sitter symbol extraction completes M2 retrieval `61d5106`
- pick project folders from the file explorer `5035b02`
- screenshot to GitHub issue (M4) — completes the milestone `5cb7884`
- **index** — hybrid lexical + vector code search `e339c69`
- **kernel** — automatic backups + boot phase timings `a9af306`
- Vercel deployments (read-only) + secret manager UI (M4) `cbb2718`
- system metrics + website uptime monitor (M4 watchers) `d8a201f`
- database manager — SQLite browser, schema explorer, SQL editor (M3) `18bcaf7`
- Docker module + API client (M3) `59795c6`
- file explorer with tree, preview, and dual search `0e2d997`
- automatic command-failure watcher via OSC 133 shell integration `81f4724`
- project memory, notification center, terminal AI diagnosis `a80469b`
- write/execute AI tools with per-call approval + FTS5 project index `40959cc`
- AI tool calling with read-only tools and explicit grant `111fa86`
- DevOS foundation (M0) + daily-driver core (M1) `3e18e3d`

### Fixed

- **security** — screenshot retention, path containment, prototype-chain themes `8693faa`
- **plugin** — the sandbox did not hold on three axes it claimed to `a2b9dbc`
- **db** — give database errors a discriminant that survives IPC `232ca8f`
- screenshots follow DEVOS_DATA_DIR like the rest of the app's data `3dce4a4`
- **security** — close the remaining findings from the tool-surface review `4f19cfd`
- **db** — close a bypass of the SQL write gate `9a13971`

### Performance

- **web** — defer the boot-blocking overlays, 650 kB -> 506 kB entry chunk `66d9390`

_14 further commits in this release changed no user-facing behaviour (documentation, CI, tests, refactors) and are not listed. `git log` has them._

