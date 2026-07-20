# 0001 — Shell out to the git CLI instead of embedding libgit2/gitoxide

Status: accepted
Date: 2026-07-18

## Context

DevOS needs git status, staging, commits, branches, diffs, and push/pull.
The obvious "native" choices are `libgit2` (via `git2-rs`) or `gitoxide` —
both give an in-process, dependency-free git implementation.

## Decision

Shell out to the user's own `git` executable, parsing `--porcelain=v2`
output for status and structured `--pretty=format:` output for log. Wrapped
in `devos-git::cli::run_git()`.

## Alternatives considered

- **libgit2 (`git2-rs`)** — mature, widely used, but has known edge-case
  divergence from real git: credential helper behavior, custom hooks,
  LFS, and some config interpretation differ from what the user's terminal
  does. A git client that behaves subtly differently from the user's own
  git is worse than a slower one that behaves identically.
- **gitoxide** — younger, pure-Rust, promising for hot paths, but not yet a
  drop-in replacement for the full surface DevOS needs (hooks, credential
  helpers, LFS).

## Consequences

- Credentials, hooks, LFS, and config all behave exactly as the user's
  terminal git does — no divergence to debug or explain.
- Every git operation pays process-spawn overhead (small, but non-zero) and
  depends on `git` being on PATH — acceptable since a developer's machine
  always has git installed.
- Output parsing (porcelain v2) is DevOS's own responsibility and is
  tested directly (`devos-git::ops::tests`) rather than trusted to a
  library's abstraction.
- `gitoxide` remains an option later **for specific hot paths only** (e.g.
  status/diff on very large repos) behind the same module interface,
  without touching credential/hook/LFS-sensitive operations.
