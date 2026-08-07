//! The sandbox itself: compile a plugin's wasm, give it exactly the host
//! functions its manifest earned, cap what it can spend, and run it.
//!
//! Four limits apply to every call, and all four are the host's, not the
//! guest's:
//!
//! - **Fuel** bounds work executed. It bounds guest instructions, and — since
//!   the 2026-08 review — it also bounds the work the guest asks the *host* to
//!   do on its behalf, because a host function that consumed no fuel made the
//!   budget meaningless for anything that crossed the boundary.
//! - **A linear-memory ceiling** bounds allocation, enforced through wasmi's
//!   `ResourceLimiter` rather than the module's own declared maximum — a
//!   hostile module simply omits that maximum.
//! - **Table and module-shape limits** bound what a module can make the host
//!   commit *before its first instruction runs*. A table's declared minimum is
//!   allocated eagerly at 8 bytes an element, so this is allocation that
//!   neither fuel nor the memory ceiling ever sees.
//! - **The granted host-function set** bounds effects, and is derived from the
//!   manifest before any guest code runs.
//!
//! Two further things are the host's and are reset per call: the **journal**
//! (bounded, and drained when the caller takes it) and the **wall clock**.

use std::sync::Arc;
use std::time::{Duration, Instant};

use wasmi::{
    Caller, Config, EnforcedLimits, Engine, Extern, Instance, Linker, Memory, Module, Store,
    StoreLimits, StoreLimitsBuilder, TrapCode,
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
    /// Includes the two cases this crate cares most about: the module imports
    /// a host function its manifest did not grant, so linking cannot complete;
    /// and the module's declared tables or memories exceed what the store will
    /// commit, so instantiation is refused before any allocation happens.
    #[error("plugin {id} could not be instantiated: {reason}")]
    Instantiate { id: String, reason: String },
    #[error("plugin {id} exports no function {name:?} with the expected signature")]
    MissingExport { id: String, name: String },
    #[error("plugin {id} exceeded its execution budget and was stopped")]
    OutOfFuel { id: String },
    #[error("plugin {id} exceeded its wall-clock budget for host calls and was stopped")]
    Deadline { id: String },
    #[error("plugin {id} trapped: {reason}")]
    Trap { id: String, reason: String },
}

pub type PluginResult<T> = Result<T, PluginError>;

/// wasmi's own exchange rate between bytes and fuel
/// (`FuelCosts::bytes_per_fuel`, 64 in wasmi 1.1). Host calls that copy guest
/// memory charge at the same rate, so asking the host to move a megabyte is
/// not cheaper than the guest moving it itself.
const BYTES_PER_FUEL: u64 = 64;

/// Flat charge for crossing the host boundary, in fuel, regardless of payload.
///
/// A boundary crossing is a trampoline, an approval-gate consultation and a
/// journal append; costing it at 100 guest instructions is conservative and
/// still low enough that ordinary plugin behaviour never notices. What it buys
/// is a hard ceiling on *call count*: with the default 5M fuel, no invocation
/// can make more than 50,000 host calls even with zero-length arguments.
/// Before this existed the measured ceiling was 1,666,631 calls.
const HOST_CALL_FUEL: u64 = 100;

/// The most of a single argument that is kept in a journal entry.
///
/// The journal is an audit trail, not a data sink: what matters is *that* a
/// call happened and roughly what it targeted. Every URL and every realistic
/// log line fits well inside this; a 64 KiB payload does not, and is recorded
/// truncated rather than retained in full.
const MAX_DETAIL_BYTES: usize = 1024;

