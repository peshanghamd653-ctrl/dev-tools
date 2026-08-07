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

## Non-goals for now

Multi-platform builds (macOS/Linux) are out of scope until after the
Windows daily-driver experience is solid — the kernel is OS-neutral, so
this is additive later, not a blocker today.
