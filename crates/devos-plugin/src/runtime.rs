//! The sandbox itself: compile a plugin's wasm, give it exactly the host
//! functions its manifest earned, cap what it can spend, and run it.
//!
//! Three limits apply to every call, and all three are the host's, not the
//! guest's:
//!
//! - **Fuel** bounds instructions executed, so a plugin that never returns is
//!   stopped rather than hanging DevOS.
//! - **A linear-memory ceiling** bounds allocation, enforced through wasmi's
//!   `ResourceLimiter` rather than the module's own declared maximum — a
//!   hostile module simply omits that maximum.
//! - **The granted host-function set** bounds effects, and is derived from the
//!   manifest before any guest code runs.

use std::sync::Arc;

use wasmi::{
    Caller, Config, Engine, Extern, Instance, Linker, Memory, Module, Store, StoreLimits,
    StoreLimitsBuilder,
};

use crate::host::{
    granted, net_allows, ApprovalGate, ApprovalRequest, Decision, DenyAll, HostCall, IMPORT_MODULE,
};
use crate::manifest::{ManifestError, PluginManifest};

#[derive(Debug, thiserror::Error)]
pub enum PluginError {
    #[error(transparent)]
    Manifest(#[from] ManifestError),
    #[error("plugin {id}: invalid wasm: {reason}")]
    Compile { id: String, reason: String },
    /// Includes the case this crate cares most about: the module imports a
    /// host function its manifest did not grant, so linking cannot complete.
    #[error("plugin {id} could not be instantiated: {reason}")]
    Instantiate { id: String, reason: String },
    #[error("plugin {id} exports no function {name:?} with the expected signature")]
    MissingExport { id: String, name: String },
    #[error("plugin {id} exceeded its execution budget and was stopped")]
    OutOfFuel { id: String },
    #[error("plugin {id} trapped: {reason}")]
    Trap { id: String, reason: String },
}

pub type PluginResult<T> = Result<T, PluginError>;

/// Per-call resource budget.
///
/// The defaults are deliberately small. A contribution-model plugin computes a
/// panel's contents or handles a command; it is not a workload. Anything that
/// needs more than this should be a DevOS job, not a plugin.
#[derive(Debug, Clone, Copy)]
pub struct SandboxLimits {
    /// Fuel granted per call. Reset before every invocation, so a plugin
    /// cannot bank an earlier call's leftovers.
    pub fuel: u64,
    /// Hard ceiling on the guest's linear memory, in bytes.
    pub memory_bytes: usize,
}

impl Default for SandboxLimits {
    fn default() -> Self {
        Self {
            fuel: 5_000_000,
            memory_bytes: 16 * 1024 * 1024,
        }
    }
}

/// One thing the plugin did that crossed the sandbox boundary.
///
/// This is the audit trail. In the app it becomes `audit_log` rows; here it
/// makes the denial paths observable to tests, which is the point — a
/// refusal that leaves no trace is indistinguishable from a call that never
/// happened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JournalEntry {
    Log(String),
    Allowed {
        call: HostCall,
        detail: String,
    },
    Denied {
        call: HostCall,
        detail: String,
        reason: DenyReason,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DenyReason {
    /// The target was outside the manifest's `net` allowlist.
    NotAllowlisted,
    /// The user said no, or did not answer.
    UserDenied,
    /// Designed and permission-gated, but not implemented by this spike.
    NotImplemented,
}

/// Host functions this spike actually implements. The rest are designed and
/// permission-gated (see [`crate::host::HostCall`]) but trap if called; the
/// gap is asserted in a test rather than described in a comment, so it cannot
/// quietly stop being true.
pub const IMPLEMENTED: &[HostCall] = &[HostCall::Log, HostCall::HttpFetch];

/// State the host functions share. Owned by the `Store`, so it is per-instance
/// and cannot be reached by another plugin.
pub struct HostState {
    plugin_id: String,
    allowlist: Vec<String>,
    gate: Arc<dyn ApprovalGate>,
    limits: StoreLimits,
    journal: Vec<JournalEntry>,
}

impl HostState {
    pub fn journal(&self) -> &[JournalEntry] {
        &self.journal
    }
}

pub struct Sandbox {
    id: String,
    store: Store<HostState>,
    instance: Instance,
    fuel: u64,
}

/// Hand-written because a `Store` is not `Debug`, and because the guest's
/// linear memory has no business being formatted into a log line.
impl std::fmt::Debug for Sandbox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Sandbox")
            .field("id", &self.id)
            .field("fuel_per_call", &self.fuel)
            .finish_non_exhaustive()
    }
}

