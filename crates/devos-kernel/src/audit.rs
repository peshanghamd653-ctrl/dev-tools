//! The audit log: the durable answer to "what happened to my machine?"
//!
//! Everything DevOS does is visible *while it happens* — a tool call renders
//! in the chat, a refused write renders in the SQL editor, a restore raises a
//! notification. None of that survives. Conversations get deleted, the chat
//! transcript is not a security record, and the one question worth answering
//! after the fact — "did I approve that, and when?" — has no home. This table
//! is that home.
//!
//! # What lands here, and what deliberately does not
//!
//! The temptation is to log everything, which produces a table nobody reads
//! and a privacy problem of its own: a complete record of which files the
//! model looked at is a browsing history of the user's source tree, written
//! into the same database whose whole point is that secrets are *not* in it.
//! So the set is closed and small — every variant of [`AuditEvent`] is an
//! event a human had to initiate and would want to find later.
//!
//! The rule that decides what a row may carry is **record the action, never
//! the payload**:
//!
//! | Event | Recorded | Not recorded, and why |
//! |---|---|---|
//! | AI tool approval/denial | tool name, the thing acted on, the outcome, and the reason for a denial | file *contents*; `save_memory`'s text (already durable and user-visible in the Memory panel) |
//! | Secret set/delete | the **name** | the value — see [`AuditEvent::SecretSet`] |
//! | SQL editor write | which connection, how many rows moved | the statement, which is where the *data* lives (`INSERT INTO t VALUES ('<a token>')`) |
//! | Issue created | `owner/name#number` | the body, which is free-form prose that routinely quotes logs and config |
//! | Restore applied/refused | which backup, and what the previous database was preserved as | — |
//!
//! Read-only tool calls (`read_file`, `list_dir`, `find_files`,
//! `search_code`) are the high-volume ones and are absent on purpose: they
//! are side-effect-free by construction, containment-checked before they run,
//! and recording each one would bury the mutating events under a log of the
//! user reading their own code. Ordinary CRUD — workspaces, projects,
//! snippets, monitors, saved requests — is absent for the same reason it is
//! not in `docs/security.md`: it is recoverable and already visible in its own
//! screen.
//!
//! # Never breaking the thing it records
//!
//! [`record`] returns `()`. A failed insert is logged and execution continues,
//! exactly as a failed backup does not stop a boot. An audit log that can veto
//! the action it is describing is a liability, not a control.

use sqlx::SqlitePool;

/// How long an entry is kept, in days.
///
/// **Why an age window rather than a row cap.** A cap ("keep the newest
/// 5,000") bounds the file but lets one busy afternoon evict a year of
/// history — the precise failure an audit log exists to prevent. An age window
/// answers the question people actually ask ("what happened to my machine last
/// month?") and cannot silently discard something that is still inside its own
/// stated window.
///
/// **Why 90 days is safe to promise.** Every event in [`AuditEvent`] requires
/// a human gesture: an approval click (or a 180-second wait), a Run press in
/// the SQL editor, a Save on a secret, a Create on an issue, a restart to
/// apply a restore. The write rate is bounded by a person, not by a loop, so
/// the window cannot blow up into an unbounded table — which is exactly why a
/// second, silently-truncating axis is not needed on top of it.
///
/// The number is surfaced to the UI in
/// [`AuditLog::retention_days`](crate::types::AuditLog::retention_days) rather
/// than restated in TypeScript, so the screen cannot claim a window the
/// backend does not keep.
pub const RETENTION_DAYS: i64 = 90;

/// Longest target string a row will carry.
///
/// A `run_command` line is the one recorded field with no natural ceiling.
/// Truncating it keeps a single row from becoming a document while leaving the
/// part a person reads — the program and its first arguments — intact.
const MAX_TARGET_CHARS: usize = 160;

/// Why a mutating tool call did not run.
///
/// "Denied" alone is not an answer: a user who pressed Deny and a user who
/// walked away from the machine for four minutes produce the same refusal for
/// the model and very different stories for the person reading this table
/// later. See `APPROVAL_TIMEOUT` in `src-tauri/src/approvals.rs`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DenialReason {
    /// The user pressed Deny.
    Declined,
    /// Nobody answered within the approval timeout.
    TimedOut { after_secs: u64 },
    /// The executor had nowhere to ask, so it failed closed. Not reachable
    /// from the running app — `ai_send` always builds a gate — but a
    /// gate-less executor refusing is a property worth seeing if it ever is.
    NoApprovalChannel,
    /// The approval could not reach the UI, or the channel died mid-wait.
    Unreachable,
}

impl DenialReason {
    fn describe(&self) -> String {
        match self {
            DenialReason::Declined => "denied by the user".to_string(),
            DenialReason::TimedOut { after_secs } => {
                format!("no answer within {after_secs}s, so it was treated as a denial")
            }
            DenialReason::NoApprovalChannel => {
                "no approval channel, so the call was refused".to_string()
            }
            DenialReason::Unreachable => {
                "the approval request could not reach the user".to_string()
            }
        }
    }
}

