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
| Host work | fuel charged inside host calls: 100 flat per crossing, plus `len / 64` for bytes copied out of guest memory | a plugin spending the *host's* time instead of its own |
| Memory | `ResourceLimiter` ceiling (default 16 MB) | a plugin allocating until the machine swaps |
| Tables | 4 tables × 10,000 elements (`ResourceLimiter`) | a plugin committing host memory at instantiation, before fuel or the memory ceiling apply |
| Module shape | `Config::enforced_limits(EnforcedLimits::strict())` | a module built to attack the compiler rather than to run |
| Audit trail | journal capped per call at 1,000 entries / 64 KB, details clamped to 1 KB, reset each call | a plugin growing an unbounded `Vec` inside the host |
| Wall clock | 2 s budget, checked on entry to each host call | a plugin driving the host in a loop (see the caveat below) |
| Effects | linker populated only from granted permissions | a plugin reaching a capability it did not declare |

The memory ceiling is the host's, not the guest's declared maximum — a
hostile module simply omits that. Fuel does not accumulate across calls, and
neither does the journal.

The table limits exist because a table's declared *minimum* is allocated
eagerly at 8 bytes an element when the module is instantiated. That is host
memory committed before the guest's first instruction, so neither fuel nor
`memory_bytes` is consulted for it. `tables × table_elements × 8` is 320 KB at
the defaults, about 2% of the memory ceiling: the point of the numbers is that
tables cannot be a second, unmetered way to ask for memory. One indirect-call
table is what LLVM and TinyGo emit, so four is headroom rather than a budget.

Reading a `(ptr, len)` string out of guest memory goes through a bounds-checked
accessor plus a 64 KB length cap; an out-of-range pointer is an error, not a
host-side out-of-bounds read and not a panic that takes DevOS down. That cap is
per call, which is why the copy is also charged fuel at wasmi's own rate of 64
bytes per unit — the cap alone bounds one call, not a million of them.

**The wall-clock budget is not a general execution deadline, and cannot be.**
It is checked on entry to a host call, so it bounds a plugin making the host
work; it cannot interrupt a compute loop (fuel does that) and it cannot
interrupt a host function already blocked in a syscall, which is what a real
`http_fetch` will be. `Sandbox::call_*` is synchronous and blocks the calling
thread. A caller that needs a hard wall-clock bound has to own it, by running
the sandbox on a thread it is prepared to abandon. The sandbox promises
termination, not promptness.

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
does for `edit_file`/`write_file`/`run_command`. A refusal — by the allowlist
or by the user — **traps**, ending the invocation, rather than returning a
sentinel the guest can ignore and retry. A refusal that can be looped on is not
free for the host: it costs a string copy, a gate consultation and a journal
entry each time, at three guest instructions apiece. A plugin that wants to
degrade gracefully gets a fresh call with a fresh budget from the app; it does
not get to retry inside one.

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

- **The approval list is consulted once in the dispatcher**, around every match
  arm, before any handler runs — keyed off the `NEEDS_APPROVAL` constant. That
  is precisely the bug the ADR-0005 update describes for `save_memory`.

  This document, and a comment in `host.rs`, claimed that was already true. It
  was not. Until the August 2026 review the check sat *inside* the `HttpFetch`
  arm and `NEEDS_APPROVAL` was referenced nowhere outside `host.rs`, so
  membership decided nothing — the exact shape both texts claimed had been
  avoided. It was not exploitable, because the one member's arm happened to
  gate; the next call added to the list would have read as gated to every
  reviewer and dispatched ungated. The tests asserted the list's *contents*,
  which is a different claim and cannot catch this. The test that holds it up
  now is parameterised over every member of the list and runs a real module for
  each: `every_call_that_needs_approval_is_refused_by_a_deny_all_gate`, with
  `a_deny_all_gate_does_not_refuse_calls_outside_the_list` as its complement.
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
denial with its reason. That journal is what `audit_log` should be fed from.
`audit_log` is now written — AI tool approvals and denials, secret writes,
SQL writes, issue creation and restores all land there — which satisfies
[ADR-0010](adr/0010-wasmi-interpreter-for-plugin-runtime.md)'s third
precondition. **The plugin journal is still not wired to it**, because the
crate is unregistered: whoever registers it has to add `AuditEvent` variants
for plugin calls, which do not exist yet. Registering the runtime without
that would mean the one subsystem the audit log was demanded for is the one
it does not cover.

The journal is **scoped to one call**, like fuel. `Sandbox::take_journal()` is
the app-facing accessor and the next invocation starts empty whether or not
anyone read the last one — the sandbox should not be holding a second copy of
the audit trail. It is bounded twice over, by entry count and by recorded
bytes, and a call that overflows either leaves a single `Truncated { dropped }`
marker rather than more entries, so "nothing else happened" and "a great deal
else happened and we declined to store it" stay distinguishable. Individual
details are clamped to 1 KB with a trailing `…`, because the audit trail
records *that* a call happened and what it targeted, not the payload.

## Not established

Honest list of what this spike does **not** show:

- **The limits above were reviewed adversarially once, in August 2026, and
  three of them were claimed rather than true.** Each was reproduced with a
  working exploit, and each is now fixed and pinned by a test that fails
  without its fix — but the general lesson is the one worth keeping: this
  document described a sandbox stronger than the one that existed, and it did
  so for months, with a passing test suite. What was found:
  - *Unbounded table allocation.* `table_elements` was unset and `tables` sat
    at wasmi's default of 10,000. A table's declared minimum is committed
    eagerly at 8 bytes an element, so a **146-byte** module got the host to
    commit **976 MB** at instantiation — before a guest instruction ran, so
    neither fuel nor the 16 MB memory ceiling was ever consulted. Scaling is
    linear; the practical ceiling was system RAM. `Config::enforced_limits`
    was also unset, so globals, functions, element and data segments were
    uncapped on untrusted input too.
  - *Fuel bounded instructions, not host work.* Host functions registered with
    `func_wrap` consumed no fuel of their own, the 64 KB read cap was per call
    with nothing capping calls, and the journal was a `Vec` that was never
    bounded and never drained. On default limits, a loop calling `log` with a
    64 KB slice made **1,666,631** host calls reading **101.7 GB** over **52
    seconds**, and a 1/50-fuel run held **2.08 GB** in the journal. `log`
    needs no permissions at all.
  - *`NEEDS_APPROVAL` was decorative.* See the permission-model section above.
- **Nothing here has been fuzzed, and one review is one review.** Every one of
  the three findings was in code that had a test suite asserting the property
  it violated. Assume there are more.
- **Nothing bounds the size of the `.wasm` file itself.** `Sandbox::load` takes
  a `&[u8]` and the caller decides where it came from. Passive data segments
  and function bodies are bounded by the module's own byte count, so the module
  byte count is a limit the install flow owes this crate and does not yet
  provide. Two adjacent things *were* checked while fixing the above and are
  already covered, recorded here so nobody re-derives them: wasmi enables
  `MULTI_MEMORY` and `MEMORY64` by default, and both are caught — a memory's
  declared minimum is checked by the `ResourceLimiter` at creation, exactly as
  a table's now is, and the memory *count* is capped at 1 in both the store and
  the enforced limits. Guest recursion depth and value-stack height are bounded
  by wasmi's own defaults (1,000 frames, 1 MB) rather than by anything here.
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