impl Sandbox {
    /// Compile and instantiate `wasm` under `manifest`'s permissions.
    ///
    /// Fails if the module imports anything the manifest did not grant. That
    /// failure is the structural half of the permission model: an ungranted
    /// host function is not defined, so the import cannot be resolved.
    pub fn load(
        manifest: &PluginManifest,
        wasm: &[u8],
        gate: Arc<dyn ApprovalGate>,
        limits: SandboxLimits,
    ) -> PluginResult<Self> {
        let mut config = Config::default();
        config.consume_fuel(true);
        let engine = Engine::new(&config);

        let module = Module::new(&engine, wasm).map_err(|e| PluginError::Compile {
            id: manifest.id.clone(),
            reason: e.to_string(),
        })?;

        let state = HostState {
            plugin_id: manifest.id.clone(),
            allowlist: manifest.permissions.net.clone(),
            gate,
            limits: StoreLimitsBuilder::new()
                .memory_size(limits.memory_bytes)
                .memories(1)
                .build(),
            journal: Vec::new(),
        };
        let mut store = Store::new(&engine, state);
        store.limiter(|state| &mut state.limits);
        store
            .set_fuel(limits.fuel)
            .map_err(|e| PluginError::Instantiate {
                id: manifest.id.clone(),
                reason: e.to_string(),
            })?;

        let mut linker = Linker::new(&engine);
        define_granted(&mut linker, manifest)?;

        let instance = linker
            .instantiate_and_start(&mut store, &module)
            .map_err(|e| PluginError::Instantiate {
                id: manifest.id.clone(),
                reason: e.to_string(),
            })?;

        Ok(Self {
            id: manifest.id.clone(),
            store,
            instance,
            fuel: limits.fuel,
        })
    }

    /// Call an exported `fn() -> ()`.
    pub fn call_void(&mut self, name: &str) -> PluginResult<()> {
        self.refuel()?;
        let func = self
            .instance
            .get_typed_func::<(), ()>(&self.store, name)
            .map_err(|_| PluginError::MissingExport {
                id: self.id.clone(),
                name: name.to_string(),
            })?;
        func.call(&mut self.store, ()).map_err(|e| self.classify(e))
    }

    /// Call an exported `fn() -> i32`.
    pub fn call_i32(&mut self, name: &str) -> PluginResult<i32> {
        self.refuel()?;
        let func = self
            .instance
            .get_typed_func::<(), i32>(&self.store, name)
            .map_err(|_| PluginError::MissingExport {
                id: self.id.clone(),
                name: name.to_string(),
            })?;
        func.call(&mut self.store, ()).map_err(|e| self.classify(e))
    }

    /// Call an exported `fn(i32, i32) -> i32`.
    pub fn call_i32_2(&mut self, name: &str, a: i32, b: i32) -> PluginResult<i32> {
        self.refuel()?;
        let func = self
            .instance
            .get_typed_func::<(i32, i32), i32>(&self.store, name)
            .map_err(|_| PluginError::MissingExport {
                id: self.id.clone(),
                name: name.to_string(),
            })?;
        func.call(&mut self.store, (a, b))
            .map_err(|e| self.classify(e))
    }

    /// What the plugin did across the boundary, in order.
    pub fn journal(&self) -> &[JournalEntry] {
        self.store.data().journal()
    }

    /// The guest's current linear memory size in bytes, or `None` if it
    /// exports no memory. Used to check the ceiling actually held.
    pub fn memory_bytes(&self) -> Option<usize> {
        let memory = self
            .instance
            .get_export(&self.store, "memory")
            .and_then(Extern::into_memory)?;
        Some(memory.size(&self.store) as usize * 64 * 1024)
    }

    /// Every call starts from a full tank; leftovers do not accumulate.
    fn refuel(&mut self) -> PluginResult<()> {
        self.store
            .set_fuel(self.fuel)
            .map_err(|e| PluginError::Trap {
                id: self.id.clone(),
                reason: e.to_string(),
            })
    }

    fn classify(&self, error: wasmi::Error) -> PluginError {
        if error.as_trap_code() == Some(wasmi::TrapCode::OutOfFuel) {
            return PluginError::OutOfFuel {
                id: self.id.clone(),
            };
        }
        PluginError::Trap {
            id: self.id.clone(),
            reason: error.to_string(),
        }
    }
}

