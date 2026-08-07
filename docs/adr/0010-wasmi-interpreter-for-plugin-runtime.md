# 0010 — Plugin runtime uses the wasmi interpreter, and does not ship in-process

Status: accepted
Date: 2026-08-07
Amended: 2026-08-07 — an adversarial review of `crates/devos-plugin` found
three of the enforced properties recorded below were claimed rather than true.
The **decision is unchanged and reinforced**: the evidence for "do not ship
this in-process" got stronger, not weaker. What changed is the evidence, which
this ADR had stated too confidently. See *Enforced properties* and the new
first entry under *Consequences*.

## Context

[ADR-0003](0003-contribution-based-plugin-model.md) settled the *shape* of the
plugin system — declarative contributions rendered by DevOS components, logic
in a WASM sandbox — and deferred the runtime to M5.
[plugin-api.md](../plugin-api.md) named "Extism or wasmtime + host functions"
without either having been tried. Two things were therefore unestablished:
whether a WASM host builds and runs on this platform and toolchain at all
(Windows 11, `x86_64-pc-windows-msvc`, rustc 1.97.1), and what it costs.

The costs are not hypothetical. This workspace already pays for Tauri and
three tree-sitter grammars; a runtime that doubles a clean build is a tax on
every contributor forever. And the DevOS process holds decrypted secrets: the
AES-256-GCM master key comes out of the OS keystore and the Anthropic API key
is in memory whenever a request is built ([security.md](../security.md)). Any
plugin runtime loaded into that process inherits that blast radius on a
sandbox escape.

A spike was built in `crates/devos-plugin` to answer these by measurement.

## Decision

**Use `wasmi` 1.1** — a pure-Rust WebAssembly *interpreter* — as the plugin
runtime, and **keep the crate out of the application**. `crates/devos-plugin`
is a workspace member so it compiles and is tested in CI, but it is not
registered in `src-tauri/src/lib.rs` `setup()`, implements no `Module`, and no
IPC command reaches it.

Everything measured on this machine, uncontended, warm cargo registry,
clean target directory, debug profile:

| | crates in dep graph | clean debug build | release binary added |
|---|---|---|---|
| baseline (no wasm host) | 1 | 4 s | — (114 KB total) |
| **wasmi 1.1** | **16** | **73 s** | **+2.2 MB** |
| wasmtime 47 | 143 | 693 s | +13.5 MB |
| extism 1.30 | 267 | 683 s | not measured (⊇ wasmtime) |

`cargo build -p devos-plugin` is 73 s clean and `cargo test -p devos-plugin`
is 102 s clean (the extra is the `wat` dev-dependency, which pulls `wast`).

Release figures are stripped, LTO, `codegen-units = 1` — the profile this
workspace actually ships — measured as an equivalent probe binary minus an
empty one: 114 KB baseline, 2.43 MB with wasmi, 14.27 MB with wasmtime. The
wasmtime release build itself took 16 m 42 s; wasmi's took 4 m 01 s.

The wasmtime and extism timings were taken with some CPU contention and should
be read as "roughly 10× wasmi", not as precise figures. That ratio is the
decision-relevant part and it is not close.

### Enforced properties

*Rewritten 2026-08-07. The original list of three is kept verbatim underneath,
because what an ADR got wrong is part of what it is for.*

- **Fuel** bounds work per call — guest instructions, and also the work the
  guest asks the *host* to do: each host call costs 100 fuel flat, plus one
  unit per 64 bytes copied out of guest memory, which is wasmi's own
  `bytes_per_fuel` rate. Fuel is reset per call, so a plugin cannot bank an
  earlier call's leftovers.
- **A linear-memory ceiling** bounds allocation via wasmi's `ResourceLimiter`,
  not via the module's own declared maximum — a hostile module omits that.
- **Table and module-shape ceilings** bound what a module can make the host
  commit before its first instruction: 4 tables of 10,000 elements via
  `ResourceLimiter`, and `EnforcedLimits::strict()` at compile time for
  globals, functions, element and data segments, and average bytes per
  function.
- **A bounded per-call journal** (1,000 entries / 64 KB, details clamped to
  1 KB, reset each call) bounds what the host retains on the plugin's behalf.
- **A wall-clock budget** checked on entry to each host call, which bounds a
  plugin driving the host in a loop and nothing else — fuel is not a clock and
  `Sandbox::call_*` is synchronous, so the general wall-clock bound belongs to
  the caller. This is recorded as a limitation, not a feature.
- **The granted host-function set** is derived from the manifest *before* any
  guest code runs. An ungranted host function is not defined in the linker, so
  a module importing it fails to instantiate. The capability is absent, not
  refused.
