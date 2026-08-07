//! DevOS plugin runtime — **feasibility spike, not wired into the app.**
//!
//! This crate answers one question: can a contribution-model plugin
//! ([ADR-0003](../../../docs/adr/0003-contribution-based-plugin-model.md)) run
//! inside a sandbox that actually holds, on this platform and this toolchain?
//! It is deliberately not registered with `src-tauri`, and it implements no
//! `Module`. Loading third-party code into the process that holds the user's
//! decrypted API keys is a separate decision, and the reasoning for not taking
//! it here is recorded in
//! [ADR-0010](../../../docs/adr/0010-wasmi-interpreter-for-plugin-runtime.md).
//!
//! What the crate contains:
//!
//! - [`manifest`] — `devos-plugin.toml`, parsed strictly. The declared surface.
//! - [`host`] — the complete inventory of host functions and the permission
//!   rules that decide which ones exist for a given plugin.
//! - [`runtime`] — the wasmi sandbox: fuel that covers host work as well as
//!   guest instructions, a memory ceiling, table and module-shape ceilings, a
//!   bounded per-call journal, and a linker populated only from granted
//!   permissions.
//!
//! An adversarial review in 2026-08 found three of those properties were
//! claimed rather than true (SEC-101 unbounded table allocation, SEC-102 fuel
//! not covering host work, SEC-103 an approval list nothing consulted). Each
//! is now pinned by a test that fails without its fix, and the tests below are
//! grouped so it is obvious which claim each one holds up.

pub mod host;
pub mod manifest;
pub mod runtime;

pub use host::{
    granted, net_allows, ApprovalGate, ApprovalRequest, Capability, Decision, DenyAll, HostCall,
    IMPORT_MODULE, NEEDS_APPROVAL,
};
pub use manifest::{
    CommandContribution, Contributions, DbAccess, ManifestError, PanelContribution, Permissions,
    PluginManifest,
};
pub use runtime::{
    deny_all, DenyReason, HostState, HostWork, JournalEntry, PluginError, Sandbox, SandboxLimits,
    IMPLEMENTED,
};

#[cfg(test)]
mod sandbox_tests {
    use std::sync::Arc;
    use std::time::Duration;

    use super::*;
    use crate::host::AllowAll;

    // Fixtures are checked in as WAT text and compiled here, at test time.
    //
    // The alternative — committing a built `.wasm` — would put an opaque
    // binary in the repository that no reviewer can diff and that nobody can
    // regenerate without the exact toolchain that produced it. Building the
    // fixtures from Rust source instead would need `wasm32-unknown-unknown`
    // installed on every contributor's machine and in CI, to prove something
    // about the *host* rather than about the guest toolchain. WAT costs
    // neither and is readable in a pull request.
    const ADDER: &str = include_str!("../fixtures/adder.wat");
    const SPIN: &str = include_str!("../fixtures/spin.wat");
    const HOG: &str = include_str!("../fixtures/hog.wat");
    const NET: &str = include_str!("../fixtures/net.wat");
    const EVIL_NET: &str = include_str!("../fixtures/evil_net.wat");
    const LOGGER: &str = include_str!("../fixtures/logger.wat");
    // The hostile fixtures the 2026-08 review's exploits were reduced to.
    const TABLE_HOG: &str = include_str!("../fixtures/table_hog.wat");
    const MANY_TABLES: &str = include_str!("../fixtures/many_tables.wat");
    const ONE_TABLE: &str = include_str!("../fixtures/one_table.wat");
    const FLOOD: &str = include_str!("../fixtures/flood.wat");
    const DENY_LOOP: &str = include_str!("../fixtures/deny_loop.wat");

    const NET_ONLY: &str = "[permissions]\nnet = [\"api.acme.com\"]\n";
    /// Everything a manifest can grant. Used where the point of the test is
    /// what happens *after* the structural gate, so the structural gate must
    /// not be what stops it.
    const ALL_PERMISSIONS: &str = "[permissions]\ndb = \"own-tables\"\nnet = [\"api.acme.com\"]\n";

    fn wasm(fixture: &str) -> Vec<u8> {
        wat::parse_str(fixture).expect("fixture is valid WAT")
    }

