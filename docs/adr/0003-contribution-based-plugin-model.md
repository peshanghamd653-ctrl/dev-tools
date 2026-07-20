# 0003 — Contribution-based plugin model instead of sandboxed arbitrary JS

Status: accepted
Date: 2026-07-18

## Context

The original vision calls for a full plugin marketplace with themes and
extensions, comparable to VS Code's model — which lets plugins run
arbitrary JavaScript with DOM access inside the host application.

## Decision

Plugins contribute **declared capabilities** (commands, panels, status
items) rendered by DevOS's own components, and execute logic in a **WASM
sandbox** with capability-gated host functions (planned, M5) — not
arbitrary in-process JavaScript with DOM access. The `Module` trait already
in use by core modules (`core`, `terminal`, `git`, `ai`) is the seed of
this contract, so it is exercised by first-party code from day one instead
of being a separate, untested API.

## Alternatives considered

- **VS Code-style extension host (Node.js + arbitrary JS/DOM access)** —
  the most feature-flexible option, but arbitrary JS running in the same
  webview as the trusted UI cannot be credibly sandboxed: it can read
  other panels' DOM, intercept events, and exfiltrate anything the host
  page can see. It would also drag a full Node.js runtime into the Rust
  host, undermining the "fast, minimal" goal.
- **No plugin system** — simplest, but forecloses the stated goal of
  community extensibility (themes, integrations) entirely.

## Consequences

- Plugin UI is necessarily more constrained than "render anything" — a
  plugin panel looks and behaves like a DevOS panel, not an arbitrary
  embedded webpage. This is treated as a feature (visual consistency), not
  just a limitation.
- The WASM runtime is more engineering work upfront than "just run JS,"
  deferred to M5 rather than blocking earlier milestones.
- The AI tool-calling feature (M2) ended up validating the same shape early
  — declared capability (`ToolDef`), gated execution (`ToolExecutor`
  scoped to one project root), explicit consent (the tools grant) — see
  [plugin-api.md](../plugin-api.md#a-preview-of-the-pattern-ai-tool-calling).