- **Per-call approval** is consulted once in the dispatcher, around every
  match arm, keyed off `NEEDS_APPROVAL`. A refusal traps rather than returning
  a sentinel, so being told no cannot be looped on.

<details>
<summary>The original three, as written on 2026-08-07 before the review</summary>

> Three properties are enforced and pinned by tests in `crates/devos-plugin`:
>
> - **Fuel** bounds instructions per call, so a plugin that never returns is
>   stopped rather than hanging DevOS. Fuel is reset per call, so a plugin
>   cannot bank an earlier call's leftovers.
> - **A linear-memory ceiling** bounds allocation via wasmi's
>   `ResourceLimiter`, not via the module's own declared maximum — a hostile
>   module omits that.
> - **The granted host-function set** is derived from the manifest *before* any
>   guest code runs. An ungranted host function is not defined in the linker,
>   so a module importing it fails to instantiate. The capability is absent,
>   not refused.

Two of the three were true as far as they went and incomplete in the same way:
each named the resource the *guest* spends and none named what the guest can
make the *host* spend. "Fuel bounds instructions per call" was accurate and
irrelevant to a plugin whose whole strategy is to call `log` in a loop. "A
linear-memory ceiling bounds allocation" was accurate about linear memory and
silent about tables, which allocate 8 bytes an element at instantiation. The
third was fine; a fourth property that this list did not mention at all —
per-call approval — was the one that was structurally broken.

</details>

