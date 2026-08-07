# Plugin API

**Status: spike. Nothing here is reachable from the running application.**
`crates/devos-plugin` compiles and is tested in CI, but it is not registered
in `src-tauri/src/lib.rs`, implements no `Module`, and no IPC command reaches
it. Each section below says plainly whether it is *proven* (built, tested,
measured) or *designed* (written down, not built). See
[ADR-0010](adr/0010-wasmi-interpreter-for-plugin-runtime.md) for the runtime
choice and the measurements behind it.

## Design position

Core modules and plugins share one contract. The `Module` trait in
`devos-kernel` (`id()` + `register(ctx)`) is deliberately the seed of the
plugin API: everything a core module can contribute (commands, events, jobs,
tables) is exactly what a plugin will contribute. That keeps the plugin
surface honest — it is exercised by first-party code (`core`, `terminal`,
`git`, `ai`) every day, not a separate, aspirational API.

UI integration is **declarative**: plugins contribute commands, panels, and
status items rendered by DevOS components. Plugins do not inject arbitrary
DOM/JS into the app — in-process JS cannot be sandboxed credibly, and UI
consistency is a product feature. See [ADR-0003](adr/0003-contribution-based-plugin-model.md).

## Manifest — proven

A plugin ships a `devos-plugin.toml`. This is parsed and validated by
`devos_plugin::manifest`; the example below is the one the parser's tests run
against, so the document and the code cannot drift apart silently.

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

Validation rules, each with a test:

- **An omitted permission grants nothing.** A manifest with no
  `[permissions]` block gets `db = "none"` and an empty `net`. Under-specified
  is the *least* privileged state, never the most.
- **Unknown keys are a parse error**, not ignored. `net_all = true` silently
  dropped would read as granted to whoever wrote it.
- **A plugin contributes only under its own id.** `acme.todo` cannot
  contribute a command called `git.commit` and shadow a core command in the
  palette.
- **The table prefix is derived, never declared.** `acme.todo` owns
  `plugin_acme_todo_*`. Because every derived prefix starts with `plugin_`, no
  plugin can name a core module's prefix (`ai_`, `index_`, `api_`, `db_`,
  `monitor*`).
- **`entry` must be a bare `.wasm` filename** — no separators, no `..`. Same
  instinct as `src-tauri/src/pathsafe.rs`: refuse the shape rather than
  canonicalize and hope.
- **`net` entries are bare hostnames.** No schemes, no paths, no ports, no
  wildcards, no userinfo. The allowlist check then compares a host to a host,
  with no ambiguity about what `https://api.acme.com@evil.example.com/` means.
- **`fs` must be empty.** Filesystem access is not part of this design;
  requesting it is a loud rejection rather than a silently ignored key.

## Runtime — proven

`wasmi` 1.1, a pure-Rust interpreter. Chosen over wasmtime and Extism on
build cost and trusted-computing-base size; see
[ADR-0010](adr/0010-wasmi-interpreter-for-plugin-runtime.md) for the numbers.

Enforced today, each pinned by a test in `crates/devos-plugin`:

| Limit | Mechanism | What it stops |
|---|---|---|
| CPU | fuel, reset per call | a plugin that never returns hanging DevOS |
| Memory | `ResourceLimiter` ceiling (default 16 MB) | a plugin allocating until the machine swaps |
| Effects | linker populated only from granted permissions | a plugin reaching a capability it did not declare |

The memory ceiling is the host's, not the guest's declared maximum — a
hostile module simply omits that. Fuel does not accumulate across calls.

Reading a `(ptr, len)` string out of guest memory goes through a bounds-checked
accessor plus a length cap; an out-of-range pointer is an error, not a
host-side out-of-bounds read and not a panic that takes DevOS down.

## Permission model — proven

Two layers, deliberately the same shape as the AI tool-grant model in
[ADR-0005](adr/0005-read-only-tools-first-with-explicit-grant.md).

**1. Structural, from the manifest.** Permissions decide which host functions
are *defined in the linker at all*. An ungranted host function is not refused
at runtime — it is absent, so a module importing it cannot instantiate. This
is the plugin equivalent of the property `security.md` records for the write
grant: "a tool that is never offered is the one thing a prompt injection
cannot reach for". The test is
`an_ungranted_host_function_is_absent_not_merely_refused`.

**2. Per-call approval, for egress only.** `http_fetch` blocks on the user
every time, with denial on timeout, exactly as `src-tauri/src/approvals.rs`
does for `edit_file`/`write_file`/`run_command`.