/// The closed set of security-relevant events DevOS records.
///
/// This vocabulary lives in the kernel because the kernel owns `audit_log`,
/// and a table whose `action` strings each module invents for itself is a
/// table nobody can query. Adding a variant here is the deliberate act of
/// deciding something belongs in the security record; there is no
/// free-form `record(actor, action, detail)` for a call site to reach for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuditEvent {
    /// A mutating AI tool call the user approved. `target` is the thing acted
    /// on — a path, a command line — never the content written.
    AiToolApproved {
        tool: String,
        target: Option<String>,
    },
    /// A mutating AI tool call that did not run, and why.
    AiToolDenied {
        tool: String,
        target: Option<String>,
        reason: DenialReason,
    },
    /// A secret was stored or overwritten.
    ///
    /// **There is no value field, by construction.** `SecretMeta` is redacted
    /// at the type level for exactly this reason (`docs/security.md`), and the
    /// audit log must not become the one place a value could reappear — it is
    /// plaintext in the same database the encryption exists to survive. The
    /// compiler enforces the absence; no screen or format string is trusted
    /// to leave it out.
    SecretSet { name: String },
    /// A secret was deleted. Name only, same reasoning.
    SecretDeleted { name: String },
    /// A write statement ran through the SQL editor against a saved
    /// connection. The statement itself is not recorded: it is where the data
    /// lives, and a `VALUES` list can carry anything the user's database
    /// carries.
    SqlWrite {
        connection: String,
        rows_affected: i64,
    },
    /// A staged restore replaced the database at boot. The durable half of
    /// the notification `Kernel::boot` already raises.
    DatabaseRestored {
        source: String,
        /// What the replaced database was preserved as — the one thing
        /// somebody needs if the restore was a mistake.
        preserved: Option<String>,
    },
    /// A staged restore was refused at boot and the database left alone.
    DatabaseRestoreRefused { source: String, error: String },
    /// An issue was filed on GitHub — DevOS's only outward-facing write, and
    /// the only recorded event whose effect is public and irreversible. The
    /// body is not recorded; the issue is its own durable copy of it.
    IssueCreated { repo: String, number: i64 },
}

impl AuditEvent {
    /// Who caused this. `ai` and `user` are a real distinction: "did I do
    /// this, or did the model do it with my permission?" is most of what
    /// someone opens this table to find out.
    pub fn actor(&self) -> &'static str {
        match self {
            AuditEvent::AiToolApproved { .. } | AuditEvent::AiToolDenied { .. } => "ai",
            AuditEvent::SecretSet { .. }
            | AuditEvent::SecretDeleted { .. }
            | AuditEvent::SqlWrite { .. }
            | AuditEvent::IssueCreated { .. } => "user",
            AuditEvent::DatabaseRestored { .. } | AuditEvent::DatabaseRestoreRefused { .. } => {
                "system"
            }
        }
    }

    /// The stable, machine-readable event type.
    ///
    /// The outcome is part of the identifier (`ai.tool.approved` vs
    /// `ai.tool.denied`) rather than buried in `detail`, so "show me every
    /// denial" is a prefix match instead of a prose search.
    pub fn action(&self) -> &'static str {
        match self {
            AuditEvent::AiToolApproved { .. } => "ai.tool.approved",
            AuditEvent::AiToolDenied { .. } => "ai.tool.denied",
            AuditEvent::SecretSet { .. } => "secret.set",
            AuditEvent::SecretDeleted { .. } => "secret.deleted",
            AuditEvent::SqlWrite { .. } => "db.write",
            AuditEvent::DatabaseRestored { .. } => "backup.restored",
            AuditEvent::DatabaseRestoreRefused { .. } => "backup.restore_refused",
            AuditEvent::IssueCreated { .. } => "issue.created",
        }
    }

    /// One line naming what was acted on, and why it went the way it did.
    pub fn detail(&self) -> Option<String> {
        match self {
            AuditEvent::AiToolApproved { tool, target } => Some(match target {
                Some(target) => format!("{tool}: {}", truncate(target)),
                None => tool.clone(),
            }),
            AuditEvent::AiToolDenied {
                tool,
                target,
                reason,
            } => Some(match target {
                Some(target) => format!("{tool}: {} — {}", truncate(target), reason.describe()),
                None => format!("{tool} — {}", reason.describe()),
            }),
            AuditEvent::SecretSet { name } | AuditEvent::SecretDeleted { name } => {
                Some(truncate(name))
            }
            AuditEvent::SqlWrite {
                connection,
                rows_affected,
            } => Some(format!(
                "{} — {rows_affected} row{} affected",
                truncate(connection),
                if *rows_affected == 1 { "" } else { "s" }
            )),
            AuditEvent::DatabaseRestored { source, preserved } => Some(match preserved {
                Some(preserved) => format!(
                    "{} — the replaced database is kept as {preserved}",
                    truncate(source)
                ),
                None => format!("{} — there was no database to preserve", truncate(source)),
            }),
            AuditEvent::DatabaseRestoreRefused { source, error } => {
                Some(format!("{} — {}", truncate(source), truncate(error)))
            }
            AuditEvent::IssueCreated { repo, number } => Some(format!("{repo}#{number}")),
        }
    }
}

