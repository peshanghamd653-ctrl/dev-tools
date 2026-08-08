<!--
Anything larger than a bug fix — a new module, a new IPC command, a dependency,
a change to a security guard — should have an issue first. See CONTRIBUTING.md.
-->

## What and why

<!-- One or two lines. The diff says what changed; say why it needed to. -->

Closes #

## The gate

Everything here must pass before review. CI runs the same set, and `clippy` is
`-D warnings`, so a warning is a failure.

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`
- [ ] `pnpm typecheck`
- [ ] `pnpm lint`
- [ ] `pnpm test`
- [ ] `pnpm build` — `tsc && vite build`, which is what CI runs and is stricter
      than `pnpm exec vite build`

## If it applies

- [ ] **Changed, added, or removed a Rust type deriving `TS`** — ran
      `pnpm gen:types` and committed `src/shared/ipc/bindings/`. CI regenerates
      the bindings as a side effect of `cargo test --workspace` and then diffs
      that directory, so a stale or never-committed binding fails the build.
      **Deleted a DTO?** Delete its `.ts` by hand — regenerate-and-diff cannot
      see an orphan.
- [ ] **Added or removed a dependency** — ran `pnpm gen:notices` and committed
      `THIRD-PARTY-NOTICES.md`. Nothing in CI checks that this file is current;
      the licence gate only fires when a genuinely new licence appears, which is
      a different question from whether the notices are stale.
- [ ] **Touched `Cargo.toml`, `package.json`, or a lockfile** — ran
      `cargo deny check advisories -W unmaintained`, `cargo deny check licenses`,
      and `pnpm audit`. A new ignore entry in `deny.toml` carries its
      justification in the file, not in this description.
- [ ] **Closed a gap, opened one, or changed a stated limit** — updated the
      relevant file in `docs/` in this PR. Do not tick a roadmap item that only
      half shipped; say which half.
- [ ] **Caught yourself explaining why you did not do the obvious thing** —
      wrote an ADR in `docs/adr/` with the next sequential number and added its
      row to the index table in the same commit.

## Commits and tests

- [ ] Commit subjects are Conventional Commits with a scope — `feat(ai):`,
      `fix(security):`, `perf(web):`, `ci:`. CHANGELOG.md is generated from
      these subjects, so a subject is a sentence a user will read. `feat`, `fix`
      and `perf` reach the changelog; `docs`, `ci`, `test`, `chore`, `refactor`
      and `style` do not, so the honest type beats the flattering one.
- [ ] New behaviour ships with its tests, in the same file or beside it. No
      mocked databases, and no mocked subprocesses where a real one is feasible.
- [ ] Security-relevant behaviour has a test that fails when the guard is
      removed — confirmed red before the fix, not only green after it.