/// Per-call resource budget.
///
/// The defaults are deliberately small. A contribution-model plugin computes a
/// panel's contents or handles a command; it is not a workload. Anything that
/// needs more than this should be a DevOS job, not a plugin.
#[derive(Debug, Clone, Copy)]
pub struct SandboxLimits {
    /// Fuel granted per call. Reset before every invocation, so a plugin
    /// cannot bank an earlier call's leftovers. Spent by guest instructions
    /// *and* by host calls, in proportion to the work they are asked to do.
    pub fuel: u64,
    /// Hard ceiling on the guest's linear memory, in bytes.
    pub memory_bytes: usize,
    /// Hard ceiling on how many tables the module may instantiate.
    ///
    /// Real toolchains emit exactly one — the indirect-call table. Four is
    /// headroom for reference-types codegen without being a budget anyone can
    /// spend.
    pub tables: usize,
    /// Hard ceiling on the elements in any one table.
    ///
    /// This is the limit SEC-101 was about. A table's declared *minimum* is
    /// allocated eagerly at 8 bytes an element when the module is
    /// instantiated, so it is host memory committed before the guest's first
    /// instruction — invisible to fuel and to `memory_bytes` alike.
    /// `tables × table_elements × 8` is the whole eager table budget: at the
    /// defaults that is 320 KiB, about 2% of `memory_bytes`, which is the
    /// property that matters. Tables must not be a second, unmetered way to
    /// ask for memory.
    pub table_elements: usize,
    /// Most entries the journal will hold for one call before it stops
    /// growing and starts counting.
    pub journal_entries: usize,
    /// Most bytes of recorded detail the journal will hold for one call.
    pub journal_bytes: usize,
    /// Wall-clock budget, checked on entry to each host call.
    ///
    /// This is **not** a general execution deadline and cannot be — see
    /// [`Sandbox::call_void`]. It bounds only the case where a plugin drives
    /// the host in a loop; pure computation is bounded by fuel instead.
    pub deadline: Option<Duration>,
}

impl Default for SandboxLimits {
    fn default() -> Self {
        Self {
            fuel: 5_000_000,
            memory_bytes: 16 * 1024 * 1024,
            // One indirect-call table is what a real toolchain emits; the
            // element cap matches `EnforcedLimits::strict()`'s 10,000-function
            // ceiling, since a table cannot usefully hold references to more
            // functions than the module is allowed to define.
            tables: 4,
            table_elements: 10_000,
            journal_entries: 1_000,
            journal_bytes: 64 * 1024,
            deadline: Some(Duration::from_secs(2)),
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
    /// The journal reached its cap and stopped recording. Carries how many
    /// entries were dropped, so "nothing else happened" and "a great deal else
    /// happened and we declined to store it" are distinguishable.
    Truncated {
        dropped: u64,
    },
}

impl JournalEntry {
    /// What this entry costs against [`SandboxLimits::journal_bytes`].
    fn weight(&self) -> usize {
        match self {
            JournalEntry::Log(detail)
            | JournalEntry::Allowed { detail, .. }
            | JournalEntry::Denied { detail, .. } => detail.len(),
            JournalEntry::Truncated { .. } => 0,
        }
    }
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

impl DenyReason {
    /// The text the guest sees in its trap. Kept here so the message and the
    /// journal entry cannot describe different refusals.
    fn as_str(self) -> &'static str {
        match self {
            DenyReason::NotAllowlisted => "target is not in the manifest's net allowlist",
            DenyReason::UserDenied => "the user denied this call",
            DenyReason::NotImplemented => "not implemented in this build",
        }
    }
}

/// How much work the guest made the host do during the current call.
///
/// Exists so "fuel bounds host work" is a property a test can assert on
/// directly, rather than one inferred from wall-clock time.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct HostWork {
    /// Host calls dispatched, including the ones that were refused.
    pub calls: u64,
    /// Bytes copied out of guest memory on the host's side.
    pub bytes_read: u64,
}

/// Host functions this spike actually implements. The rest are designed and
/// permission-gated (see [`crate::host::HostCall`]) but trap if called; the
/// gap is asserted in a test rather than described in a comment, so it cannot
/// quietly stop being true.
pub const IMPLEMENTED: &[HostCall] = &[HostCall::Log, HostCall::HttpFetch];

/// The bounded audit trail for one call.
///
/// Unbounded before the 2026-08 review: a plugin needing no permissions at all
/// could hold 2.08 GiB in it. Bounded twice over now — by entry count and by
/// recorded bytes — because either one alone is trivially worked around.
struct Journal {
    entries: Vec<JournalEntry>,
    bytes: usize,
    max_entries: usize,
    max_bytes: usize,
}

impl Journal {
    fn new(max_entries: usize, max_bytes: usize) -> Self {
        Self {
            entries: Vec::new(),
            bytes: 0,
            max_entries,
            max_bytes,
        }
    }