    fn manifest(permissions: &str) -> PluginManifest {
        PluginManifest::parse(&format!(
            "id = \"acme.todo\"\nname = \"Todo\"\nversion = \"0.1.0\"\nentry = \"plugin.wasm\"\n{permissions}"
        ))
        .expect("test manifest is valid")
    }

    fn load(
        permissions: &str,
        fixture: &str,
        gate: Arc<dyn ApprovalGate>,
    ) -> Result<Sandbox, PluginError> {
        load_with(permissions, fixture, gate, SandboxLimits::default())
    }

    fn load_with(
        permissions: &str,
        fixture: &str,
        gate: Arc<dyn ApprovalGate>,
        limits: SandboxLimits,
    ) -> Result<Sandbox, PluginError> {
        Sandbox::load(&manifest(permissions), &wasm(fixture), gate, limits)
    }

    // ---- it runs at all -------------------------------------------------

    #[test]
    fn a_plugin_computes_and_returns_a_value() {
        let mut plugin = load("", ADDER, deny_all()).expect("adder loads");
        assert_eq!(plugin.call_i32_2("add", 2, 3).unwrap(), 5);
    }

    #[test]
    fn calling_a_function_the_plugin_does_not_export_is_an_error() {
        let mut plugin = load("", ADDER, deny_all()).expect("adder loads");
        assert!(matches!(
            plugin.call_i32("subtract"),
            Err(PluginError::MissingExport { .. })
        ));
    }

    #[test]
    fn a_module_that_is_not_wasm_is_refused_before_anything_runs() {
        let result = Sandbox::load(
            &manifest(""),
            b"this is not a wasm module",
            deny_all(),
            SandboxLimits::default(),
        );
        assert!(matches!(result, Err(PluginError::Compile { .. })));
    }

    // ---- the sandbox holds ----------------------------------------------

    #[test]
    fn a_plugin_that_never_returns_is_stopped_by_fuel() {
        // Without this, `spin` hangs the calling thread forever. With it, the
        // call returns an error in milliseconds.
        let mut plugin = load("", SPIN, deny_all()).expect("spin loads");
        assert!(matches!(
            plugin.call_void("spin"),
            Err(PluginError::OutOfFuel { .. })
        ));
    }

    #[test]
    fn fuel_is_restored_per_call_and_not_carried_over() {
        // A budget large enough for one call many times over, but far too
        // small for 200 calls' worth of accumulated spend. If `begin_call`
        // ever stops running, this fails.
        let mut plugin = load_with(
            "",
            ADDER,
            deny_all(),
            SandboxLimits {
                fuel: 1_000,
                ..SandboxLimits::default()
            },
        )
        .expect("adder loads");
        for i in 0..200 {
            assert_eq!(plugin.call_i32_2("add", i, 1).unwrap(), i + 1);
        }
    }

    #[test]
    fn a_plugin_cannot_allocate_past_its_memory_ceiling() {
        const CAP: usize = 1024 * 1024;
        let mut plugin = load_with(
            "",
            HOG,
            deny_all(),
            SandboxLimits {
                memory_bytes: CAP,
                ..SandboxLimits::default()
            },
        )
        .expect("hog loads");

        // The guest asks for memory in a loop until it is told no. `-1` is
        // wasm's "grow failed" sentinel: the refusal reached the guest.
        assert_eq!(plugin.call_i32("hog").unwrap(), -1);
        assert!(
            plugin.memory_bytes().unwrap() <= CAP,
            "guest grew to {:?}, ceiling was {CAP}",
            plugin.memory_bytes()
        );
    }

    // ---- SEC-101: allocation the guest never has to execute for -----------
    //
    // A table's declared minimum is committed at instantiation, 8 bytes an
    // element, before the guest's first instruction. Fuel never sees it and
    // `memory_bytes` does not cover it, so these limits are the only thing
    // between a two-hundred-byte file and an OOM abort of the process holding
    // the user's decrypted API keys.