Permission design, and its relationship to
[ADR-0005](0005-read-only-tools-first-with-explicit-grant.md), is in
[plugin-api.md](../plugin-api.md#permission-model--proven).

## Alternatives considered

- **wasmtime 47 directly** — the fastest execution by a wide margin (JIT via
  Cranelift) and the reference implementation, actively maintained on a
  monthly release train. It lost on two counts. The first is cost: 143 crates,
  roughly ten times the clean build and six times the shipped binary size,
  paid by every contributor and every user on a workspace that is already slow
  to build, to run plugins that compute a panel's contents. The second matters
  more: a JIT compiles untrusted input
  to native code and executes it, which is a materially larger trusted
  computing base than an interpreter's decode loop, inside a process holding
  the user's API keys. Wasmtime's security record is good and its team takes
  this seriously — this is not a claim that wasmtime is unsafe. It is a claim
  that when the workload is "run a todo tracker's event handler", buying JIT
  throughput with both build time and attack surface is the wrong trade.
- **Extism 1.30** — the option `plugin-api.md` named first, and a genuinely
  nice developer experience: plugin SDKs in a dozen languages, a manifest
  format, host functions, and memory handling already solved. But it is
  wasmtime plus a layer — 267 crates, the largest graph measured — so it
  inherits the JIT surface *and* adds a dependency whose own release cadence
  gates upgrades (extism 1.30, published 2026-06-04, pins `wasmtime ^43`
  while wasmtime is at 47). It also brings a `cbindgen` build-dependency that
  exists to generate a C header this project will never use. Extism solves
  polyglot plugin distribution, which is a problem DevOS does not have yet
  and may never have.
- **Ship the runtime wired into the app now.** Rejected; see Consequences.
- **A separate plugin host process.** The design that actually fixes the
  blast-radius problem, and the likely eventual answer. Not attempted here:
  it is a much larger piece of work (process lifecycle, IPC, crash handling,
  backpressure) and doing it badly is worse than not doing it. Deferred
  rather than dismissed.

## Consequences

- **A sandbox is only as good as its last adversarial review, and this one
  failed its first.** In August 2026 `crates/devos-plugin` was reviewed with
  the specific brief "is the sandbox real". Three findings, all reproduced with
  working exploits rather than inferred:
  1. **Unbounded table allocation (high).** A **146-byte** module declaring
     only tables got the host to commit **976 MB** at instantiation. A table's
     declared minimum is allocated eagerly at 8 bytes an element, and none of
     it runs, so neither fuel nor the memory ceiling was ever consulted.
     Scaling is linear and the store permitted 10,000 tables; the practical
     ceiling was system RAM, i.e. an OOM abort of the process holding the
     user's decrypted API keys. `Config::enforced_limits` was also never set,
     so globals, functions and element/data segments were uncapped on
     untrusted input.
  2. **Fuel bounded instructions, not host work (high).** Host functions
     consumed no fuel of their own; the 64 KB guest-string read cap was per
     call with nothing capping calls; the journal was never bounded and never
     drained. On the default 5M fuel, a loop calling `log` — which requires no
     permissions at all — made **1,666,631** host calls reading **101.7 GB**
     over **52 seconds**, while the crate's own test comment claimed fuel
     stopped a runaway "in milliseconds". A 1/50-fuel run held **2.08 GB** in
     the journal.
  3. **`NEEDS_APPROVAL` was decorative (medium, structurally serious).** The
     constant was referenced nowhere outside `host.rs`; approval happened
     inside the `HttpFetch` arm of the dispatcher's match — the exact shape a
     comment in that file claimed had been avoided, citing ADR-0005's
     `save_memory` update. Not exploitable with one member. The next member
     would have dispatched ungated while reading as gated to every reviewer.

  All three are fixed, each pinned by a test that fails without its fix.
  The instructive part is not the bugs but that **the crate had a green test
  suite asserting the properties it violated** — the tests checked the shape
  of the configuration and the contents of the constant, never that either bit.
  This is the strongest argument in this ADR for not shipping in-process:
  the properties that make an in-process sandbox tolerable are exactly the ones
  that can be confidently documented, plausibly tested, and false.
- **Plugins will be slow relative to native code.** An interpreter is
  typically several times slower than a JIT. This is acceptable for
  contribution-model plugins and is not acceptable for compute. The
  `SandboxLimits` defaults (5M fuel, 16 MB memory, 4 tables of 10,000
  elements, a 1,000-entry / 64 KB journal, a 2 s host-call deadline) encode
  that opinion: anything needing more should be a DevOS job, not a plugin.
- **The sandbox is in-process, and in-process is not isolation.** Fuel,
  memory caps and a narrow host API make a plugin's *intended* behaviour
  bounded. None of them contain a plugin that finds a bug in the interpreter
  or in a host function, and on that path the plugin lands in a process
  holding the decrypted Anthropic API key and full filesystem access as the
  user. Choosing an interpreter shrinks that risk; it does not remove it.
  This is why the crate is not registered with the app.
- **What would have to be true before it ships in-process**, in the order
  that buys the most safety per unit of work:
  1. The remaining five host functions exist and are individually reviewed —
     `db_query` in particular, which must enforce the own-tables prefix
     against a real SQL surface, and where
     [security.md](../security.md#database-query-execution--implemented-m3)
     already records that classifying SQL by keyword is not a guard.
  2. Plugin sources are constrained — first-party or signed only — so
     "malicious plugin" is a supply-chain question rather than an anyone-can
     question.
  3. `audit_log` actually gets written, since the runtime already produces the
     journal it needs and nothing currently persists it. Note the journal is
     now per call and drained by `Sandbox::take_journal()`: whatever writes
     `audit_log` has to take it after each invocation, because the sandbox
     deliberately does not keep a second copy.
  4. **A second adversarial review that finds nothing, and a fuzzer.** The
     first review found three exploitable gaps in code that looked reviewed
     and had passing tests for the properties it broke. One clean review is
     evidence; the first one was not clean, so there is no reason yet to
     believe the fourth finding does not exist.
  5. For genuinely untrusted third-party plugins, an out-of-process host.
     Until that exists, a plugin marketplace open to arbitrary authors should
     not exist either.
- **Path length is a real constraint on Windows and worth recording.** The
  first attempt to build wasmtime and extism failed identically with
  `LNK1104: cannot open file`, which reads like a corrupt toolchain and is
  not. The path
  `…\probe-wasmtime\target\debug\build\wasmtime-internal-component-macro-<hash>\build_script_build-<hash>.exe`
  is 262 characters; `MAX_PATH` is 260, and `link.exe` does not opt into long
  paths. Both build fine from a shorter root. Wasmtime's crate names are long
  enough that this is reachable from an ordinarily deep checkout, so a
  contributor hitting it should be told what it is rather than debugging their
  linker. This cost wasmi nothing — its longest crate name is
  `wasmi_collections`.
- **Fixtures are checked in as `.wat` text**, compiled to wasm at test time by
  the `wat` dev-dependency. A committed `.wasm` would be an opaque blob no
  reviewer can diff; generating fixtures from Rust would require
  `wasm32-unknown-unknown` on every contributor's machine and in CI to prove
  something about the *host*. The tradeoff is that fixtures are hand-written
  WebAssembly rather than output from the toolchain real plugins will use, so
  they exercise the sandbox faithfully and the guest developer experience not
  at all.
- **The guest ABI is unproven.** The spike passes `(ptr, len)` pairs into the
  guest's exported linear memory, which is enough to show the host can read
  guest memory under a bounds check. It does not establish a calling
  convention for returning variable-length data to the guest, and no real
  language toolchain has been pointed at it. That is the next thing to build,
  and it is more work than it looks.
