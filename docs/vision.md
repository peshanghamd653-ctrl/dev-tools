# Vision

## What DevOS is

DevOS is a desktop **developer operating center** — one application that
replaces the daily rotation of terminal, git client, Docker Desktop,
Postman, DBeaver, AI chat, and deployment dashboards. Not another Electron
dashboard bolted onto a browser tab: a fast, keyboard-driven, AI-first
control surface for the entire development loop.

Working name for the aspiration: *"Jarvis for developers."*

## Who it's for

The primary user is the person building this — a full-stack developer who
wants one fast tool instead of a dozen slow ones, is comfortable with
keyboard-driven interfaces (Raycast, Warp, Linear users), and wants AI
woven into the workflow rather than bolted on as a chat sidebar.

## Core philosophy

- **Incredibly fast.** Sub-second cold start, minimal RAM, no jank.
- **Modular.** Every capability is a module behind the same contract; no
  module reaches into another's internals.
- **AI-first.** The assistant has real tools, not just a text box.
- **Keyboard-driven.** Every action reachable without a mouse.
- **Offline-first where possible.** Local Ollama models, local SQLite,
  local terminal — the network is an enhancement, not a dependency.
- **Enterprise quality.** Typed boundaries, tested behavior, honest docs.

## The honest reframing

The originating brief describes a multi-year product: dashboards, Docker,
API client, database browser, deployment integrations across five clouds,
a plugin marketplace, autonomous coding agents. Building all of it before
shipping anything would produce a broad shell of empty screens — the
opposite of the goal.

So DevOS is built in **vertical milestones**. Each milestone is a real
improvement to daily use, not a stub. Planned-but-not-built modules are
visible in the sidebar (disabled, tagged with their milestone) so the
roadmap is honest without faking functionality. See
[feature-roadmap.md](feature-roadmap.md) for the current milestone state.

## Non-goals (for now)

- A general Chrome DevTools replacement inside the app (WebView2 can't do
  this credibly — see [architecture.md](architecture.md#deliberate-deviations)).
- A sandboxed arbitrary-JS plugin runtime (security tradeoff — see
  [plugin-api.md](plugin-api.md)).
- Cross-platform parity from day one. Windows is the primary target during
  M0–M2; the kernel is OS-neutral by design so macOS/Linux support is a
  matter of adding OS-specific adapters (keyring, pty), not a rewrite.
