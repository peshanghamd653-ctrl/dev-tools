# Release Process

**Current state: pre-release.** No version has been tagged and no release has
been published — the repository has no tags yet. This document describes what
exists today and what a first public release needs. It does not describe a
process that is already running.

The project is public and MIT-licensed, so a release is now something
strangers install, not something the author copies onto their own machine. That
changes two things: artifacts have to be reproducible from a tag, and the
licence question below stops being theoretical.

## Distribution

**Releases are GitHub Releases**, one per `v<version>` tag. There is no other
channel — no package manager, no download page, no mirror. The README points at
the Releases page and nowhere else.

**The artifacts are the two Windows installers** Tauri's bundler already
produces (`bundle.targets: "all"` in `src-tauri/tauri.conf.json`), verified by
a local `pnpm tauri build` on 2026-08-07 at version 0.1.0:

| Artifact | Path under `target/release/bundle/` | Size at 0.1.0 |
|---|---|---|
| NSIS installer | `nsis/DevOS_<version>_x64-setup.exe` | 5.9 MB |
| MSI installer | `msi/DevOS_<version>_x64_en-US.msi` | 8.4 MB |

Both are x64 Windows only — see the non-goals section. Nothing else is attached
to a release: no portable zip, no standalone `devos-desktop.exe`, no debug
symbols.

**Code signing and the in-app updater are being wired up separately**, and are
deliberately **not** documented here yet. Whoever lands them owns the section
that describes them, including where the private signing key lives (outside
this repository — `.gitignore` blocks `*.key` as a second line of defence) and
what an update manifest contains. Until then: the installers published from
this process are unsigned, Windows SmartScreen warns on first run, and the
README says so.

## What exists today

