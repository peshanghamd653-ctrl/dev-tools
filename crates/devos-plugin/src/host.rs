//! Host functions — the only way out of the sandbox.
//!
//! A WASM guest can compute, and nothing else. Every effect it can have on
//! the machine goes through one of the functions named here, so this file is
//! the complete list of what a plugin can do. Adding a variant to [`HostCall`]
//! is the moment to think about permissions; adding a `linker.func_wrap` call
//! anywhere else is a bug.
//!
//! The gating is two-layered, deliberately the same shape as the AI
//! tool-grant model (ADR-0005):
//!
//! 1. **Structural.** [`granted`] derives the host-function set from the
//!    manifest's permissions, and the runtime defines *only* those in the
//!    linker. An ungranted call is not refused at runtime — it is absent, so
//!    a module importing it cannot instantiate at all. This is the plugin
//!    equivalent of "a tool that is never offered is the one thing a prompt
//!    injection cannot reach for".
//! 2. **Per-call approval.** Calls in [`NEEDS_APPROVAL`] additionally block on
//!    the user, every time, with denial on timeout. The list is consulted once
//!    in `runtime::dispatch`, around every match arm, before any handler runs
//!    — that ordering is the fix ADR-0005's update describes for
//!    `save_memory`.
//!
//!    This comment used to claim that ordering was already the case. It was
//!    not: until the 2026-08 review the check lived *inside* the `HttpFetch`
//!    arm and [`NEEDS_APPROVAL`] was referenced nowhere outside this file, so
//!    membership decided nothing. It happened not to be exploitable — the one
//!    member's arm did gate — but the next call added to the list would have
//!    read as gated to every reviewer and dispatched ungated. The test that
//!    now holds this up is parameterised over the list itself
//!    (`every_call_that_needs_approval_is_refused_by_a_deny_all_gate`), so
//!    asserting the list's *contents*, which is all the old tests did, is no
//!    longer mistaken for asserting that the list has an effect.

use crate::manifest::{DbAccess, Permissions};

/// Every host function a plugin can import, under the wasm import module
/// `devos`. This enum is the security boundary's inventory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HostCall {
    /// Append a line to this plugin's own log. No effect outside DevOS.
    Log,
    /// Read a value from the plugin's own key-value namespace.
    KvGet,
    /// Write a value to the plugin's own key-value namespace.
    KvSet,
    /// Read-only SQL, restricted to the plugin's own table prefix.
    DbQuery,
    /// Publish an event under the plugin's own id on the kernel event bus.
    EmitEvent,
    /// Raise a notification, attributed on screen to this plugin.
    Notify,
    /// An outbound HTTPS request to an allowlisted host.
    HttpFetch,
}

impl HostCall {
    /// The wasm import name. `(import "devos" "http_fetch" ...)`.
    pub fn name(self) -> &'static str {
        match self {
            HostCall::Log => "log",
            HostCall::KvGet => "kv_get",
            HostCall::KvSet => "kv_set",
            HostCall::DbQuery => "db_query",
            HostCall::EmitEvent => "emit_event",
            HostCall::Notify => "notify",
            HostCall::HttpFetch => "http_fetch",
        }
    }

    /// What the manifest must declare for this call to be defined at all.
    pub fn requires(self) -> Capability {
        match self {
            // Confined to DevOS's own surfaces and the plugin's own namespace.
            HostCall::Log => Capability::Ambient,
            HostCall::EmitEvent => Capability::Ambient,
            HostCall::Notify => Capability::Ambient,
            HostCall::KvGet | HostCall::KvSet | HostCall::DbQuery => Capability::Db,
            HostCall::HttpFetch => Capability::Net,
        }
    }
}

/// The import module name every DevOS host function lives under.
pub const IMPORT_MODULE: &str = "devos";