    /// Append, or — if either cap is reached — count the drop instead.
    ///
    /// The truncation marker is written once and then incremented in place, so
    /// a plugin that keeps calling cannot grow the journal by being refused
    /// entry to it.
    fn record(&mut self, entry: JournalEntry) {
        let weight = entry.weight();
        let full = self.entries.len() >= self.max_entries
            || self.bytes.saturating_add(weight) > self.max_bytes;
        if full {
            match self.entries.last_mut() {
                Some(JournalEntry::Truncated { dropped }) => *dropped = dropped.saturating_add(1),
                _ => self.entries.push(JournalEntry::Truncated { dropped: 1 }),
            }
            return;
        }
        self.bytes += weight;
        self.entries.push(entry);
    }

    fn entries(&self) -> &[JournalEntry] {
        &self.entries
    }

    fn take(&mut self) -> Vec<JournalEntry> {
        self.bytes = 0;
        std::mem::take(&mut self.entries)
    }
}

/// State the host functions share. Owned by the `Store`, so it is per-instance
/// and cannot be reached by another plugin.
pub struct HostState {
    plugin_id: String,
    allowlist: Vec<String>,
    gate: Arc<dyn ApprovalGate>,
    limits: StoreLimits,
    journal: Journal,
    work: HostWork,
    deadline: Option<Duration>,
    started: Instant,
    deadline_exceeded: bool,
}

impl HostState {
    pub fn journal(&self) -> &[JournalEntry] {
        self.journal.entries()
    }

    pub fn host_work(&self) -> HostWork {
        self.work
    }

    /// Record a boundary crossing, clamping the detail so one call cannot
    /// place 64 KiB into the audit trail.
    fn record(&mut self, entry: JournalEntry) {
        self.journal.record(clamp(entry));
    }