    #[test]
    fn a_module_declaring_a_giant_table_is_refused_before_it_allocates() {
        // 4,194,304 elements — 32 MiB, twice the default memory ceiling, from
        // a module with nothing to run. Before the fix `table_elements` was
        // unlimited and this loaded happily.
        let error = load("", TABLE_HOG, deny_all()).expect_err("must not instantiate");
        assert!(
            matches!(error, PluginError::Instantiate { .. }),
            "got {error:?}"
        );
    }

    #[test]
    fn a_module_declaring_more_tables_than_the_ceiling_is_refused() {
        // Capping one table's elements is not enough while a module may
        // declare 10,000 tables, which was wasmi's default. Eight tiny tables
        // against a ceiling of four: the refusal must come from the count.
        let limits = SandboxLimits::default();
        assert!(limits.tables < 8, "fixture must exceed the ceiling");
        let error = load("", MANY_TABLES, deny_all()).expect_err("must not instantiate");
        assert!(
            matches!(error, PluginError::Instantiate { .. }),
            "got {error:?}"
        );
    }

    #[test]
    fn an_ordinary_indirect_call_table_still_loads_and_runs() {
        // The other half of the fix: a ceiling tight enough to break the
        // single indirect-call table every real toolchain emits would be a
        // ceiling that breaks every plugin.
        let mut plugin = load("", ONE_TABLE, deny_all()).expect("a normal function table loads");
        assert_eq!(plugin.call_i32_2("add_indirect", 2, 3).unwrap(), 5);
    }

    #[test]
    fn the_eager_table_budget_is_a_small_fraction_of_the_memory_ceiling() {
        // The property the numbers were chosen for, rather than the numbers
        // themselves: tables must not be a second, unmetered way to ask the
        // host for memory. 8 bytes per element is wasmi's `UntypedVal`.
        let limits = SandboxLimits::default();
        let table_bytes = limits.tables * limits.table_elements * 8;
        assert!(
            table_bytes * 20 <= limits.memory_bytes,
            "tables may commit {table_bytes} bytes against a {} byte memory ceiling",
            limits.memory_bytes
        );
    }

    #[test]
    fn module_shape_is_bounded_at_compile_time_not_only_at_instantiation() {
        // `Config::enforced_limits` was never set, so globals, functions,
        // element and data segments and average bytes per function were all
        // unbounded on untrusted input. A module is input to the compiler
        // before it is input to the interpreter. Globals stand in for the set.
        let mut source = String::from("(module\n");
        for _ in 0..1_001 {
            source.push_str("(global i32 (i32.const 0))\n");
        }
        source.push_str("(func (export \"run\") (result i32) (i32.const 0)))");

        let error = load("", &source, deny_all()).expect_err("must be refused while compiling");
        assert!(
            matches!(&error, PluginError::Compile { reason, .. } if reason.contains("global")),
            "got {error:?}"
        );
    }

    // ---- SEC-102: fuel has to cover what the host is made to do -----------

    #[test]
    fn a_host_call_charges_fuel_for_the_work_it_is_asked_to_do() {
        // Host functions registered with `func_wrap` consume no fuel of their
        // own. Measured before the fix, on 50x this budget: 1,666,631 calls,
        // 101.7 GiB read, 52 seconds — while the sandbox claimed a runaway was
        // stopped "in milliseconds".
        //
        // `deadline` is off so this test is about fuel and nothing else.
        const FUEL: u64 = 100_000;
        let mut plugin = load_with(
            "",
            FLOOD,
            deny_all(),
            SandboxLimits {
                fuel: FUEL,
                deadline: None,
                ..SandboxLimits::default()
            },
        )
        .expect("flood loads under an empty manifest — `log` is ambient");

        let error = plugin.call_void("flood_big").expect_err("must be stopped");
        assert!(
            matches!(error, PluginError::OutOfFuel { .. }),
            "got {error:?}"
        );

        let work = plugin.host_work();
        // wasmi copies 64 bytes per unit of fuel; a host call may not beat the
        // engine's own rate for the same work.
        assert!(
            work.bytes_read <= FUEL * 64,
            "the host copied {} bytes on a {FUEL} fuel budget",
            work.bytes_read
        );
        // And the flat per-crossing charge bounds the call count on its own,
        // so a zero-length argument is not a free boundary crossing.
        assert!(
            work.calls <= FUEL / 100,
            "the host was called {} times on a {FUEL} fuel budget",
            work.calls
        );
    }