Why only egress needs the per-call dialog is ADR-0005's own argument, applied
to a different surface. That ADR declined to approve every read because
friction without a corresponding safety benefit makes a feature unusable,
which has its own cost. The same reasoning puts `kv_set` and `emit_event` on
the standing install-time grant: they are confined to the plugin's own
namespace, so the blast radius is the plugin's own state. What distinguishes
`http_fetch` is that its effect *leaves the machine*. A plugin that corrupts
its own key-value store has harmed itself; a plugin that makes a request can
post the contents of that store to someone else. The allowlist bounds where it
can go; approval bounds whether it goes.

Two ordering properties, both tested, both learned from ADR-0005's update:

- **The approval list is checked before dispatch**, keyed off an explicit
  `NEEDS_APPROVAL` constant — not inside one match arm. That is precisely the
  bug the ADR-0005 update describes for `save_memory`, fixed here before it
  could happen.
- **The allowlist is checked before the user is asked.** A prompt for a host
  the manifest never declared is a prompt the user should never see; asking
  trains them to approve, and hands them a decision that was not theirs.

The default `ApprovalGate` is `DenyAll`. A host wired up without an approval
channel refuses every call needing approval rather than falling through to
allowing it.

## Host functions — designed; two of seven built

`log` and `http_fetch` are implemented. The other five are designed and
permission-gated but trap if called. The gap is asserted by a test
(`the_spike_implements_only_the_two_host_functions_it_claims`) so it fails
when it stops being true, rather than living in prose that goes stale.

| Host function | Requires | Approval | Built |
|---|---|---|---|
| `log` | — | standing | yes |
| `emit_event` | — | standing | no |
| `notify` | — | standing | no |
| `kv_get` | `db` | standing | no |
| `kv_set` | `db` | standing | no |
| `db_query` | `db` | standing | no |
| `http_fetch` | `net` | **per call** | yes |

`http_fetch` is gated end to end (allowlist, then approval, then journal) but
performs no actual request — the spike records that the call was permitted
rather than opening a socket, which keeps the tests hermetic without changing
anything about the gating.

Every call crossing the boundary is recorded in a journal, including every
denial with its reason. That journal is what `audit_log` should be fed from;
nothing writes to `audit_log` yet.

## Not established

Honest list of what this spike does **not** show:

- **In-process is not isolation.** The limits above bound a plugin's intended
  behaviour. None of them contain a plugin that finds a bug in the interpreter
  or in a host function — and on that path it is inside the process holding
  the decrypted Anthropic API key. This is why the crate is not registered
  with the app, and why a marketplace open to arbitrary authors needs an
  out-of-process host first. See ADR-0010's Consequences for the ordered list
  of what would have to be true.
- **The guest ABI.** `(ptr, len)` into guest memory works. There is no
  convention yet for returning variable-length data *to* the guest, which is
  the harder half.
- **No real toolchain has been pointed at it.** Fixtures are hand-written WAT
  (readable in a diff, no `wasm32-unknown-unknown` needed in CI). They
  exercise the host faithfully and the plugin-author experience not at all.
  Whether a Rust or TinyGo plugin can be built against this API without a
  WASI shim is untested — and WASI is a liability here, not a feature, since
  it is a filesystem and clock API that would sit outside the manifest's
  permission model.
- **Panels, commands, and the frontend.** The manifest parses contributions;
  nothing renders them.
- **Install, update, signing, and removal.** Entirely unbuilt.

## A preview of the pattern: AI tool calling

The M2 AI tool-calling feature (`crates/devos-ai::ToolExecutor`,
`src-tauri/src/tools.rs::ProjectTools`) is a working small-scale version of
this shape, worth studying before extending the WASM runtime:

- Tools are declared data (`ToolDef { name, description, input_schema }`),
  not code the model can inject.
- Execution is capability-gated: `ProjectTools` can only read within one
  canonicalized project root; traversal is structurally rejected.
- Nothing runs without an **explicit user grant** — the tools list sent to
  the model is empty unless the user has turned the grant on.

The plugin runtime generalizes this: manifest-declared capabilities instead
of a hardcoded tool set, WASM instead of in-process Rust, but the shape —
declared surface, gated execution, explicit consent — is the same.

## Phasing

| Phase | Capability |
|---|---|
| M0–M2 | `Module` trait in-tree; core modules use it; AI tool-calling previews the capability-gated pattern |
| M3–M4 | Every new module keeps the contract honest |
| **M5 (now)** | **Runtime spike: manifest, sandbox, permission model — proven, not shipped** |
| M5+ | Remaining host functions, guest ABI, SDK (`packages/plugin-sdk`), install flow |
| Later | Out-of-process host; only then a marketplace open to third-party authors |

Non-goal: a Node.js plugin host (VS Code model). It would drag an entire JS
runtime into the Rust host and break the security story.
