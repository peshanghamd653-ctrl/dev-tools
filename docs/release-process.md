# Release Process

**Current state: pre-release.** No version has been tagged or published.
This document describes what exists today and what's needed before a first
release — it does not describe a process that is already running.

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
  - Neither job produces or uploads a build artifact yet — this is a
    correctness gate, not a release pipeline.
- **Local build**: `pnpm tauri build` produces an installer via Tauri's
  bundler (`tauri.conf.json` → `bundle.targets: "all"`), but it is unsigned
  and has never been run end-to-end in this project.

## What's needed for a v0.1.0

In rough dependency order:

1. **Code signing** for the Windows installer (self-signed is fine for
   personal use; a real cert if this is ever distributed to others).
2. **`tauri-plugin-updater`** wired up, with a signing keypair for update
   manifests, so the app can update itself — currently not integrated.
3. **A release workflow**: tag push (`v*`) → `cargo test` + `pnpm test` gate
   → `tauri build` → attach the installer to a GitHub Release. The existing
   CI workflow only does the test/lint gate; the build-and-publish half
   doesn't exist yet.
4. **A versioning scheme.** Suggest SemVer tied to milestones informally
   (`0.1.0` = M0+M1 complete, `0.2.0` = M2 complete, `1.0.0` once the app
   is the author's actual daily driver with no missing core-loop feature).
   `package.json`, `Cargo.toml` (workspace), and `tauri.conf.json` versions
   should move together — not automated yet.
5. **A changelog.** None exists. When automation is added, prefer
   generating it from commit messages/PR titles over hand-maintaining one.
6. **A dependency licence review**, needed the moment a binary is handed to
   anyone else and not before. `cargo deny` is already installed by CI, so
   this is a matter of turning on a check that is deliberately off today:
   `cargo deny check licenses` currently reports **401 errors**, because
   cargo-deny's allow-list starts empty and every crate must be adjudicated.
   That is a project in its own right and the wrong thing to bolt onto a
   security gate — see the header comment in `deny.toml`. Two things to
   settle at the same time: the third-party notices file a distributed
   binary needs (most of these crates are MIT/Apache-2.0 with attribution
   requirements), and whether any crate carries a copyleft or unusual
   licence that the bundle cannot accept.

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