    #[test]
    fn the_journal_stops_growing_at_its_entry_cap_and_records_what_it_dropped() {
        // `HostState.journal` was a `Vec` that was never bounded and never
        // drained. A 1/50-fuel run held 2.08 GiB in it.
        const ENTRIES: usize = 50;
        let mut plugin = load_with(
            "",
            FLOOD,
            deny_all(),
            SandboxLimits {
                journal_entries: ENTRIES,
                deadline: None,
                ..SandboxLimits::default()
            },
        )
        .expect("flood loads");

        assert!(matches!(
            plugin.call_void("flood_small"),
            Err(PluginError::OutOfFuel { .. })
        ));

        let journal = plugin.journal();
        assert_eq!(
            journal.len(),
            ENTRIES + 1,
            "the cap plus exactly one truncation marker, however many calls were made"
        );
        let Some(JournalEntry::Truncated { dropped }) = journal.last() else {
            panic!(
                "the last entry should be the marker, got {:?}",
                journal.last()
            );
        };
        // The marker is not decoration: "nothing else happened" and "a great
        // deal else happened and we declined to store it" must be different
        // readings, and the count must reconcile with the work done.
        assert!(
            *dropped > 0,
            "the plugin made far more calls than {ENTRIES}"
        );
        assert_eq!(
            plugin.host_work().calls,
            ENTRIES as u64 + dropped,
            "every dispatched call is either recorded or counted as dropped"
        );
    }

    #[test]
    fn the_journal_stops_growing_at_its_byte_cap() {
        // The entry cap alone is worked around by making each entry huge: the
        // 64 KiB read cap is per call, so 1,000 entries is 64 MiB.
        const BYTES: usize = 8 * 1024;
        let mut plugin = load_with(
            "",
            FLOOD,
            deny_all(),
            SandboxLimits {
                fuel: 200_000,
                journal_bytes: BYTES,
                deadline: None,
                ..SandboxLimits::default()
            },
        )
        .expect("flood loads");

        assert!(matches!(
            plugin.call_void("flood_big"),
            Err(PluginError::OutOfFuel { .. })
        ));

        let journal = plugin.journal();
        let held: usize = journal
            .iter()
            .map(|entry| match entry {
                JournalEntry::Log(detail)
                | JournalEntry::Allowed { detail, .. }
                | JournalEntry::Denied { detail, .. } => detail.len(),
                JournalEntry::Truncated { .. } => 0,
            })
            .sum();
        assert!(
            held <= BYTES,
            "the journal holds {held} bytes, cap was {BYTES}"
        );
        assert!(
            journal
                .iter()
                .any(|entry| matches!(entry, JournalEntry::Truncated { .. })),
            "hitting the byte cap must be recorded, not silent"
        );
        assert!(
            journal.len() < SandboxLimits::default().journal_entries,
            "the byte cap should be what bit here, not the entry cap"
        );

        // Each argument is clamped before it is stored, so one call cannot put
        // 64 KiB into the audit trail even when the journal has room.
        let Some(JournalEntry::Log(text)) = journal.first() else {
            panic!("expected a log entry first, got {:?}", journal.first());
        };
        assert!(
            text.len() < 64 * 1024,
            "a 64 KiB argument was stored whole ({} bytes)",
            text.len()
        );
        assert!(text.ends_with('…'), "a clamped detail must look clamped");
    }

    #[test]
    fn the_journal_belongs_to_one_call_and_does_not_accumulate_across_them() {
        // The journal's consumer is the app, which takes it after each
        // invocation. Retaining it across calls was what turned a per-call
        // cap into no cap at all, and it is now scoped exactly like fuel.
        let mut plugin = load("", LOGGER, deny_all()).expect("logger loads");

        // Three calls, nothing taken in between. A caller that never collects
        // the journal must still not be able to make the sandbox hold three
        // calls' worth of it — that is what "per call" has to mean, and the
        // caller forgetting is exactly the case that used to grow unbounded.
        for _ in 0..3 {
            plugin.call_void("run").unwrap();
        }
        assert_eq!(
            plugin.journal(),
            [JournalEntry::Log("hello from plugin".into())],
            "the journal accumulated across calls"
        );

        // And taking it — what the app does, since the audit trail belongs to
        // whatever writes `audit_log` — leaves the sandbox holding nothing.
        assert_eq!(plugin.take_journal().len(), 1);
        assert!(plugin.journal().is_empty(), "taking it leaves it empty");
    }

