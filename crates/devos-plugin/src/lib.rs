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
//! - [`runtime`] — the wasmi sandbox: fuel, a memory ceiling, and a linker
//!   populated only from granted permissions.

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
    deny_all, DenyReason, HostState, JournalEntry, PluginError, Sandbox, SandboxLimits, IMPLEMENTED,
};

#[cfg(test)]
mod sandbox_tests {
    use std::sync::Arc;

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
        Sandbox::load(
            &manifest(permissions),
            &wasm(fixture),
            gate,
            SandboxLimits::default(),
        )
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
        // small for 200 calls' worth of accumulated spend. If `refuel` ever
        // stops running, this fails.
        let mut plugin = Sandbox::load(
            &manifest(""),
            &wasm(ADDER),
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
        let mut plugin = Sandbox::load(
            &manifest(""),
            &wasm(HOG),
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
        let plugin = load("[permissions]\nnet = [\"api.acme.com\"]\n", NET, deny_all());
        assert!(plugin.is_ok(), "granted net should link: {plugin:?}");
    }

    #[test]
    fn a_call_outside_the_allowlist_is_denied_without_asking_the_user() {
        // The gate would say yes to anything. The allowlist check runs first,
        // so the user is never prompted for a host the manifest never declared.
        let mut plugin = load(
            "[permissions]\nnet = [\"api.acme.com\"]\n",
            EVIL_NET,
            Arc::new(AllowAll),
        )
        .expect("evil_net links once net is granted");

        assert_eq!(plugin.call_i32("run").unwrap(), -1);
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
        let mut plugin =
            load("[permissions]\nnet = [\"api.acme.com\"]\n", NET, deny_all()).expect("net links");

        assert_eq!(plugin.call_i32("run").unwrap(), -1);
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
        let mut plugin = load(
            "[permissions]\nnet = [\"api.acme.com\"]\n",
            NET,
            Arc::new(AllowAll),
        )
        .expect("net links");

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
        // A standing "yes" would show one journal entry for three calls.
        let mut plugin = load(
            "[permissions]\nnet = [\"api.acme.com\"]\n",
            NET,
            Arc::new(AllowAll),
        )
        .expect("net links");
        for _ in 0..3 {
            plugin.call_i32("run").unwrap();
        }
        assert_eq!(plugin.journal().len(), 3);
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
        let mut plugin = Sandbox::load(
            &manifest("[permissions]\ndb = \"own-tables\"\n"),
            &wasm(source),
            deny_all(),
            SandboxLimits::default(),
        )
        .expect("kv_get links when db is granted");

        let error = plugin.call_i32("run").expect_err("must not succeed");
        assert!(
            matches!(&error, PluginError::Trap { reason, .. } if reason.contains("not implemented")),
            "got {error:?}"
        );
    }
}