/// Define exactly the host functions the manifest grants — the structural gate.
///
/// Note what is *not* here: any per-call permission check for whether a
/// capability was granted. That question is settled by which arms of this
/// match run at all.
fn define_granted(linker: &mut Linker<HostState>, manifest: &PluginManifest) -> PluginResult<()> {
    let fail = |e: wasmi::errors::LinkerError| PluginError::Instantiate {
        id: manifest.id.clone(),
        reason: e.to_string(),
    };

    for call in granted(&manifest.permissions) {
        match call {
            HostCall::Log => {
                linker
                    .func_wrap(
                        IMPORT_MODULE,
                        call.name(),
                        |mut caller: Caller<'_, HostState>, ptr: i32, len: i32| {
                            let text = read_guest_string(&caller, ptr, len)?;
                            caller.data_mut().journal.push(JournalEntry::Log(text));
                            Ok(())
                        },
                    )
                    .map_err(fail)?;
            }
            HostCall::HttpFetch => {
                linker
                    .func_wrap(
                        IMPORT_MODULE,
                        call.name(),
                        |mut caller: Caller<'_, HostState>,
                         ptr: i32,
                         len: i32|
                         -> Result<i32, wasmi::Error> {
                            let url = read_guest_string(&caller, ptr, len)?;
                            Ok(http_fetch(caller.data_mut(), url))
                        },
                    )
                    .map_err(fail)?;
            }
            // Designed, permission-gated, not implemented by this spike.
            // Defined anyway so that a plugin importing them still links —
            // and then fails loudly at the call, in the journal, rather than
            // failing confusingly at instantiation.
            //
            // The `(i32, i32) -> i32` shape here is a placeholder, not a
            // commitment: these calls have no agreed ABI yet, and a guest
            // importing one with a different signature will still fail to
            // link. Settling the ABI is the work this stub is standing in for.
            other => {
                linker
                    .func_wrap(
                        IMPORT_MODULE,
                        other.name(),
                        move |mut caller: Caller<'_, HostState>,
                              _a: i32,
                              _b: i32|
                              -> Result<i32, wasmi::Error> {
                            caller.data_mut().journal.push(JournalEntry::Denied {
                                call: other,
                                detail: String::new(),
                                reason: DenyReason::NotImplemented,
                            });
                            Err(wasmi::Error::new(format!(
                                "devos.{} is not implemented in this build",
                                other.name()
                            )))
                        },
                    )
                    .map_err(fail)?;
            }
        }
    }
    Ok(())
}

/// The egress path, and the only host function here with a per-call gate.
///
/// Order matters and is the point: the allowlist is checked **before** the
/// user is asked. A prompt for a host the manifest never declared is a prompt
/// the user should never see — asking would train them to approve, and a
/// user who approves a request DevOS already knows is out of scope has been
/// handed a decision that was not theirs to make.
fn http_fetch(state: &mut HostState, url: String) -> i32 {
    const DENIED: i32 = -1;
    const OK: i32 = 0;

    if !net_allows(&state.allowlist, &url) {
        state.journal.push(JournalEntry::Denied {
            call: HostCall::HttpFetch,
            detail: url,
            reason: DenyReason::NotAllowlisted,
        });
        return DENIED;
    }

    let decision = state.gate.request(&ApprovalRequest {
        plugin_id: state.plugin_id.clone(),
        call: HostCall::HttpFetch,
        detail: url.clone(),
    });
    if decision == Decision::Deny {
        state.journal.push(JournalEntry::Denied {
            call: HostCall::HttpFetch,
            detail: url,
            reason: DenyReason::UserDenied,
        });
        return DENIED;
    }

    // The spike stops here: it records that the request was permitted rather
    // than performing it. Nothing about the gating changes when a real client
    // is attached, and leaving the socket out keeps these tests hermetic.
    state.journal.push(JournalEntry::Allowed {
        call: HostCall::HttpFetch,
        detail: url,
    });
    OK
}

/// Read a `(ptr, len)` string out of the guest's linear memory.
///
/// `Memory::read` is bounds-checked against the guest's *current* memory, so a
/// pointer past the end is an error here rather than a host-side out-of-bounds
/// read. The length cap is separate: it stops a guest from asking the host to
/// materialise a 4 GiB `String` on its behalf, which is a host-memory
/// exhaustion the guest's own ceiling would not cover.
fn read_guest_string(
    caller: &Caller<'_, HostState>,
    ptr: i32,
    len: i32,
) -> Result<String, wasmi::Error> {
    const MAX_LEN: i32 = 64 * 1024;

    if !(0..=MAX_LEN).contains(&len) || ptr < 0 {
        return Err(wasmi::Error::new(format!(
            "host call rejected: (ptr {ptr}, len {len}) is out of range"
        )));
    }
    let memory: Memory = caller
        .get_export("memory")
        .and_then(Extern::into_memory)
        .ok_or_else(|| wasmi::Error::new("plugin exports no linear memory named `memory`"))?;

    let mut buffer = vec![0u8; len as usize];
    memory
        .read(caller, ptr as usize, &mut buffer)
        .map_err(|e| wasmi::Error::new(format!("host call rejected: {e}")))?;

    String::from_utf8(buffer).map_err(|_| wasmi::Error::new("host call rejected: invalid UTF-8"))
}

/// Convenience for callers that have no approval channel wired up yet.
pub fn deny_all() -> Arc<dyn ApprovalGate> {
    Arc::new(DenyAll)
}