    #[test]
    fn a_plugin_cannot_loop_on_being_refused() {
        // A refusal that returns a sentinel can be retried, so being told no
        // costs the host a string copy, a gate consultation and a journal
        // entry, and costs the guest three instructions. Refusal has to end
        // the call.
        let mut plugin =
            load(NET_ONLY, DENY_LOOP, Arc::new(AllowAll)).expect("deny_loop links with net");

        let error = plugin
            .call_i32("run")
            .expect_err("the refusal must end the call");
        assert!(
            matches!(&error, PluginError::Trap { reason, .. } if reason.contains("refused")),
            "got {error:?}"
        );
        assert_eq!(
            plugin.journal(),
            [JournalEntry::Denied {
                call: HostCall::HttpFetch,
                detail: "https://evil.example.com/collect".into(),
                reason: DenyReason::NotAllowlisted,
            }]
        );
        assert_eq!(
            plugin.host_work().calls,
            1,
            "exactly one attempt should have reached the host"
        );
    }

    #[test]
    fn a_host_call_past_the_wall_clock_budget_is_stopped() {
        // Fuel cannot provide a clock: an interpreter executes a fuel budget
        // in whatever wall time the machine takes. This bounds only the case
        // of a plugin driving the host in a loop — see `Sandbox::call_void`
        // for why the general wall-clock bound belongs to the caller.
        let mut plugin = load_with(
            "",
            LOGGER,
            deny_all(),
            SandboxLimits {
                deadline: Some(Duration::ZERO),
                ..SandboxLimits::default()
            },
        )
        .expect("logger loads");

        let error = plugin.call_void("run").expect_err("must be stopped");
        assert!(
            matches!(error, PluginError::Deadline { .. }),
            "got {error:?}"
        );
        assert!(
            plugin.journal().is_empty(),
            "the call was stopped before it could have an effect"
        );

        // And a budget a plugin is not abusing does not interfere.
        let mut plugin = load_with(
            "",
            LOGGER,
            deny_all(),
            SandboxLimits {
                deadline: Some(Duration::from_secs(60)),
                ..SandboxLimits::default()
            },
        )
        .expect("logger loads");
        plugin
            .call_void("run")
            .expect("a normal call is unaffected");
    }

    // ---- the permission model -------------------------------------------

    #[test]
    fn an_ungranted_host_function_is_absent_not_merely_refused() {
        // The headline property. `net.wat` imports `devos.http_fetch`; the
        // manifest grants no `net`, so the host never defines it and linking
        // fails. There is no code path in which this plugin reaches the
        // network, because there is no instance of this plugin at all.
        let error = load("", NET, Arc::new(AllowAll)).expect_err("must not instantiate");
        let PluginError::Instantiate { reason, .. } = &error else {
            panic!("expected an instantiation failure, got {error:?}");
        };
        assert!(
            reason.contains("http_fetch"),
            "the failure should name the missing import: {reason}"
        );
    }

    #[test]
    fn granting_net_is_what_makes_the_module_link() {
        // Same bytes, same fixture — only the manifest changed.
        let plugin = load(NET_ONLY, NET, deny_all());
        assert!(plugin.is_ok(), "granted net should link: {plugin:?}");
    }