/// Calls that block on explicit user approval on **every** invocation.
///
/// Only egress is here, and the reasoning is ADR-0005's own: per-call approval
/// on a side-effect-free or self-scoped operation is friction without a safety
/// benefit, and friction that makes a feature unusable has its own cost. What
/// distinguishes [`HostCall::HttpFetch`] is that its effect leaves the
/// machine. A plugin writing its own key-value namespace can corrupt its own
/// state; a plugin making a request can post the contents of that state to
/// someone else. The allowlist bounds *where* it can go, approval bounds
/// *whether* it goes.
pub const NEEDS_APPROVAL: &[HostCall] = &[HostCall::HttpFetch];

impl HostCall {
    /// Consulted once by `runtime::dispatch`, around every arm, never inside a
    /// handler. Membership here is what gates a call; nothing else does.
    pub fn needs_approval(self) -> bool {
        NEEDS_APPROVAL.contains(&self)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capability {
    /// Always available. Cannot read or move data across a boundary.
    Ambient,
    /// Requires `permissions.db = "own-tables"`.
    Db,
    /// Requires a non-empty `permissions.net` allowlist.
    Net,
}

/// The host functions a manifest's permissions actually grant.
///
/// This is the *whole* structural gate: the runtime defines exactly this set
/// in the linker and nothing else, so the answer to "can this plugin reach the
/// network" is decided here, once, from declared data — not at each call site.
pub fn granted(permissions: &Permissions) -> Vec<HostCall> {
    const ALL: &[HostCall] = &[
        HostCall::Log,
        HostCall::KvGet,
        HostCall::KvSet,
        HostCall::DbQuery,
        HostCall::EmitEvent,
        HostCall::Notify,
        HostCall::HttpFetch,
    ];
    ALL.iter()
        .copied()
        .filter(|call| match call.requires() {
            Capability::Ambient => true,
            Capability::Db => permissions.db == DbAccess::OwnTables,
            Capability::Net => !permissions.net.is_empty(),
        })
        .collect()
}

/// What the user is shown when a call needs approval. Carries the specific
/// thing being asked for (the URL, not just "network access"), because
/// approving a request whose target you cannot see is not approval.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovalRequest {
    pub plugin_id: String,
    pub call: HostCall,
    pub detail: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Deny,
}

/// How the runtime asks the user. The real implementation belongs in the app
/// layer, alongside `src-tauri/src/approvals.rs`; this crate only needs the
/// shape, and deliberately does not depend on Tauri to get it.
pub trait ApprovalGate: Send + Sync {
    fn request(&self, request: &ApprovalRequest) -> Decision;
}

/// The default gate, and the one a caller gets by forgetting to supply one.
///
/// It denies. A host wired up without an approval channel must refuse every
/// call that needs approval rather than falling through to allowing it —
/// the same property `security.md` records for the tool executor ("a
/// gate-less executor refuses every mutating tool").
#[derive(Debug, Default, Clone, Copy)]
pub struct DenyAll;

impl ApprovalGate for DenyAll {
    fn request(&self, _request: &ApprovalRequest) -> Decision {
        Decision::Deny
    }
}

/// A gate that allows everything. Test scaffolding only — it is `#[cfg(test)]`
/// so it cannot be reached from application code by accident.
#[cfg(test)]
#[derive(Debug, Default, Clone, Copy)]
pub struct AllowAll;

#[cfg(test)]
impl ApprovalGate for AllowAll {
    fn request(&self, _request: &ApprovalRequest) -> Decision {
        Decision::Allow
    }
}

/// Is `url` permitted by this allowlist?
///
/// Exact host match on an https URL, nothing else. Subdomains are not implied
/// (`api.acme.com` does not grant `evil.acme.com`), because the manifest
/// author who wanted both can write both, and the user reading the install
/// prompt should see the literal list of hosts that will be contacted.
pub fn net_allows(allowlist: &[String], url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else {
        // Plain http is never allowed: the allowlist would still be honoured,
        // but the traffic would be readable and modifiable in transit.
        return false;
    };
    // Host ends at the first '/', '?' or '#'. A '@' means userinfo was
    // present, which makes "which part is the host" a question with a
    // famously error-prone answer — refuse rather than parse it.
    let host = rest
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    if host.is_empty() || host.contains('@') || host.contains(':') {
        return false;
    }
    allowlist.iter().any(|allowed| allowed == &host)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::PluginManifest;

    fn permissions(toml_body: &str) -> Permissions {
        let src = format!(
            "id = \"acme.todo\"\nname = \"T\"\nversion = \"1\"\nentry = \"p.wasm\"\n{toml_body}"
        );
        PluginManifest::parse(&src)
            .expect("valid manifest")
            .permissions
    }

    #[test]
    fn a_manifest_with_no_permissions_grants_only_ambient_calls() {
        let granted = granted(&permissions(""));
        assert_eq!(
            granted,
            vec![HostCall::Log, HostCall::EmitEvent, HostCall::Notify]
        );
        assert!(!granted.contains(&HostCall::HttpFetch));
        assert!(!granted.contains(&HostCall::DbQuery));
    }

    #[test]
    fn net_permission_is_what_puts_http_fetch_in_the_linker() {
        let without = granted(&permissions(""));
        let with = granted(&permissions("[permissions]\nnet = [\"api.acme.com\"]\n"));
        assert!(!without.contains(&HostCall::HttpFetch));
        assert!(with.contains(&HostCall::HttpFetch));
    }

    #[test]
    fn db_permission_gates_all_three_data_calls_together() {
        let with = granted(&permissions("[permissions]\ndb = \"own-tables\"\n"));
        for call in [HostCall::KvGet, HostCall::KvSet, HostCall::DbQuery] {
            assert!(with.contains(&call), "{call:?} should be granted");
        }
    }

    /// Asserts the list's *contents* only. That membership has any effect is a
    /// separate claim, and one this test cannot make — it was true here for
    /// months while the dispatcher ignored the list entirely. The test that
    /// makes it is `every_call_that_needs_approval_is_refused_by_a_deny_all_gate`
    /// in `lib.rs`, which runs a real module for every member.
    #[test]
    fn only_egress_needs_per_call_approval() {
        assert!(HostCall::HttpFetch.needs_approval());
        for call in [
            HostCall::Log,
            HostCall::KvGet,
            HostCall::KvSet,
            HostCall::DbQuery,
            HostCall::EmitEvent,
            HostCall::Notify,
        ] {
            assert!(!call.needs_approval(), "{call:?} should not need approval");
        }
    }

    #[test]
    fn the_default_gate_denies() {
        let decision = DenyAll.request(&ApprovalRequest {
            plugin_id: "acme.todo".into(),
            call: HostCall::HttpFetch,
            detail: "https://api.acme.com/x".into(),
        });
        assert_eq!(decision, Decision::Deny);
    }

    #[test]
    fn allowlist_matches_exact_https_hosts_only() {
        let list = vec!["api.acme.com".to_string()];
        assert!(net_allows(&list, "https://api.acme.com/v1/things"));
        assert!(net_allows(&list, "https://API.acme.com/v1"));

        // Not matched, each for its own reason.
        assert!(!net_allows(&list, "http://api.acme.com/v1")); // not https
        assert!(!net_allows(&list, "https://evil.example.com/")); // not listed
        assert!(!net_allows(&list, "https://evil.acme.com/")); // sibling subdomain
        assert!(!net_allows(&list, "https://api.acme.com.evil.com/")); // suffix trick
        assert!(!net_allows(&list, "https://api.acme.com@evil.com/")); // userinfo
        assert!(!net_allows(&list, "https://api.acme.com:8443/")); // port
        assert!(!net_allows(&list, "file:///c:/secrets")); // other scheme
        assert!(!net_allows(&[], "https://api.acme.com/")); // empty allowlist
    }
}
