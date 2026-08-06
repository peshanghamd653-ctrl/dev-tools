# Architecture Decision Records

Short records of decisions that were non-obvious, had real tradeoffs, or
are likely to be questioned later ("why didn't we just use X?"). Not every
decision needs one — routine choices that follow an established pattern
don't. Write one when you catch yourself justifying a choice in a PR
description; that justification belongs here instead, where it won't rot
alongside the diff.

## Format

```md
# NNNN — Short title

Status: accepted | superseded by NNNN
Date: YYYY-MM-DD

## Context
What problem forced a decision.

## Decision
What was chosen.

## Alternatives considered
What else was on the table, and why it lost.

## Consequences
What this makes easier, harder, or forecloses.
```

Numbers are sequential and never reused, even if a decision is later
superseded — supersede with a new number and a note in both files.

## Index

| # | Title |
|---|---|
| [0001](0001-shell-out-to-git-cli.md) | Shell out to the git CLI instead of embedding libgit2/gitoxide |
| [0002](0002-ts-rs-over-specta-for-ipc-types.md) | ts-rs over specta for Rust→TypeScript type generation |
| [0003](0003-contribution-based-plugin-model.md) | Contribution-based plugin model instead of sandboxed arbitrary JS |
| [0004](0004-single-sqlite-file-with-wal.md) | Single SQLite file (WAL) as the only persistence layer |
| [0005](0005-read-only-tools-first-with-explicit-grant.md) | AI tool calling ships read-only-first, gated by explicit grant |
| [0006](0006-terminal-sessions-live-in-rust.md) | Terminal sessions live in the Rust process, not the webview |
| [0007](0007-sqlite-only-database-manager-first.md) | Database manager ships SQLite-only behind a driver-shaped abstraction |
| [0008](0008-in-process-watchers-notify-on-transitions.md) | Background watchers run in-process and notify on state transitions |
| [0009](0009-deployments-read-only-no-write-actions.md) | Deployment integration ships read-only, with no write actions |