    /// Reset everything that is scoped to a single invocation.
    fn begin_call(&mut self) {
        self.journal.take();
        self.work = HostWork::default();
        self.started = Instant::now();
        self.deadline_exceeded = false;
    }
}

/// Truncate an entry's detail to [`MAX_DETAIL_BYTES`], on a char boundary,
/// marking it so a truncated value is never mistaken for a complete one.
fn clamp(entry: JournalEntry) -> JournalEntry {
    fn clamp_str(text: String) -> String {
        if text.len() <= MAX_DETAIL_BYTES {
            return text;
        }
        let mut end = MAX_DETAIL_BYTES;
        while end > 0 && !text.is_char_boundary(end) {
            end -= 1;
        }
        let mut clamped = text[..end].to_string();
        clamped.push('…');
        clamped
    }

    match entry {
        JournalEntry::Log(detail) => JournalEntry::Log(clamp_str(detail)),
        JournalEntry::Allowed { call, detail } => JournalEntry::Allowed {
            call,
            detail: clamp_str(detail),
        },
        JournalEntry::Denied {
            call,
            detail,
            reason,
        } => JournalEntry::Denied {
            call,
            detail: clamp_str(detail),
            reason,
        },
        marker @ JournalEntry::Truncated { .. } => marker,
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
    ///
    /// Also fails if the module's *declared shape* exceeds what the host is
    /// willing to commit — too many tables, a table whose minimum is larger
    /// than the element cap, more globals or functions than
    /// `EnforcedLimits::strict()` permits. Those checks matter because they
    /// are the only ones that run before allocation: a module is untrusted
    /// input to the compiler and to the instantiator, not only to the
    /// interpreter.
    pub fn load(
        manifest: &PluginManifest,
        wasm: &[u8],
        gate: Arc<dyn ApprovalGate>,
        limits: SandboxLimits,
    ) -> PluginResult<Self> {
        let mut config = Config::default();
        config.consume_fuel(true);
        // Compile-time bounds on module shape: globals, functions, tables,
        // element and data segments, parameter and result counts, and a
        // minimum average function size that defends wasmi's own lazy
        // compilation against a module made of a million empty functions.
        // Nothing here was capped before the 2026-08 review.
        config.enforced_limits(EnforcedLimits::strict());
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
                // One module per store, always. Left at wasmi's default of
                // 10,000 there was no reason for it to be.
                .instances(1)
                // SEC-101. Both of these were unset: `tables` defaulted to
                // 10,000 and `table_elements` to unlimited, which let a
                // 146-byte module commit 976 MiB at instantiation time.
                .tables(limits.tables)
                .table_elements(limits.table_elements)
                .build(),
            journal: Journal::new(limits.journal_entries, limits.journal_bytes),
            work: HostWork::default(),
            deadline: limits.deadline,
            started: Instant::now(),
            deadline_exceeded: false,
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
    ///
    /// Synchronous: this blocks the calling thread until the plugin returns or
    /// is stopped. Fuel guarantees it *is* stopped, but fuel is not a clock —
    /// an interpreter executes a fuel budget in whatever wall time the machine
    /// takes. A caller that needs a hard wall-clock bound must own it (run the
    /// sandbox on a thread it is prepared to abandon); the sandbox can only
    /// promise termination, and [`SandboxLimits::deadline`] only covers the
    /// specific case of a plugin driving the host in a loop.
    pub fn call_void(&mut self, name: &str) -> PluginResult<()> {
        self.begin_call()?;
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
        self.begin_call()?;
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
        self.begin_call()?;
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

    /// What the plugin did across the boundary during the **most recent call**.
    ///
    /// Scoped to one call, like fuel: the next invocation starts with an empty
    /// journal whether or not anyone read this one. A consumer that needs the
    /// record must take it — see [`Sandbox::take_journal`]. Retaining it
    /// across calls is what let a permission-free plugin hold gigabytes.
    pub fn journal(&self) -> &[JournalEntry] {
        self.store.data().journal()
    }

    /// Take the journal, leaving it empty. This is the app-facing accessor:
    /// the audit trail belongs to whatever writes `audit_log`, and the sandbox
    /// should not be holding a second copy of it.
    pub fn take_journal(&mut self) -> Vec<JournalEntry> {
        self.store.data_mut().journal.take()
    }

    /// How much work the most recent call made the host do. The accounting
    /// behind [`SandboxLimits::fuel`]'s claim to bound host calls.
    pub fn host_work(&self) -> HostWork {
        self.store.data().host_work()
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

    /// Every call starts from a full tank, an empty journal and a fresh clock;
    /// nothing accumulates.
    fn begin_call(&mut self) -> PluginResult<()> {
        self.store.data_mut().begin_call();
        self.store
            .set_fuel(self.fuel)
            .map_err(|e| PluginError::Trap {
                id: self.id.clone(),
                reason: e.to_string(),
            })
    }

    fn classify(&self, error: wasmi::Error) -> PluginError {
        // Checked first: a deadline stop is reported to the guest as an
        // ordinary host error, so only the host's own flag can tell it apart
        // from a plugin bug.
        if self.store.data().deadline_exceeded {
            return PluginError::Deadline {
                id: self.id.clone(),
            };
        }
        if error.as_trap_code() == Some(TrapCode::OutOfFuel) {
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
/// Note what is *not* here: any per-call permission check. Every granted call
/// is wired to the same [`dispatch`], which applies the per-call checks
/// uniformly. Which arms of *this* match run decides whether a capability
/// exists at all; `dispatch` decides what happens when one is used.
fn define_granted(linker: &mut Linker<HostState>, manifest: &PluginManifest) -> PluginResult<()> {
    let fail = |e: wasmi::errors::LinkerError| PluginError::Instantiate {
        id: manifest.id.clone(),
        reason: e.to_string(),
    };

    for call in granted(&manifest.permissions) {
        match call {
            // `log` is the one call whose settled signature returns nothing.
            // Everything else takes and returns the placeholder
            // `(i32, i32) -> i32` shape — a placeholder, not a commitment:
            // these calls have no agreed ABI yet, and a guest importing one
            // with a different signature will still fail to link. Settling the
            // ABI is the work these stubs stand in for.
            HostCall::Log => {
                linker
                    .func_wrap(
                        IMPORT_MODULE,
                        call.name(),
                        |mut caller: Caller<'_, HostState>,
                         ptr: i32,
                         len: i32|
                         -> Result<(), wasmi::Error> {
                            dispatch(&mut caller, HostCall::Log, ptr, len).map(drop)
                        },
                    )
                    .map_err(fail)?;
            }
            other => {
                linker
                    .func_wrap(
                        IMPORT_MODULE,
                        other.name(),
                        move |mut caller: Caller<'_, HostState>,
                              ptr: i32,
                              len: i32|
                              -> Result<i32, wasmi::Error> {
                            dispatch(&mut caller, other, ptr, len)
                        },
                    )
                    .map_err(fail)?;
            }
        }
    }
    Ok(())
}

/// The single entry point for every host call, and the place the per-call
/// checks live.
///
/// The order is the design:
///
/// 1. **Deadline**, so a plugin driving the host in a loop is stopped even if
///    the individual calls are cheap.
/// 2. **Fuel**, flat plus per byte, charged *before* the work rather than
///    after it — a budget checked afterwards is not a budget.
/// 3. **Scope** — for egress, the manifest's allowlist. Checked before the
///    user is asked: a prompt for a host the manifest never declared is a
///    prompt the user should never see, because asking trains them to approve
///    and hands them a decision that was not theirs to make.
/// 4. **Approval**, keyed off [`HostCall::needs_approval`], around *every*
///    arm. This is SEC-103: it used to sit inside the `HttpFetch` arm, which
///    meant `NEEDS_APPROVAL` decided nothing and the next call added to it
///    would have dispatched ungated while reading as gated.
/// 5. **The effect**, which is the only step that is per-call specific.
///
/// A refusal at 3 or 4 traps. Returning a sentinel meant a guest could loop on
/// being refused, and a refusal that can be looped on is not free for the host
/// — it is a way to spend the host's time and journal at the guest's
/// convenience. Trapping ends the invocation, so exactly one denial is
/// recorded per attempt. A plugin that wants to degrade gracefully gets a
/// fresh call with a fresh budget from the app; it does not get to retry
/// inside one.
fn dispatch(
    caller: &mut Caller<'_, HostState>,
    call: HostCall,
    ptr: i32,
    len: i32,
) -> Result<i32, wasmi::Error> {
    const OK: i32 = 0;

    check_deadline(caller)?;
    charge_fuel(caller, HOST_CALL_FUEL)?;
    let detail = read_guest_string(caller, ptr, len)?;
    caller.data_mut().work.calls += 1;

    if let Some(reason) = out_of_scope(caller.data(), call, &detail) {
        return Err(refuse(caller, call, detail, reason));
    }

    if call.needs_approval() {
        let request = ApprovalRequest {
            plugin_id: caller.data().plugin_id.clone(),
            call,
            detail: detail.clone(),
        };
        let gate = Arc::clone(&caller.data().gate);
        if gate.request(&request) == Decision::Deny {
            return Err(refuse(caller, call, detail, DenyReason::UserDenied));
        }
    }

    match call {
        HostCall::Log => {
            caller.data_mut().record(JournalEntry::Log(detail));
            Ok(OK)
        }
        // The spike stops here: it records that the request was permitted
        // rather than performing it. Nothing about the gating changes when a
        // real client is attached, and leaving the socket out keeps these
        // tests hermetic.
        HostCall::HttpFetch => {
            caller
                .data_mut()
                .record(JournalEntry::Allowed { call, detail });
            Ok(OK)
        }
        // Designed, permission-gated, not implemented by this spike. Defined
        // anyway so a plugin importing one still links — and then fails
        // loudly at the call, in the journal, rather than failing confusingly
        // at instantiation.
        other => Err(refuse(caller, other, detail, DenyReason::NotImplemented)),
    }
}

/// Record a refusal and produce the trap that ends the call.
///
/// Journalling happens first and unconditionally: a refusal that leaves no
/// trace is indistinguishable from a call that never happened.
fn refuse(
    caller: &mut Caller<'_, HostState>,
    call: HostCall,
    detail: String,
    reason: DenyReason,
) -> wasmi::Error {
    let message = format!("devos.{} refused: {}", call.name(), reason.as_str());
    caller.data_mut().record(JournalEntry::Denied {
        call,
        detail,
        reason,
    });
    wasmi::Error::new(message)
}

/// Per-call scope checks that are decided from declared data, not from the
/// user. Today only egress has one.
fn out_of_scope(state: &HostState, call: HostCall, detail: &str) -> Option<DenyReason> {
    match call {
        HostCall::HttpFetch if !net_allows(&state.allowlist, detail) => {
            Some(DenyReason::NotAllowlisted)
        }
        _ => None,
    }
}

/// Stop the call if it has been driving the host past its wall-clock budget.
///
/// Honest about what this is: it is checked only on entry to a host call, so
/// it bounds *the plugin making the host work*, which is the shape SEC-102
/// reproduced (1,666,631 calls over 52 seconds). It cannot interrupt a compute
/// loop — fuel does that — and it cannot interrupt a host function already
/// blocked inside a syscall, which is what a real `http_fetch` will be. The
/// hard wall-clock bound belongs to the caller and is documented on
/// [`Sandbox::call_void`].
fn check_deadline(caller: &mut Caller<'_, HostState>) -> Result<(), wasmi::Error> {
    let state = caller.data();
    let Some(deadline) = state.deadline else {
        return Ok(());
    };
    if state.started.elapsed() <= deadline {
        return Ok(());
    }
    caller.data_mut().deadline_exceeded = true;
    Err(wasmi::Error::new(format!(
        "host calls exceeded the plugin's {deadline:?} wall-clock budget"
    )))
}

/// Take `amount` fuel from the guest's budget, or stop it.
///
/// Host functions registered with `func_wrap` consume no fuel of their own, so
/// without this the fuel budget bounded only what the guest did *itself* —
/// which is precisely nothing, for a plugin whose whole strategy is to make
/// the host do the work.
fn charge_fuel(caller: &mut Caller<'_, HostState>, amount: u64) -> Result<(), wasmi::Error> {
    if amount == 0 {
        return Ok(());
    }
    let remaining = caller.get_fuel()?;
    match remaining.checked_sub(amount) {
        Some(left) => {
            caller.set_fuel(left)?;
            Ok(())
        }
        None => {
            caller.set_fuel(0)?;
            Err(wasmi::Error::from(TrapCode::OutOfFuel))
        }
    }
}

/// Read a `(ptr, len)` string out of the guest's linear memory.
///
/// `Memory::read` is bounds-checked against the guest's *current* memory, so a
/// pointer past the end is an error here rather than a host-side out-of-bounds
/// read. The length cap is separate: it stops a guest from asking the host to
/// materialise a 4 GiB `String` on its behalf, which is a host-memory
/// exhaustion the guest's own ceiling would not cover.
///
/// The cap is per call, though, and nothing caps the number of calls — which
/// is why the copy is charged fuel at wasmi's own [`BYTES_PER_FUEL`] rate
/// before it happens. 64 KiB read a million times is a hundred gigabytes, and
/// the budget has to see it.
fn read_guest_string(
    caller: &mut Caller<'_, HostState>,
    ptr: i32,
    len: i32,
) -> Result<String, wasmi::Error> {
    const MAX_LEN: i32 = 64 * 1024;

    if !(0..=MAX_LEN).contains(&len) || ptr < 0 {
        return Err(wasmi::Error::new(format!(
            "host call rejected: (ptr {ptr}, len {len}) is out of range"
        )));
    }
    charge_fuel(caller, len as u64 / BYTES_PER_FUEL)?;

    let memory: Memory = caller
        .get_export("memory")
        .and_then(Extern::into_memory)
        .ok_or_else(|| wasmi::Error::new("plugin exports no linear memory named `memory`"))?;

    let mut buffer = vec![0u8; len as usize];
    memory
        .read(&*caller, ptr as usize, &mut buffer)
        .map_err(|e| wasmi::Error::new(format!("host call rejected: {e}")))?;
    caller.data_mut().work.bytes_read += buffer.len() as u64;

    String::from_utf8(buffer).map_err(|_| wasmi::Error::new("host call rejected: invalid UTF-8"))
}

/// Convenience for callers that have no approval channel wired up yet.
pub fn deny_all() -> Arc<dyn ApprovalGate> {
    Arc::new(DenyAll)
}