- **CI** (`.github/workflows/ci.yml`), runs on every push to `main` and on
  every PR:
  - `frontend` job (ubuntu): `pnpm install --frozen-lockfile`, lint,
    typecheck, test, `vite build`.
  - `rust` job (windows-latest, since the app is Windows-first): `cargo fmt
    --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
    `cargo test --workspace`, then a **generated-bindings check**. Those
    tests regenerate `src/shared/ipc/bindings/*.ts` through ts-rs as a side
    effect, so the job diffs that directory immediately afterwards and fails
    when the committed bindings differ from what the Rust DTOs now produce —
    without it, a stale binding passed CI silently. Untracked files are
    included (a new DTO whose binding was never committed is the case most
    worth catching, and plain `git diff` ignores untracked files), and the
    comparison ignores CR-at-EOL so a Windows checkout cannot fail it on line
    endings alone. The failure prints the diff and the fix: `pnpm gen:types`
    (= `cargo test --workspace export_bindings`), then commit the result.
  - `e2e` job (windows-latest): the WebdriverIO smoke suite, on pull requests
    and `workflow_dispatch` only. It has never run on a GitHub runner — see
    [testing.md](testing.md) — so it is not a release gate yet.
  - `audit` job (ubuntu): the one job with no event filter, so it is also
    what the weekly Monday schedule runs. It gates **both** halves of the
    dependency graph against published advisories. npm side: `pnpm audit`
    unfiltered as an informational report, then two gates — moderate+ in
    production dependencies, high/critical anywhere. Rust side: `cargo deny
    check advisories` against `deny.toml` at the repo root, printed in full
    first and then gated on vulnerabilities and unsoundness only, with
    unmaintained-crate advisories left as warnings. `cargo-deny` rather than
    `cargo-audit` because it resolves the real build graph through `cargo
    metadata` instead of reading `Cargo.lock`, which is a feature/target union
    — 46 crate names in this lock file are never compiled on any target, and
    all three vulnerabilities `cargo audit` reports today are among them.
    `deny.toml` carries the reasoning and has an empty ignore list.
  - No job in `ci.yml` produces or uploads a build artifact — it is a
    correctness gate, not a release pipeline. The publishing half lives in a
    separate `release.yml`, added by the concurrent release/signing work; its
    steps are documented by whoever owns it, not here.
- **Local build**: `pnpm tauri build` produces both installers above. It has
  been run end-to-end once, on the author's machine, unsigned.

## What a first public release needs

In rough dependency order. Items 1 and 2 are owned by the concurrent
signing/updater work and are listed here only so the sequence is visible.

1. **Code signing** for the Windows installers. Being wired separately.
2. **`tauri-plugin-updater`**, with its signing keypair. Being wired
   separately. The private key must never enter this repository.
3. **A release workflow**: tag push (`v*`) → the same gate CI already runs →
   `pnpm tauri build` on `windows-latest` → attach the MSI and NSIS installers
   to the GitHub Release for that tag. Being added alongside the signing and
   updater work; this document deliberately does not restate its steps, so
   there is one description of them rather than two that can disagree.
4. **A versioning scheme.** SemVer, tied informally to milestones (`0.1.0` =
   M0+M1 complete, `1.0.0` once the app is the author's daily driver with no
   missing core-loop feature). Three files carry the version and must move
   together — `package.json`, the workspace `Cargo.toml`
   (`[workspace.package] version`), and `src-tauri/tauri.conf.json`. All three
   read `0.1.0` today, and this is now enforced rather than remembered:
   `pnpm check:versions` runs in CI on every PR, and the release workflow runs
   the same script with the tag before it builds anything.

   The asymmetry is deliberate. CI asks only "do the three agree", which is a
   question a PR can answer; the tag comparison happens at release time, where
   the tag exists. Catching it on the PR is worth the duplicate step because
   the fix there is an amend, whereas after a tag it is a retag.

   `src-tauri/tauri.conf.json` is the one that matters most: it is what the
   built binary reports and what `latest.json` advertises. If it is the stale
   one, a release publishes with everything green and every installed copy
   ignores it, because the advertised version equals the one already
   installed — a release nobody receives, with nothing red to show for it.
5. ~~**A changelog.**~~ **Done.** [CHANGELOG.md](../CHANGELOG.md) is generated
   from the commit history by `pnpm gen:changelog`. Every commit in the history
   follows Conventional Commits, which is what makes generating it preferable
   to writing one — a hand-maintained changelog is a second description of the
   same work, and the two disagree the first time someone is in a hurry.

   Only `feat`, `fix` and `perf` reach the file, plus anything marked breaking.
   The rest changed nothing for a user, so listing them would bury what did.
   Each release states how many commits it is *not* showing, because "nothing
   else happened" and "the rest was not user-facing" are different claims and
   a reader should not have to guess which one is meant.

   **Regenerate it before tagging.** Nothing enforces freshness, and unlike
   the version check this one cannot be gated yet: commit links are derived
   from `origin`, so a CI runner and an unpushed clone produce different files
   and the gate would fail on unrelated PRs. Setting `repository` in
   `package.json` once the repo has a fixed home makes the output
   environment-independent and the gate safe to add — the script says so at the
   point where someone would try.
6. ~~**A dependency licence review.**~~ **Done.** The 401 unadjudicated crates
   are adjudicated: `deny.toml` carries a curated `[licenses] allow` list and
   `cargo deny check licenses` runs as a required step in the `audit` job.
   Nothing in the graph is copyleft in a way that reaches DevOS — there is no
   GPL or AGPL at any depth. The five MPL-2.0 crates (`cssparser`,
   `cssparser-macros`, `dtoa-short`, `option-ext`, `selectors`) are weak,
   file-level copyleft that reaches only their own files, and they arrive
   unmodified; patching one would oblige us to publish that file's source.

   [THIRD-PARTY-NOTICES.md](../THIRD-PARTY-NOTICES.md) is the attribution file
   a distributed binary needs, regenerated by `pnpm gen:notices`. **Regenerate
   it whenever dependencies change** — the licence gate catches a *new* licence
   arriving, but nothing detects that the notices are merely out of date.

   Two details worth keeping straight when editing that pipeline. Attribution
   follows the binary, not the source: publishing this repository triggers no
   duty, and handing someone an installer triggers all of it. And the npm half
   is derived from a real build's sourcemaps rather than from the dependency
   tree, because Tailwind places build-only tooling (esbuild, lightningcss) in
   `dependencies` — the tree lists 252 packages where 141 actually ship. The
   two fonts are the one obligation that is not about compiled-away code: they
   ship as `.woff2` binaries under OFL-1.1, which forbids selling the fonts
   alone and reserves their names.

## Security reports against a release

Vulnerability reports go through GitHub private security advisories, not the
issue tracker — [SECURITY.md](../SECURITY.md) has the process, the scope, and
the list of already-documented accepted risks. Only the latest release is
supported; fixes ship forward, never as backports to an older tag.

## Non-goals for now

Multi-platform builds (macOS/Linux) are out of scope until after the
Windows daily-driver experience is solid — the kernel is OS-neutral, so
this is additive later, not a blocker today.

**One thing that has to move with that decision**, because it fails silently
otherwise: `deny.toml` sets `[graph] targets = ["x86_64-pc-windows-msvc"]`, so
the advisory gate only considers crates that compile for Windows. Tauri pulls
an entire GTK3/X11 stack into `Cargo.lock` for the Linux target and `xcap`
pulls in `xcb`; today those are correctly out of scope, and two of the three
vulnerabilities currently in the lock file live there. The day a second
platform is supported, add its triple to that list **in the same commit** —
nothing will go red to remind you, because the whole point of that setting is
that it does not.
