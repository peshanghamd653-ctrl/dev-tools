# 0002 — ts-rs over specta for Rust→TypeScript type generation

Status: accepted
Date: 2026-07-18

## Context

Every IPC payload (DTOs, events) is defined once in Rust and needs a
matching TypeScript type on the frontend, generated automatically so the
two can never silently drift.

## Decision

Use `ts-rs`: derive `#[derive(TS)]` on each DTO, `#[ts(export, export_to =
"...")]` writes a matching `.ts` file. Regenerated via `cargo test
--workspace export_bindings` (aliased as `pnpm gen:types`).

## Alternatives considered

- **specta** (often paired with `tauri-specta`) — more integrated with
  Tauri's command-generation story, can also generate a typed `invoke`
  wrapper. More moving parts and a runtime coupling to Tauri's command
  macros; DevOS already wraps every IPC call by hand in
  `shared/ipc/client.ts` (a deliberate single-choke-point pattern — see
  [coding-guidelines.md](../coding-guidelines.md)), so the extra
  code-generation specta offers there isn't needed.
- **Hand-written TypeScript types** — the default before either tool: fast
  to start, guaranteed to drift the moment a Rust field is renamed.

## Consequences

- Generating bindings is a plain `cargo test` run — no separate CLI tool,
  no build-script coupling.
- Non-`i64`/`u64` numeric types need an explicit `#[ts(type = "number")]`
  override where ts-rs would otherwise map them to `bigint` (discovered
  during M0 when `tsc` rejected `bigint` being passed to `Date()`) — a
  small, known friction point.
- Generated files under `src/shared/ipc/bindings/` are committed (not
  gitignored), so a fresh clone typechecks without needing a Rust build
  first — deliberate, matches "generated files are committed" in
  [architecture.md](../architecture.md).
