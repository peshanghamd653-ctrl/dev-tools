# Plugin API

## Design position

Core modules and plugins share one contract. The `Module` trait in
`devos-kernel` (`id()` + `register(ctx)`) is deliberately the seed of the
plugin API: everything a core module can contribute (commands, events, jobs,
tables) is exactly what a plugin will contribute. That keeps the plugin
surface honest — it is exercised by first-party code (`core`, `terminal`,
`git`, `ai`) every day, not a separate, aspirational API.

## Contribution model (declarative)

A plugin ships a manifest:

```toml
# devos-plugin.toml
id = "acme.todo"
name = "Todo Tracker"
version = "0.1.0"
entry = "plugin.wasm"

[contributes]
commands = [{ id = "acme.todo.add", title = "Add Todo" }]
panels   = [{ id = "acme.todo.panel", title = "Todos", icon = "check" }]

[permissions]
db = "own-tables"        # only its own prefixed tables
net = ["api.acme.com"]   # explicit allowlist
fs = []                  # none
```

UI integration is **declarative**: plugins contribute commands, panels, and
status items rendered by DevOS components. Plugins do not inject arbitrary
DOM/JS into the app — in-process JS cannot be sandboxed credibly, and UI
consistency is a product feature. See [ADR-0003](adr/0003-contribution-based-plugin-model.md).

## Runtime (planned, M5)

- Plugin logic compiles to **WASM** (any language), executed via a WASI
  runtime (Extism or wasmtime + host functions).
- Host functions expose a capability-gated API: `db_query` (own tables),
  `emit_event`, `http_fetch` (allowlisted hosts), `kv_get/set`, `notify`.
- Permissions are declared in the manifest, shown at install time, enforced
  at the host-function boundary, and recorded in `audit_log`.
- Panels get data via the same event/query mechanism the core UI uses.

## A preview of the pattern: AI tool calling

The M2 AI tool-calling feature (`crates/devos-ai::ToolExecutor`,
`src-tauri/src/tools.rs::ProjectTools`) is a working small-scale version of
this shape, worth studying before building the WASM runtime:

- Tools are declared data (`ToolDef { name, description, input_schema }`),
  not code the model can inject.
- Execution is capability-gated: `ProjectTools` can only read within one
  canonicalized project root; traversal is structurally rejected.
- Nothing runs without an **explicit user grant** — the tools list sent to
  the model is empty unless the user has turned the grant on for that
  conversation.

The plugin runtime generalizes this: manifest-declared capabilities instead
of a hardcoded tool set, WASM instead of in-process Rust, but the shape —
declared surface, gated execution, explicit consent — is the same.

## Phasing

| Phase | Capability |
|---|---|
| M0–M2 (now) | `Module` trait in-tree; core modules use it; AI tool-calling previews the capability-gated pattern |
| M3–M4 | Every new module keeps the contract honest |
| M5 | WASM runtime, manifest loader, permission prompts, SDK (`packages/plugin-sdk`), marketplace scaffold |

Non-goal: a Node.js plugin host (VS Code model). It would drag an entire JS
runtime into the Rust host and break the security story.
