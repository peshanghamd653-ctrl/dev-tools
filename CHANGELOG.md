# Changelog

Notable changes to DevOS, newest first. Versions follow [SemVer](https://semver.org);
DevOS is pre-1.0, so the interface may still move between minor versions.

**This file is generated** from the commit history by `pnpm gen:changelog` —
do not edit it by hand. Only `feat`, `fix` and `perf` commits appear, plus
anything marked breaking: those are the ones that change something for a person
using the app. Each release says how many other commits it contains.

## 0.2.0 — unreleased

### Added

- **secrets** — environment variable manager (Ctrl+Shift+V) [`0160902`](https://github.com/peshanghamd653-ctrl/dev-tools/commit/0160902b038b34687b29e4f5e95de384c379bb1d)
- **security** — flag outdated dependencies (npm/pnpm; cargo reports Unsupported) [`51307ca`](https://github.com/peshanghamd653-ctrl/dev-tools/commit/51307cab5d380a482c61b69e1f5fe7999edc69f6)
- **security** — flag Docker containers running as root [`744b189`](https://github.com/peshanghamd653-ctrl/dev-tools/commit/744b1895a848712cab08497406e882178562d4c1)
- **security** — flag .env files that aren't gitignored [`9a6c820`](https://github.com/peshanghamd653-ctrl/dev-tools/commit/9a6c8203997d03f764eb3340f3de1b4ab57c241c)
- **toolbox** — five more tools — XML formatter, JSON to YAML, diff, Markdown preview, HTTP status lookup [`8da265c`](https://github.com/peshanghamd653-ctrl/dev-tools/commit/8da265ccfc9ec4b8e10f79cb8f6dfe35a7159a23)
- **security** — wire the security center to a real page (Ctrl+Shift+E) [`96972a4`](https://github.com/peshanghamd653-ctrl/dev-tools/commit/96972a460d718a61cb17414a79928f7318bd9ab1)
- **palette** — natural-language commands in the Command Palette (item 14) [`f5572d6`](https://github.com/peshanghamd653-ctrl/dev-tools/commit/f5572d642a8a3b2b030f00a8c66d83556572d29e)
- **security** — security center backend — git, secret scan, dependency audit [`a3d96a1`](https://github.com/peshanghamd653-ctrl/dev-tools/commit/a3d96a18d657b4457a4d0c941afdf336c79f8065)
- **toolbox** — built-in utility toolbox (roadmap item 17) [`818dc3f`](https://github.com/peshanghamd653-ctrl/dev-tools/commit/818dc3f16d2688bb48970b66959e3d4b831e7a88)
- **security** — redact secrets before AI tool output reaches the model [`241f8be`](https://github.com/peshanghamd653-ctrl/dev-tools/commit/241f8befe2460888dacf255787692c2396349b85)
- **mcp** — MCP client — server config, stdio handshake, tool discovery [`ab4def1`](https://github.com/peshanghamd653-ctrl/dev-tools/commit/ab4def10f412ad45ffa8d3900f5af779fb580221)
- **index** — "go to symbol" — jump to a declaration by name, Ctrl+T [`7dd8875`](https://github.com/peshanghamd653-ctrl/dev-tools/commit/7dd88750e2fd81cac663232cfd4f7dee70dba85d)
- **ai** — categorize project memory (architecture/convention/decision/known-issue) [`46119ed`](https://github.com/peshanghamd653-ctrl/dev-tools/commit/46119edcc9a9021e5ded12f8556d9cf7ef1b2c46)
- **system** — performance profiler — CPU/memory history and charts [`a7cb6de`](https://github.com/peshanghamd653-ctrl/dev-tools/commit/a7cb6de77185101931f8a21c5c232ce228c377ad)
- **api** — environments and {{VAR}} substitution for the API client [`1f6ecb9`](https://github.com/peshanghamd653-ctrl/dev-tools/commit/1f6ecb9b48f2504fef63ca0732ff036877557bcf)
- **ai** — git-aware tools — inspect a diff, commit, create a branch [`5785523`](https://github.com/peshanghamd653-ctrl/dev-tools/commit/57855230c7e6d7a02a2c58ae05a925ce230bb71b)
- **ai** — add a structured lint tool alongside run_tests [`a0e9df7`](https://github.com/peshanghamd653-ctrl/dev-tools/commit/a0e9df7913d7d0b5af46be4519b2c96284286bc1)
- **ai** — add a structured test-runner tool, and fix a provider-naming bug it surfaced [`d5c83b5`](https://github.com/peshanghamd653-ctrl/dev-tools/commit/d5c83b5cb49c231f0ee99aef2f66793b89852ddd)
- **ai** — extend tool calling to Ollama [`f94fe62`](https://github.com/peshanghamd653-ctrl/dev-tools/commit/f94fe624467b5e4d6c5ae8f7d74f331127a99dea)
- tell the user why DevOS could not start [`a2e69ef`](https://github.com/peshanghamd653-ctrl/dev-tools/commit/a2e69ef086fd5b992eed602b56c794dab923bdea)

### Fixed

- **deps** — bump xcb 1.7.0 -> 1.7.1, dropping vulnerable quick-xml 0.30.0 [`05cf010`](https://github.com/peshanghamd653-ctrl/dev-tools/commit/05cf010704e8be11c41a65ad196eef2319215a91)
- **kernel** — serialize devos-kernel backup tests instead of widening the wait again [`b83aec4`](https://github.com/peshanghamd653-ctrl/dev-tools/commit/b83aec4790c92c24abbed8a1275ab6e7bb8c86ac)
- **kernel** — widen shm-release wait to 120s, correct the earlier theory [`0faaec1`](https://github.com/peshanghamd653-ctrl/dev-tools/commit/0faaec1c5c2a6e1f2ba3e9f8fdd8370231b3fcc0)
- **kernel** — widen the shm-release wait again — 30s still wasn't enough on CI [`efc4f35`](https://github.com/peshanghamd653-ctrl/dev-tools/commit/efc4f35ffbbf16c5d10cac06b9200c37c63c23ac)
- **kernel** — give CI enough time to see Windows release a memory-mapped file [`ac95890`](https://github.com/peshanghamd653-ctrl/dev-tools/commit/ac958906094f6a01b273637094c3df59e6192e05)
- **security** — stop sending the Gemini key to the Ollama endpoint [`b1d1be8`](https://github.com/peshanghamd653-ctrl/dev-tools/commit/b1d1be8a80ae25276dad0d2c87360d6a445a8406)
- **web** — make the first run survivable, starting with Ollama being unreachable [`8e00e4a`](https://github.com/peshanghamd653-ctrl/dev-tools/commit/8e00e4a4abed3a069bf7c673bca5d1707615e14d)

### Performance

- **kernel** — move the daily backup off the boot path [`d7a9c46`](https://github.com/peshanghamd653-ctrl/dev-tools/commit/d7a9c461c7706d656a4fa2df3f3ffdb445879fb1)
- find where startup actually goes — 90-96% is WebView2 creation [`299da86`](https://github.com/peshanghamd653-ctrl/dev-tools/commit/299da86bed32a64ab00a1051f1ee2f2e087e3ef3)

_4 further commits in this release changed no user-facing behaviour (documentation, CI, tests, refactors) and are not listed. `git log` has them._

## 0.1.0 — 2026-08-08

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

- pin migration line endings, which decided whether the app starts at all [`c7a71a3`](https://github.com/peshanghamd653-ctrl/dev-tools/commit/c7a71a3734f4eb03642c9680f3f99cbb89fbc32e)
- **deps** — re-resolve nanoid to the patched 3.3.17 [`172b048`](https://github.com/peshanghamd653-ctrl/dev-tools/commit/172b0484f3529516b4bc60d69c117318990e2c7f)
- **security** — screenshot retention, path containment, prototype-chain themes [`8693faa`](https://github.com/peshanghamd653-ctrl/dev-tools/commit/8693faa0037470663c968f61432d3de79babb782)
- **plugin** — the sandbox did not hold on three axes it claimed to [`a2b9dbc`](https://github.com/peshanghamd653-ctrl/dev-tools/commit/a2b9dbcb5e26b0a986d4fd693a04f576a6426da8)
- **db** — give database errors a discriminant that survives IPC [`232ca8f`](https://github.com/peshanghamd653-ctrl/dev-tools/commit/232ca8f9ce07dbd64079141c9fd4cd9088d686c5)
- screenshots follow DEVOS_DATA_DIR like the rest of the app's data [`3dce4a4`](https://github.com/peshanghamd653-ctrl/dev-tools/commit/3dce4a413907fa9b46bde03d56101882da9f6849)
- **security** — close the remaining findings from the tool-surface review [`4f19cfd`](https://github.com/peshanghamd653-ctrl/dev-tools/commit/4f19cfdc0d6f85ecee805268398303d909c02be6)
- **db** — close a bypass of the SQL write gate [`9a13971`](https://github.com/peshanghamd653-ctrl/dev-tools/commit/9a139712dbb61dbf63dda42eb9af9ebc4fe354c5)

### Performance

- **web** — defer the boot-blocking overlays, 650 kB -> 506 kB entry chunk [`66d9390`](https://github.com/peshanghamd653-ctrl/dev-tools/commit/66d9390022bd64f0816b31eba7e4935b6794a43a)

_21 further commits in this release changed no user-facing behaviour (documentation, CI, tests, refactors) and are not listed. `git log` has them._

