# 0006 — Terminal sessions live in the Rust process, not the webview

Status: accepted
Date: 2026-07-18

## Context

The terminal module needs multiple concurrent sessions, split views, and
sessions that survive the user navigating away from the Terminal page —
DevOS is a multi-feature app, not a dedicated terminal emulator, so users
will constantly switch between Terminal, Git, and AI Assistant.

## Decision

Each pty (via `portable-pty`/ConPTY) is owned by `TerminalManager` in the
Rust process, keyed by session id, independent of any webview route.
`xterm.js` instances on the frontend are cached in a module-level registry
(`src/features/terminal/registry.ts`) rather than recreated per mount, so
both the backend pty *and* the frontend terminal widget (with its
scrollback) persist across route changes — only the DOM attachment moves.

## Alternatives considered

- **Spawn/kill the pty per page visit** — simplest, but would kill a
  long-running build or dev server the moment the user checked git status,
  which defeats the point of an integrated terminal in a multi-feature app.
- **Keep the pty alive but recreate the xterm.js instance on remount** —
  keeps the shell alive but loses scrollback and causes a visible
  flash/rebuild on every navigation back to Terminal.

## Consequences

- Session state (`TerminalManager`) and widget state (the registry) are two
  separate caches that must be reconciled: `reconcileSessions()` runs on
  first mount to detect backend sessions whose frontend instance doesn't
  exist (e.g. after a full webview reload) and surfaces them as
  "disconnected" rather than silently orphaning or killing them.
- The pty read loop runs on a dedicated OS thread per session (blocking
  reads don't fit Tokio's async model directly), forwarding into an
  unbounded `mpsc` channel — a pattern worth reusing if another module
  needs to bridge a blocking I/O source into the async event system.
- Memory cost: N live sessions means N OS threads + N xterm.js instances
  retained even when not visible. Not a problem at the scale of a single
  developer's session count; would need revisiting if sessions could
  proliferate unboundedly.