    #[test]
    fn a_call_outside_the_allowlist_is_denied_without_asking_the_user() {
        // The gate would say yes to anything. The allowlist check runs first,
        // so the user is never prompted for a host the manifest never declared.
        let mut plugin = load(NET_ONLY, EVIL_NET, Arc::new(AllowAll))
            .expect("evil_net links once net is granted");

        let error = plugin.call_i32("run").expect_err("must not succeed");
        assert!(
            matches!(&error, PluginError::Trap { reason, .. } if reason.contains("allowlist")),
            "got {error:?}"
        );
        assert_eq!(
            plugin.journal(),
            [JournalEntry::Denied {
                call: HostCall::HttpFetch,
                detail: "https://evil.example.com/collect".into(),
                reason: DenyReason::NotAllowlisted,
            }]
        );
    }

    #[test]
    fn an_allowlisted_call_still_needs_per_call_approval() {
        let mut plugin = load(NET_ONLY, NET, deny_all()).expect("net links");

        let error = plugin.call_i32("run").expect_err("must not succeed");
        assert!(
            matches!(&error, PluginError::Trap { reason, .. } if reason.contains("denied")),
            "got {error:?}"
        );
        assert_eq!(
            plugin.journal(),
            [JournalEntry::Denied {
                call: HostCall::HttpFetch,
                detail: "https://api.acme.com/v1/things".into(),
                reason: DenyReason::UserDenied,
            }]
        );
    }

    #[test]
    fn an_approved_allowlisted_call_is_permitted_and_recorded() {
        let mut plugin = load(NET_ONLY, NET, Arc::new(AllowAll)).expect("net links");

        assert_eq!(plugin.call_i32("run").unwrap(), 0);
        assert_eq!(
            plugin.journal(),
            [JournalEntry::Allowed {
                call: HostCall::HttpFetch,
                detail: "https://api.acme.com/v1/things".into(),
            }]
        );
    }

    #[test]
    fn approval_is_asked_for_every_call_not_once_per_session() {
        // A standing "yes" would leave later calls with an empty journal. The
        // journal is now per call, so the evidence has to be collected the way
        // the app collects it — by taking it after each invocation.
        let mut plugin = load(NET_ONLY, NET, Arc::new(AllowAll)).expect("net links");
        let mut taken = Vec::new();
        for _ in 0..3 {
            plugin.call_i32("run").unwrap();
            taken.extend(plugin.take_journal());
        }
        assert_eq!(taken.len(), 3);
        assert!(taken
            .iter()
            .all(|entry| matches!(entry, JournalEntry::Allowed { .. })));
    }

    // ---- SEC-103: the approval list has to decide something ---------------
    //
    // `NEEDS_APPROVAL` was referenced nowhere outside `host.rs`. The approval
    // check sat inside the dispatcher's `HttpFetch` arm — the exact shape the
    // code comment claimed had been avoided. Not exploitable with one member,
    // but the next call added to the list would have read as gated to every
    // reviewer and dispatched ungated. The old tests asserted the list's
    // contents and could not have caught it; these run a real module for every
    // member.

    /// The argument every generated module passes. Allowlisted, so the scope
    /// check — which runs before approval, deliberately — is never what
    /// refuses these calls, and approval is the only thing left that can.
    const APPROVAL_URL: &str = "https://api.acme.com/x";

    /// A module importing exactly `call` and invoking it once.
    fn importer(call: HostCall) -> String {
        // `log` is the one call whose settled signature returns nothing.
        let (signature, body) = if call == HostCall::Log {
            (
                "(param i32 i32)",
                format!(
                    "(call $f (i32.const 0) (i32.const {})) (i32.const 0)",
                    APPROVAL_URL.len()
                ),
            )
        } else {
            (
                "(param i32 i32) (result i32)",
                format!("(call $f (i32.const 0) (i32.const {}))", APPROVAL_URL.len()),
            )
        };
        format!(
            "(module\n  (import \"devos\" \"{name}\" (func $f {signature}))\n  \
             (memory (export \"memory\") 1)\n  (data (i32.const 0) \"{APPROVAL_URL}\")\n  \
             (func (export \"run\") (result i32) {body}))",
            name = call.name(),
        )
    }

