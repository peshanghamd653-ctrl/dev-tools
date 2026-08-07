# Changelog

Notable changes to DevOS, newest first. Versions follow [SemVer](https://semver.org);
DevOS is pre-1.0, so the interface may still move between minor versions.

**This file is generated** from the commit history by `pnpm gen:changelog` —
do not edit it by hand. Only `feat`, `fix` and `perf` commits appear, plus
anything marked breaking: those are the ones that change something for a person
using the app. Each release says how many other commits it contains.

## 0.1.0 — unreleased

### Added

- self-update, release pipeline, and public-repo groundwork [`19609fd`](https://github.com/peshanghamd653-ctrl/dev-tools/commit/19609fdd3c93dae1be58b23b5bccf25b5114aa92)
- **ai** — add Gemini as a provider [`b3d0ea8`](https://github.com/peshanghamd653-ctrl/dev-tools/commit/b3d0ea83256df13278fca5321ef5dcd6086e78f8)
- **audit** — write the audit log that has existed since M0 [`895f3e9`](https://github.com/peshanghamd653-ctrl/dev-tools/commit/895f3e9785b161a26990f4d04e17ed6a1e91dd7f)
- **backups** — restore a snapshot from inside the app [`b85afd8`](https://github.com/peshanghamd653-ctrl/dev-tools/commit/b85afd880699853277a45c15a214a1f9aacacd23)
- snippet library (M5) [`540e645`](https://github.com/peshanghamd653-ctrl/dev-tools/commit/540e645ee4a53902938be4bad205d7abab5f301e)
- **terminal** — derive the xterm scheme from theme tokens [`4e1712d`](https://github.com/peshanghamd653-ctrl/dev-tools/commit/4e1712d6b0992ce47874ed2c645f28029ac767a7)
- **ui** — theme system with Midnight, Daylight and Obsidian (M5) [`4738ef3`](https://github.com/peshanghamd653-ctrl/dev-tools/commit/4738ef3ceda6ac60f90e8b7a71a5f063746e591e)
- **index** — tree-sitter symbol extraction completes M2 retrieval [`61d5106`](https://github.com/peshanghamd653-ctrl/dev-tools/commit/61d5106aa0662e4c85a59a1be969ffccb7bf65b7)
- pick project folders from the file explorer [`5035b02`](https://github.com/peshanghamd653-ctrl/dev-tools/commit/5035b0288b1dbed97b9e1e1d8ebc44005ba7621f)
- screenshot to GitHub issue (M4) — completes the milestone [`5cb7884`](https://github.com/peshanghamd653-ctrl/dev-tools/commit/5cb7884bb3353d145e731c358db3afdd3eb2bf14)
- **index** — hybrid lexical + vector code search [`e339c69`](https://github.com/peshanghamd653-ctrl/dev-tools/commit/e339c694ddd78e416bb5e921f8e3db76be0d1343)
- **kernel** — automatic backups + boot phase timings [`a9af306`](https://github.com/peshanghamd653-ctrl/dev-tools/commit/a9af306f6cfe2890b7652225b7f42d15eaaafdd8)
- Vercel deployments (read-only) + secret manager UI (M4) [`cbb2718`](https://github.com/peshanghamd653-ctrl/dev-tools/commit/cbb2718110ba982773db9a1a8bf29fe99e08b329)
- system metrics + website uptime monitor (M4 watchers) [`d8a201f`](https://github.com/peshanghamd653-ctrl/dev-tools/commit/d8a201ffb5f474c24e4b294b06ebdc981d059353)
- database manager — SQLite browser, schema explorer, SQL editor (M3) [`18bcaf7`](https://github.com/peshanghamd653-ctrl/dev-tools/commit/18bcaf7843e73e16efd8194c699cb564d535b22e)
- Docker module + API client (M3) [`59795c6`](https://github.com/peshanghamd653-ctrl/dev-tools/commit/59795c6ab2313b0e11a63d233d0e6777c4248d4a)
- file explorer with tree, preview, and dual search [`0e2d997`](https://github.com/peshanghamd653-ctrl/dev-tools/commit/0e2d997ff8ce331b1200fc185600820b471d951c)
- automatic command-failure watcher via OSC 133 shell integration [`81f4724`](https://github.com/peshanghamd653-ctrl/dev-tools/commit/81f4724b6b2b0a5c446c7b69ffc67210db09bd47)
- project memory, notification center, terminal AI diagnosis [`a80469b`](https://github.com/peshanghamd653-ctrl/dev-tools/commit/a80469b680d76b96d992d0b655494c934469ec55)
- write/execute AI tools with per-call approval + FTS5 project index [`40959cc`](https://github.com/peshanghamd653-ctrl/dev-tools/commit/40959cc2b4666ddce55ba8922d4a3361659c796d)
- AI tool calling with read-only tools and explicit grant [`111fa86`](https://github.com/peshanghamd653-ctrl/dev-tools/commit/111fa86b75f0ce71f17ac1e2f8467f0a6efc4102)
- DevOS foundation (M0) + daily-driver core (M1) [`3e18e3d`](https://github.com/peshanghamd653-ctrl/dev-tools/commit/3e18e3db175d056d691d81ad7733342c0d979ee4)

### Fixed

- **security** — screenshot retention, path containment, prototype-chain themes [`8693faa`](https://github.com/peshanghamd653-ctrl/dev-tools/commit/8693faa0037470663c968f61432d3de79babb782)
- **plugin** — the sandbox did not hold on three axes it claimed to [`a2b9dbc`](https://github.com/peshanghamd653-ctrl/dev-tools/commit/a2b9dbcb5e26b0a986d4fd693a04f576a6426da8)
- **db** — give database errors a discriminant that survives IPC [`232ca8f`](https://github.com/peshanghamd653-ctrl/dev-tools/commit/232ca8f9ce07dbd64079141c9fd4cd9088d686c5)
- screenshots follow DEVOS_DATA_DIR like the rest of the app's data [`3dce4a4`](https://github.com/peshanghamd653-ctrl/dev-tools/commit/3dce4a413907fa9b46bde03d56101882da9f6849)
- **security** — close the remaining findings from the tool-surface review [`4f19cfd`](https://github.com/peshanghamd653-ctrl/dev-tools/commit/4f19cfdc0d6f85ecee805268398303d909c02be6)
- **db** — close a bypass of the SQL write gate [`9a13971`](https://github.com/peshanghamd653-ctrl/dev-tools/commit/9a139712dbb61dbf63dda42eb9af9ebc4fe354c5)

### Performance

- **web** — defer the boot-blocking overlays, 650 kB -> 506 kB entry chunk [`66d9390`](https://github.com/peshanghamd653-ctrl/dev-tools/commit/66d9390022bd64f0816b31eba7e4935b6794a43a)

_15 further commits in this release changed no user-facing behaviour (documentation, CI, tests, refactors) and are not listed. `git log` has them._