/// Clamp a recorded field to [`MAX_TARGET_CHARS`], on a character boundary.
///
/// Marked as cut rather than quietly shortened — a command line that ends
/// mid-token and says nothing about it is worse than no entry, because it
/// reads as the whole command.
fn truncate(value: &str) -> String {
    let value = value.trim();
    if value.chars().count() <= MAX_TARGET_CHARS {
        return value.to_string();
    }
    let cut: String = value.chars().take(MAX_TARGET_CHARS).collect();
    format!("{cut}… (truncated)")
}

/// Append an event to the audit log.
///
/// Infallible at the call site on purpose: writing the record must never
/// break the thing it records. A failure is logged and skipped, the same
/// contract `Kernel::notify` has for backups — and the reason this returns
/// `()` rather than a `Result` nobody would be able to do anything with.
pub async fn record(pool: &SqlitePool, event: AuditEvent) {
    let action = event.action();
    if let Err(error) =
        crate::repo::add_audit_entry(pool, event.actor(), action, event.detail().as_deref()).await
    {
        tracing::warn!(action, %error, "could not write the audit entry");
    }
}

/// Drop entries older than [`RETENTION_DAYS`].
///
/// `now_ms` is a parameter rather than read from the clock so the boundary is
/// testable without sleeping for three months.
pub async fn prune(pool: &SqlitePool, now_ms: i64) -> crate::error::KernelResult<u64> {
    let cutoff = now_ms - RETENTION_DAYS * 24 * 60 * 60 * 1000;
    crate::repo::prune_audit_log(pool, cutoff).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_secret_event_can_only_carry_a_name() {
        // Not an assertion about a format string — an assertion about the
        // shape. `SecretSet` has one field, and it is the name. If a value
        // field is ever added this stops compiling.
        let event = AuditEvent::SecretSet {
            name: "anthropic-api-key".into(),
        };
        assert_eq!(event.actor(), "user");
        assert_eq!(event.action(), "secret.set");
        assert_eq!(event.detail().as_deref(), Some("anthropic-api-key"));
    }

    #[test]
    fn a_denial_says_which_kind_of_denial_it_was() {
        let declined = AuditEvent::AiToolDenied {
            tool: "run_command".into(),
            target: Some("rm -rf build".into()),
            reason: DenialReason::Declined,
        }
        .detail()
        .unwrap();
        assert!(declined.contains("rm -rf build"), "{declined}");
        assert!(declined.contains("denied by the user"), "{declined}");

        let timed_out = AuditEvent::AiToolDenied {
            tool: "write_file".into(),
            target: Some("src/new.rs".into()),
            reason: DenialReason::TimedOut { after_secs: 180 },
        }
        .detail()
        .unwrap();
        assert!(timed_out.contains("180s"), "{timed_out}");
        assert_ne!(
            declined, timed_out,
            "a walk-away and a refusal must not read the same"
        );
    }

    #[test]
    fn a_long_command_is_cut_and_says_so() {
        let long = "cargo run -- ".to_string() + &"x".repeat(500);
        let detail = AuditEvent::AiToolApproved {
            tool: "run_command".into(),
            target: Some(long),
        }
        .detail()
        .unwrap();
        assert!(detail.contains("cargo run"), "the head survives");
        assert!(detail.ends_with("… (truncated)"), "got: {detail}");
        assert!(detail.chars().count() < 220, "still one line");
    }

    #[test]
    fn every_action_is_a_distinct_dotted_identifier() {
        let events = [
            AuditEvent::AiToolApproved {
                tool: "x".into(),
                target: None,
            },
            AuditEvent::AiToolDenied {
                tool: "x".into(),
                target: None,
                reason: DenialReason::Declined,
            },
            AuditEvent::SecretSet { name: "x".into() },
            AuditEvent::SecretDeleted { name: "x".into() },
            AuditEvent::SqlWrite {
                connection: "x".into(),
                rows_affected: 1,
            },
            AuditEvent::DatabaseRestored {
                source: "x".into(),
                preserved: None,
            },
            AuditEvent::DatabaseRestoreRefused {
                source: "x".into(),
                error: "y".into(),
            },
            AuditEvent::IssueCreated {
                repo: "a/b".into(),
                number: 1,
            },
        ];
        let mut actions: Vec<&str> = events.iter().map(AuditEvent::action).collect();
        let count = actions.len();
        actions.sort_unstable();
        actions.dedup();
        assert_eq!(actions.len(), count, "action identifiers must be unique");
        assert!(
            actions.iter().all(|a| a.contains('.')),
            "actions are dotted so the UI can group by domain: {actions:?}"
        );
    }

    #[test]
    fn one_row_affected_is_not_pluralised() {
        let one = AuditEvent::SqlWrite {
            connection: "notes.db".into(),
            rows_affected: 1,
        }
        .detail()
        .unwrap();
        assert!(one.contains("1 row affected"), "{one}");
        let many = AuditEvent::SqlWrite {
            connection: "notes.db".into(),
            rows_affected: 12,
        }
        .detail()
        .unwrap();
        assert!(many.contains("12 rows affected"), "{many}");
    }
}