    #[test]
    fn every_call_that_needs_approval_is_refused_by_a_deny_all_gate() {
        assert!(
            !NEEDS_APPROVAL.is_empty(),
            "a vacuous parameterisation proves nothing"
        );
        for &call in NEEDS_APPROVAL {
            let mut plugin = load(ALL_PERMISSIONS, &importer(call), deny_all())
                .unwrap_or_else(|e| panic!("{call:?} should link under full permissions: {e:?}"));

            let error = match plugin.call_i32("run") {
                Err(error) => error,
                Ok(value) => panic!("{call:?} returned {value} instead of being refused"),
            };
            assert!(
                matches!(&error, PluginError::Trap { reason, .. } if reason.contains("denied")),
                "{call:?}: expected a refusal, got {error:?}"
            );
            assert_eq!(
                plugin.journal(),
                [JournalEntry::Denied {
                    call,
                    detail: APPROVAL_URL.into(),
                    reason: DenyReason::UserDenied,
                }],
                "{call:?} must leave exactly one denial and no effect"
            );
        }
    }

    #[test]
    fn a_deny_all_gate_does_not_refuse_calls_outside_the_list() {
        // The complement, and the thing that keeps the test above honest:
        // membership has to be what decides. A non-member under the same gate
        // must not be refused for user denial — whatever else happens to it.
        let all = granted(&manifest(ALL_PERMISSIONS).permissions);
        let outside: Vec<HostCall> = all.into_iter().filter(|c| !c.needs_approval()).collect();
        assert!(!outside.is_empty());

        for call in outside {
            let mut plugin = load(ALL_PERMISSIONS, &importer(call), deny_all())
                .unwrap_or_else(|e| panic!("{call:?} should link under full permissions: {e:?}"));
            let _ = plugin.call_i32("run");
            assert!(
                !plugin.journal().iter().any(|entry| matches!(
                    entry,
                    JournalEntry::Denied {
                        reason: DenyReason::UserDenied,
                        ..
                    }
                )),
                "{call:?} is not in NEEDS_APPROVAL but the gate refused it: {:?}",
                plugin.journal()
            );
        }
    }

    // ---- the host/guest boundary ----------------------------------------

    #[test]
    fn the_host_reads_a_string_out_of_guest_memory() {
        let mut plugin = load("", LOGGER, deny_all()).expect("logger loads");
        plugin.call_void("run").unwrap();
        assert_eq!(
            plugin.journal(),
            [JournalEntry::Log("hello from plugin".into())]
        );
    }

    #[test]
    fn an_out_of_bounds_pointer_from_the_guest_is_an_error_not_a_host_read() {
        // The guest hands the host (ptr, len) spanning past the end of its
        // single page. This must be a trap, not a host-side out-of-bounds
        // read and not a panic that takes DevOS down with it.
        let mut plugin = load("", LOGGER, deny_all()).expect("logger loads");
        let error = plugin
            .call_void("run_out_of_bounds")
            .expect_err("must not succeed");
        assert!(
            matches!(&error, PluginError::Trap { reason, .. } if reason.contains("rejected")),
            "expected a rejected host call, got {error:?}"
        );
        assert!(plugin.journal().is_empty(), "nothing should be logged");
    }

    // ---- what this spike does not do ------------------------------------

    #[test]
    fn the_spike_implements_only_the_two_host_functions_it_claims() {
        // This test exists so the gap between the designed host API and the
        // built one is recorded somewhere that fails when it changes, rather
        // than only in prose that quietly goes stale.
        assert_eq!(IMPLEMENTED, [HostCall::Log, HostCall::HttpFetch]);
    }

    #[test]
    fn an_unimplemented_host_function_fails_loudly_when_called() {
        // `kv_get` is granted by `db = "own-tables"` and designed, but this
        // build has no implementation. It must trap, not silently return 0.
        let source = r#"(module
          (import "devos" "kv_get" (func $kv_get (param i32 i32) (result i32)))
          (memory (export "memory") 1)
          (func (export "run") (result i32) (call $kv_get (i32.const 0) (i32.const 0))))"#;
        let mut plugin = load("[permissions]\ndb = \"own-tables\"\n", source, deny_all())
            .expect("kv_get links when db is granted");

        let error = plugin.call_i32("run").expect_err("must not succeed");
        assert!(
            matches!(&error, PluginError::Trap { reason, .. } if reason.contains("not implemented")),
            "got {error:?}"
        );
    }
}
