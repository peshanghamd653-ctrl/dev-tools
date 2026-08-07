# Contributing

DevOS is an opinionated single-maintainer project. Small fixes are welcome as
PRs directly. For anything larger than a bug fix — a new module, a new IPC
command, a dependency, a change to a security guard — **open an issue first**.
The [roadmap](docs/feature-roadmap.md) and the
[ADRs](docs/adr/) will usually tell you whether an idea has already been
considered and rejected, and why.

Security problems do not go here. See [SECURITY.md](SECURITY.md).

Contributions are accepted under the [MIT licence](LICENSE).

## Setup

See [README.md](README.md#from-source) for prerequisites (Rust stable MSVC,
Node, pnpm, C++ Build Tools, `git` on `PATH`). The app is Windows-only today;
the Rust half of CI runs on `windows-latest`, so a change that only builds on
another OS cannot be verified here.

```sh
pnpm install
pnpm tauri dev
```

## The gate

Everything below must pass before a PR is ready. CI runs the same set, and
`clippy` is `-D warnings`, so a warning is a failure.

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
pnpm typecheck
pnpm lint
pnpm test
pnpm exec vite build
```

Two notes on the last line: CI actually runs `pnpm build`, which is
`tsc && vite build` — the stricter form, and worth running yourself before
pushing. And if you touched `Cargo.toml`, `package.json` or a lockfile, also
run the advisory gate:

```sh
cargo deny check advisories -W unmaintained   # cargo install cargo-deny --locked
cargo deny check licenses
pnpm audit
```

`deny.toml` at the repo root carries the reasoning and has an empty ignore
list. Adding an entry to it needs a justification in the file, not just in the
PR.

## Third-party notices

**If you add or remove a dependency, regenerate the attribution file:**

```sh
pnpm gen:notices    # needs: cargo install cargo-about --locked --features cli
git add THIRD-PARTY-NOTICES.md
```

Most of what DevOS ships is other people's code under licenses that require
their copyright notice to travel with the binary. Publishing source triggers
none of that; shipping an installer triggers all of it.

Unlike the bindings above, **CI does not check that this file is current** —
`cargo deny check licenses` fails only when a genuinely *new* license appears,
which is a different question from whether the notices are stale. Regenerating
is on you. It is cheap: the script builds the frontend with sourcemaps and
reads which packages actually ended up in the bundle, so it stays correct
without anyone maintaining a list by hand.

## Commit messages and the changelog

Commit subjects follow [Conventional Commits](https://www.conventionalcommits.org)
— `type(scope): subject`, with `!` before the colon for a breaking change.
Every commit in the history conforms, and that is load-bearing rather than
decorative: [CHANGELOG.md](CHANGELOG.md) is generated from these subjects by
`pnpm gen:changelog`, so a subject is the sentence a user will eventually read.
Write it as one.

`feat`, `fix` and `perf` appear in the changelog. `docs`, `ci`, `test`,
`chore`, `refactor` and `style` do not — they are counted, not listed, so
choosing the honest type matters more than choosing a flattering one.

It is regenerated before a release rather than on every commit, so there is no
need to run it in a normal PR.

## Generated IPC bindings

Every IPC DTO is defined once in Rust and exported to TypeScript by ts-rs
([ADR-0002](docs/adr/0002-ts-rs-over-specta-for-ipc-types.md)).

**If you change, add, or remove a Rust type that derives `TS`, run
`pnpm gen:types` and commit the result.**

```sh
pnpm gen:types    # = cargo test --workspace export_bindings
git add src/shared/ipc/bindings/
```

CI regenerates the bindings as a side effect of `cargo test --workspace` and
then diffs `src/shared/ipc/bindings/` — **an uncommitted or stale binding fails
the build**, printing the diff and the fix. New, never-committed bindings count
too (the check uses `--intent-to-add`, because a plain `git diff` exits 0 for an
untracked file — which is the drift most worth catching).

One thing the check structurally cannot catch: **deleting** a Rust DTO leaves
its `.ts` file orphaned, and nothing regenerates over it. Delete the binding by
hand in the same commit.

## Testing conventions

From [docs/testing.md](docs/testing.md). These are not style preferences — each
one exists because a mock hid a real bug here:

- **No mocked databases.** Anything touching SQLite uses a real file in a
  `tempfile::tempdir()`. This is how `git restore --staged` failing in a repo
  with no commits was found; a mock would have passed.
- **No mocked subprocesses where a real one is feasible.** The terminal tests
  spawn a real `cmd.exe` through ConPTY and answer the real cursor-position
  probe. HTTP-facing crates test against a hermetic one-shot local TCP server
  and assert on the **raw bytes the server received**, which is how "the API key
  is in a header and not in the request line" became a test rather than a claim.
- **Security behaviour gets a test that fails if the guard is removed.** Not a
  happy-path test. Write it, confirm it goes red without your fix, then fix.
  The path-traversal, wrong-key-decrypt, and `PRAGMA query_only` bypass tests
  are all written this way, and each records that it was verified failing first.
- **Tests live next to the code they test** — `#[cfg(test)] mod tests` in the
  same Rust file, `Foo.test.ts` beside `Foo.ts`. There is no separate test tree.
- **A new module ships its tests with the code that introduces it.** A PR
  adding an IPC command with no domain-layer test behind it is incomplete.

## Code conventions

Read [docs/coding-guidelines.md](docs/coding-guidelines.md) before your first
PR. The load-bearing ones:

- All SQL lives in `repo.rs` (or `ops.rs` for CLI-backed modules). Never inline
  in a Tauri command.
- `#[tauri::command]` functions are thin: validate, call one domain function,
  map the error, maybe emit an event. No business logic.
- `src/shared/ipc/client.ts` is the only place `invoke`, `listen` or `Channel`
  is called.
- Modules never import each other; cross-module effects go through the event
  bus.
- Redaction at the type level, not by convention — if a struct should never
  carry a secret, give it no field that could.
- Comments explain **why**, not what.

Commit messages follow Conventional Commits with a scope, matching the existing
history — `feat(ai):`, `fix(security):`, `perf(web):`, `ci:`.

## Architecture decisions

When a choice is non-obvious, has a real tradeoff, or will be questioned later,
write an ADR in [docs/adr/](docs/adr/) instead of burying the justification in
a PR description. The rule of thumb from
[docs/adr/README.md](docs/adr/README.md): if you catch yourself explaining why
you *didn't* do the obvious thing, that explanation belongs in an ADR.

Use the format in that file, take the next sequential number (numbers are never
reused, even when a decision is superseded), and add a row to the index table in
the same commit.

## Documentation is part of the change

The docs in [docs/](docs/) are credible because they state what is missing —
per-module deferrals, known test gaps, accepted risks. If your change closes a
gap, opens one, or changes a limit, update the relevant doc in the same PR.

Match the existing register: specific, verifiable, and honest about
limitations. Do not replace a stated limitation with marketing language, and do
not tick a roadmap item that only half shipped — say which half.
