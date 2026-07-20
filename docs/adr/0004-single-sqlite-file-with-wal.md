# 0004 — Single SQLite file (WAL) as the only persistence layer

Status: accepted
Date: 2026-07-18

## Context

DevOS needs to persist workspaces/projects, settings, background jobs, AI
conversations, and encrypted secrets — with modules that must stay
independently replaceable (no module reaching into another's tables) and a
strict startup-time budget.

## Decision

One SQLite database file per install, WAL journal mode, opened through a
single `SqlitePool` shared via the kernel. Every module owns its own
prefixed tables (`ai_*`, eventually `git_*`/`term_*`) and creates them
itself; no module holds a foreign key into another module's table.

## Alternatives considered

- **Postgres/MySQL, even embedded/managed locally** — massive operational
  overhead (a running server process, connection management) for a
  single-user desktop app with no concurrent multi-writer requirement.
- **Multiple separate SQLite files (one per module)** — would enforce
  module isolation even more strictly, but multiplies connection-pool
  overhead and rules out any future cross-module query without an
  explicit sync step; a single file with a naming convention gets the same
  isolation discipline without the operational cost.
- **A full CQRS split (separate read/write stores)** — unjustified
  complexity until a genuine read/write model divergence appears (project
  indexing, in M2+, is the first candidate — see
  [architecture.md](../architecture.md)).

## Consequences

- Zero ops: no server to start, stop, or configure; the file is the backup
  unit.
- WAL mode gives concurrent-safe reads without blocking the job runner's
  writes.
- Module isolation is a **convention** (table name prefix, no cross-module
  FKs), not something the database enforces — code review is the guard
  rail, documented in [coding-guidelines.md](../coding-guidelines.md).
- A genuinely high-write-volume feature (e.g. full-text/vector indexing at
  scale) may eventually need a dedicated store; `sqlite-vec` is the current
  plan for embeddings specifically because it keeps that data in the same
  file rather than introducing a second system.
