//! Automatic backups of the DevOS database file, and the path back from one.
//!
//! Two triggers write snapshots, both at boot, both best effort:
//!
//! - **pre-migration** — a snapshot taken immediately before `sqlx::migrate!`
//!   applies anything, so a migration that drops or mangles data can be
//!   undone by hand. Only when a migration is actually pending.
//! - **daily** — at most one snapshot per calendar day, keeping the newest
//!   [`DAILY_RETENTION`] and deleting the rest.
//!
//! Both write into a `backups/` directory beside the live database. Neither
//! can fail the boot: every error is logged and swallowed, because an app
//! that starts without a backup beats an app that refuses to start.
//!
//! # Restore
//!
//! Reading a backup back in is a different shape of problem from writing one.
//! A snapshot can be taken from a live pool; installing one cannot. By the
//! time any UI code runs, `devos.db` is open, `devos.db-wal` and
//! `devos.db-shm` exist beside it, and every `Arc<Kernel>` in the app is
//! holding that pool. So restore is split in two:
//!
//! 1. [`stage_restore`] — runs while the app is up. It validates the chosen
//!    file, copies it to `restore-pending/staged.db` beside the database, and
//!    only then writes `restore-pending/RESTORE.json`. That marker is the
//!    commit point, and it lands via a rename, so it is either wholly there or
//!    wholly absent.
//! 2. [`apply_pending_restore`] — runs from [`Kernel::boot`](crate::Kernel::boot)
//!    *before* the pool is opened, which is the only moment the file can be
//!    swapped. It preserves the current database first, deletes the sidecars,
//!    and renames the staged file into place.
//!
//! Two properties are load-bearing and are tested rather than asserted here:
//! **the database being replaced is always preserved first**, under
//! `devos-replaced-<timestamp>.db`; and **an interrupted restore either fully
//! applied or did not happen**, because the only two things that survive a
//! kill are the marker and the staged file, and every combination of the two
//! has exactly one honest interpretation.

use std::path::{Path, PathBuf};

use chrono::Local;
use serde::{Deserialize, Serialize};
use sqlx::migrate::Migrator;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;
use ts_rs::TS;

use crate::error::{KernelError, KernelResult};

/// How many daily snapshots to keep before the oldest is deleted. A week is
/// enough to notice a bad day and roll back to before it, without the backup
/// directory growing to several times the size of the database itself.
pub const DAILY_RETENTION: usize = 7;

const BACKUP_DIR: &str = "backups";
/// `devos-daily-YYYY-MM-DD.db` — fixed-width date, so lexical order over the
/// file names *is* chronological order and rotation needs no `mtime` lookups.
const DAILY_PREFIX: &str = "devos-daily-";
const PRE_MIGRATION_PREFIX: &str = "devos-premigration-";
/// `devos-replaced-YYYYMMDD-HHMMSS.db` — the database a restore displaced.
/// Written by [`apply_pending_restore`], never by rotation, and never pruned.
const REPLACED_PREFIX: &str = "devos-replaced-";

/// Staging area for a restore, beside the database rather than in a temp
/// directory: the final step is a `rename`, which is only atomic (and on
/// Windows only *possible*) within one filesystem.
const RESTORE_DIR: &str = "restore-pending";
const STAGED_FILE: &str = "staged.db";
const MARKER_FILE: &str = "RESTORE.json";
/// Suffix appended while a file is still being written. Nothing reads a
/// `.part`; the rename off it is what publishes the content.
const PARTIAL_SUFFIX: &str = ".part";

/// The `backups/` directory beside `db_path`.
pub fn backup_dir(db_path: &Path) -> PathBuf {
    db_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(BACKUP_DIR)
}

/// The `restore-pending/` directory beside `db_path`.
pub fn restore_dir(db_path: &Path) -> PathBuf {
    db_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(RESTORE_DIR)
}

/// Snapshot the database before migrations run. Best effort — returns the
/// backup path when one was written, `None` when it was skipped or failed.
///
/// Call this *immediately* before `Migrator::run`, with the same migrator.
pub async fn run_pre_migration_backup(
    pool: &SqlitePool,
    db_path: &Path,
    migrator: &Migrator,
) -> Option<PathBuf> {
    match try_pre_migration_backup(pool, db_path, migrator).await {
        Ok(Some(path)) => {
            tracing::info!(backup = %path.display(), "pre-migration database backup written");
            Some(path)
        }
        Ok(None) => None,
        Err(e) => {
            tracing::warn!(error = %e, "pre-migration database backup failed; continuing to migrate");
            None
        }
    }
}

/// Snapshot the database once per calendar day, then prune to
/// [`DAILY_RETENTION`]. Best effort — returns the backup path only when this
/// call is the one that wrote it.
pub async fn run_daily_backup(pool: &SqlitePool, db_path: &Path) -> Option<PathBuf> {
    match try_daily_backup(pool, db_path).await {
        Ok(Some(path)) => {
            tracing::info!(backup = %path.display(), "daily database backup written");
            Some(path)
        }
        Ok(None) => None,
        Err(e) => {
            tracing::warn!(error = %e, "daily database backup failed; continuing without it");
            None
        }
    }
}

async fn try_pre_migration_backup(
    pool: &SqlitePool,
    db_path: &Path,
    migrator: &Migrator,
) -> KernelResult<Option<PathBuf>> {
    if !db_path.is_file() {
        tracing::debug!("no database file yet, skipping pre-migration backup");
        return Ok(None);
    }

    let (applied_any, pending) = migration_state(pool, migrator).await;
    if !applied_any {
        // First run. The pool is opened with `create_if_missing`, so the file
        // already exists by the time we hold a pool to query through — an
        // empty `_sqlx_migrations` is the honest first-run signal, and a
        // brand new database has nothing worth preserving.
        tracing::debug!("first run, skipping pre-migration backup");
        return Ok(None);
    }
    if !pending {
        // Steady-state boot. Copying here would mean a snapshot on every
        // single launch; that job belongs to the daily rotation.
        return Ok(None);
    }

    let target = migrator.iter().map(|m| m.version).max().unwrap_or_default();
    let dir = backup_dir(db_path);
    // Timestamp first so these sort chronologically alongside each other;
    // the target version says what the snapshot is "before".
    let dest = dir.join(format!(
        "{PRE_MIGRATION_PREFIX}{}-v{target:04}.db",
        Local::now().format("%Y%m%d-%H%M%S")
    ));
    create_dir(&dir)?;
    snapshot(pool, db_path, &dest).await?;
    Ok(Some(dest))
}

async fn try_daily_backup(pool: &SqlitePool, db_path: &Path) -> KernelResult<Option<PathBuf>> {
    if !db_path.is_file() {
        tracing::debug!("no database file yet, skipping daily backup");
        return Ok(None);
    }

    let dir = backup_dir(db_path);
    let dest = dir.join(format!(
        "{DAILY_PREFIX}{}.db",
        Local::now().format("%Y-%m-%d")
    ));
    if dest.exists() {
        // Already backed up today. The file name is the whole record of that
        // — no state to keep in `settings`, and deleting the file is a valid
        // way for the user to ask for a fresh one.
        return Ok(None);
    }

    create_dir(&dir)?;
    snapshot(pool, db_path, &dest).await?;
    prune_daily(&dir);
    Ok(Some(dest))
}

/// `(any migration applied, any migration pending)`.
async fn migration_state(pool: &SqlitePool, migrator: &Migrator) -> (bool, bool) {
    // A missing `_sqlx_migrations` is not an error here: it just means sqlx
    // has never migrated this file, which is exactly the fresh-database case.
    let applied: Vec<i64> = sqlx::query_scalar("SELECT version FROM _sqlx_migrations")
        .fetch_all(pool)
        .await
        .unwrap_or_default();
    let pending = migrator.iter().any(|m| !applied.contains(&m.version));
    (!applied.is_empty(), pending)
}

/// Write a consistent snapshot of the live database to `dest`.
///
/// The database runs in WAL mode ([ADR-0004]), which is what makes this
/// non-trivial: committed transactions live in `devos.db-wal` until a
/// checkpoint folds them back into `devos.db`. Copying the main file on its
/// own therefore yields a backup that is silently missing the most recent
/// commits, and — if a checkpoint is writing pages while the copy reads them
/// — possibly a torn one. A backup like that is worse than no backup, because
/// it will be trusted.
///
/// `VACUUM INTO` is the correct answer: SQLite runs it inside a read
/// transaction, so the file it produces contains everything committed at that
/// instant (WAL frames included), is self-contained, and the live database is
/// never modified — no checkpoint forced on a running app, no writer blocked
/// beyond the read transaction. It has existed since SQLite 3.27 (2019) and
/// the `libsqlite3-sys` bundled by sqlx 0.8 is far newer, so this is the path
/// that actually runs; [`checkpoint_and_copy`] exists for a build linked
/// against an older system SQLite.
///
/// [ADR-0004]: ../../../docs/adr/0004-single-sqlite-file-with-wal.md
async fn snapshot(pool: &SqlitePool, db_path: &Path, dest: &Path) -> KernelResult<()> {
    if supports_vacuum_into(pool).await {
        vacuum_into(pool, dest).await
    } else {
        tracing::warn!("SQLite predates VACUUM INTO; falling back to checkpoint + file copy");
        checkpoint_and_copy(pool, db_path, dest).await
    }
}

async fn supports_vacuum_into(pool: &SqlitePool) -> bool {
    let version: String = match sqlx::query_scalar("SELECT sqlite_version()")
        .fetch_one(pool)
        .await
    {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(error = %e, "could not read sqlite_version()");
            return false;
        }
    };
    let mut parts = version.split('.').map(str::parse::<u32>);
    match (parts.next(), parts.next()) {
        (Some(Ok(major)), Some(Ok(minor))) => (major, minor) >= (3, 27),
        _ => false,
    }
}

async fn vacuum_into(pool: &SqlitePool, dest: &Path) -> KernelResult<()> {
    // Bound, not interpolated: the destination is a filesystem path we build,
    // but SQL string-building around user-controlled data directories is a
    // habit not worth acquiring. VACUUM cannot run inside a transaction, and
    // sqlx does not wrap a bare `execute` in one.
    let dest_str = dest.to_str().ok_or_else(|| {
        KernelError::Other(format!("backup path is not UTF-8: {}", dest.display()))
    })?;
    sqlx::query("VACUUM INTO ?")
        .bind(dest_str)
        .execute(pool)
        .await?;
    Ok(())
}

/// Fallback for a SQLite older than 3.27.
///
/// `wal_checkpoint(TRUNCATE)` blocks until every committed frame has been
/// written back into the main database file and the WAL is emptied, so the
/// copy that follows contains all of them. Any writer that commits in the gap
/// between the checkpoint and the copy lands in a fresh WAL, so that file is
/// copied alongside as `<dest>-wal`: SQLite recovers it on next open, which
/// turns a lost commit into a recovered one.
async fn checkpoint_and_copy(pool: &SqlitePool, db_path: &Path, dest: &Path) -> KernelResult<()> {
    sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
        .execute(pool)
        .await?;
    std::fs::copy(db_path, dest).map_err(|e| io_err("copy database to", dest, &e))?;

    let wal = wal_sidecar(db_path);
    if std::fs::metadata(&wal)
        .map(|m| m.len() > 0)
        .unwrap_or(false)
    {
        let dest_wal = wal_sidecar(dest);
        std::fs::copy(&wal, &dest_wal)
            .map_err(|e| io_err("copy write-ahead log to", &dest_wal, &e))?;
    }
    Ok(())
}

/// Delete the oldest daily backups until only [`DAILY_RETENTION`] remain.
fn prune_daily(dir: &Path) {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) => {
            tracing::warn!(error = %e, dir = %dir.display(), "could not list backups to prune");
            return;
        }
    };
    let mut daily: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| is_daily_backup(path))
        .collect();
    // Chronological, because the date in the name is fixed-width ISO-8601.
    daily.sort();

    let excess = daily.len().saturating_sub(DAILY_RETENTION);
    for old in daily.iter().take(excess) {
        match std::fs::remove_file(old) {
            Ok(()) => tracing::debug!(backup = %old.display(), "pruned old daily backup"),
            Err(e) => tracing::warn!(error = %e, backup = %old.display(), "could not prune backup"),
        }
        // Only present if the checkpoint fallback wrote one.
        let _ = std::fs::remove_file(wal_sidecar(old));
    }
}

fn is_daily_backup(path: &Path) -> bool {
    path.is_file()
        && path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with(DAILY_PREFIX) && name.ends_with(".db"))
}

/// `foo.db` -> `foo.db-wal`, the name SQLite gives the write-ahead log.
fn wal_sidecar(path: &Path) -> PathBuf {
    sidecar(path, "-wal")
}

/// SQLite derives its sidecar names by appending to the database file name,
/// so a rename that does not carry them along breaks the pairing — which is
/// the whole reason [`swap_in`] deletes them rather than leaving them.
fn sidecar(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(suffix);
    PathBuf::from(name)
}

fn create_dir(dir: &Path) -> KernelResult<()> {
    std::fs::create_dir_all(dir).map_err(|e| io_err("create backup directory", dir, &e))
}

fn io_err(what: &str, path: &Path, e: &std::io::Error) -> KernelError {
    KernelError::Other(format!("failed to {what} {}: {e}", path.display()))
}

/// The file name, for a message a person reads. Full paths in a toast are
/// noise; the directory is one "Reveal in Explorer" click away.
fn display_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

// ---------------------------------------------------------------------------
// Listing
// ---------------------------------------------------------------------------

/// Which trigger wrote a backup — the difference matters when choosing one.
///
/// `Unknown` is not a failure. The backup directory is an ordinary folder and
/// someone can legitimately drop a file into it (copied off another machine,
/// or renamed by hand); refusing to list it would make the folder and the UI
/// disagree, and validation is what decides whether a file is usable anyway.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../src/shared/ipc/bindings/")]
#[serde(rename_all = "camelCase")]
pub enum BackupKind {
    Daily,
    PreMigration,
    /// A database displaced by a restore. Listed like any other backup
    /// precisely so that "I restored the wrong one" is itself recoverable.
    Replaced,
    Unknown,
}

/// One file in `backups/`, with the metadata needed to choose between them.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../src/shared/ipc/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct BackupEntry {
    pub name: String,
    /// Absolute, so the UI can hand it straight to "reveal in Explorer".
    pub path: String,
    pub kind: BackupKind,
    /// Modification time in epoch milliseconds.
    ///
    /// Read from the filesystem rather than parsed out of the file name: the
    /// daily name only carries a calendar date, and a name is a claim while
    /// `mtime` is a fact. `0` when the platform will not report one.
    #[ts(type = "number")]
    pub modified_at: i64,
    #[ts(type = "number")]
    pub size: i64,
}

/// Everything the backups UI needs in one round trip: where the folder is,
/// what is in it, and whether a restore is already waiting for a restart.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../src/shared/ipc/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct BackupListing {
    pub dir: String,
    pub entries: Vec<BackupEntry>,
    pub pending: Option<PendingRestore>,
}

/// A restore that has been staged and will apply on the next boot.
///
/// Doubles as the on-disk marker (`restore-pending/RESTORE.json`): the record
/// the UI shows and the record boot acts on are the same bytes, so they cannot
/// describe different things.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../src/shared/ipc/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct PendingRestore {
    /// File name of the backup that will be installed.
    pub source: String,
    #[ts(type = "number")]
    pub requested_at: i64,
    #[ts(type = "number")]
    pub size: i64,
}

/// Every `.db` file in `backups/`, newest first. Never fails: a missing or
/// unreadable directory is an empty list, which is what the UI shows anyway.
pub fn list_backups(db_path: &Path) -> Vec<BackupEntry> {
    let dir = backup_dir(db_path);
    let Ok(read) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut entries: Vec<BackupEntry> = read
        .flatten()
        .filter_map(|entry| backup_entry(&entry.path()))
        .collect();
    // Newest first, name as the tie-break so the order is total and stable —
    // several snapshots can share a modification time to the millisecond.
    entries.sort_by(|a, b| {
        b.modified_at
            .cmp(&a.modified_at)
            .then_with(|| b.name.cmp(&a.name))
    });
    entries
}

fn backup_entry(path: &Path) -> Option<BackupEntry> {
    let name = path.file_name()?.to_str()?.to_owned();
    // `.db` only: this skips the `-wal` the pre-3.27 fallback can leave
    // beside a snapshot, which is part of a backup rather than one itself.
    if !name.ends_with(".db") {
        return None;
    }
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() {
        return None;
    }
    Some(BackupEntry {
        kind: classify(&name),
        modified_at: modified_ms(&meta),
        size: meta.len() as i64,
        path: path.display().to_string(),
        name,
    })
}

fn classify(name: &str) -> BackupKind {
    if name.starts_with(DAILY_PREFIX) {
        BackupKind::Daily
    } else if name.starts_with(PRE_MIGRATION_PREFIX) {
        BackupKind::PreMigration
    } else if name.starts_with(REPLACED_PREFIX) {
        BackupKind::Replaced
    } else {
        BackupKind::Unknown
    }
}

fn modified_ms(meta: &std::fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|since| since.as_millis() as i64)
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

/// The first 16 bytes of every SQLite database file, terminator included.
const SQLITE_MAGIC: &[u8; 16] = b"SQLite format 3\0";

/// Refuse a candidate that is not a healthy DevOS database.
///
/// This runs before anything is staged *and* again at boot before anything is
/// swapped, because the two happen in different processes with an arbitrary
/// gap between them. The order is cheapest-first, and each step catches a
/// failure the next one would report worse:
///
/// 1. **Header** — a file that does not start with the SQLite magic is not a
///    database at all (a text file, a half-downloaded archive, a `.db` that
///    is really a zip). Saying so beats "file is not a database" from sqlx.
/// 2. **Declared size** — the header records the page size and page count, so
///    a truncated file can be caught arithmetically. This is the case worth
///    the extra care: a copy interrupted by a full disk is still a valid
///    SQLite *prefix*, and SQLite will often open it without complaint.
/// 3. **`PRAGMA integrity_check`** — logical corruption inside a file that is
///    structurally the right size.
/// 4. **Is it ours** — a pristine SQLite file from some other application
///    would restore "successfully" and leave DevOS staring at a stranger's
///    tables, with the real database already moved aside. `_sqlx_migrations`
///    exists in every DevOS database that has ever booted.
pub async fn validate_backup(path: &Path) -> KernelResult<()> {
    let name = display_name(path);
    let meta = std::fs::metadata(path)
        .map_err(|e| KernelError::InvalidInput(format!("{name} cannot be read: {e}")))?;
    if !meta.is_file() {
        return Err(KernelError::InvalidInput(format!("{name} is not a file")));
    }
    check_header(path, &name, meta.len())?;
    check_contents(path, &name).await
}

fn check_header(path: &Path, name: &str, len: u64) -> KernelResult<()> {
    use std::io::Read;

    let mut header = [0u8; 100];
    let mut file = std::fs::File::open(path)
        .map_err(|e| KernelError::InvalidInput(format!("{name} cannot be opened: {e}")))?;
    file.read_exact(&mut header).map_err(|_| {
        KernelError::InvalidInput(format!(
            "{name} is {len} bytes — too small to be a SQLite database"
        ))
    })?;

    if &header[..16] != SQLITE_MAGIC {
        return Err(KernelError::InvalidInput(format!(
            "{name} is not a SQLite database: it does not begin with the SQLite file header"
        )));
    }

    // Offset 16: page size, with 1 as the escape for 65536 (it does not fit
    // in the u16 the format allots).
    let page_size = match u16::from_be_bytes([header[16], header[17]]) {
        1 => 65_536u64,
        other => u64::from(other),
    };
    if page_size < 512 || !page_size.is_power_of_two() {
        return Err(KernelError::InvalidInput(format!(
            "{name} declares an impossible page size ({page_size}); the header is damaged"
        )));
    }

    // Offset 28 holds the database size in pages, but only when the file
    // change counter (24) equals the version-valid-for number (92). When they
    // differ the field was written by a SQLite older than 3.7.0 and must be
    // ignored — deriving "truncated" from a stale count would reject a
    // perfectly good backup.
    let change_counter = be_u32(&header[24..28]);
    let valid_for = be_u32(&header[92..96]);
    let pages = u64::from(be_u32(&header[28..32]));
    if pages > 0 && change_counter == valid_for {
        let expected = pages * page_size;
        if len < expected {
            return Err(KernelError::InvalidInput(format!(
                "{name} is truncated: its header describes {pages} pages of {page_size} bytes \
                 ({expected} in total) but the file is only {len} bytes"
            )));
        }
    }
    Ok(())
}

fn be_u32(bytes: &[u8]) -> u32 {
    u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

/// Open the candidate read-only and ask SQLite what it thinks.
///
/// Read-only and single-connection on purpose: validating a file must never
/// create it, migrate it, write a journal beside it, or leave a `-wal` and
/// `-shm` next to a backup that had none.
async fn check_contents(path: &Path, name: &str) -> KernelResult<()> {
    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(false)
        .read_only(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .map_err(|e| {
            KernelError::InvalidInput(format!("{name} could not be opened as a database: {e}"))
        })?;

    let verdict = interrogate(&pool, name).await;
    pool.close().await;
    verdict
}

async fn interrogate(pool: &SqlitePool, name: &str) -> KernelResult<()> {
    let report: Vec<String> = sqlx::query_scalar("PRAGMA integrity_check")
        .fetch_all(pool)
        .await
        .map_err(|e| KernelError::InvalidInput(format!("{name} could not be checked: {e}")))?;
    if report.iter().any(|line| line != "ok") {
        return Err(KernelError::InvalidInput(format!(
            "{name} failed SQLite's integrity check: {}",
            report.join("; ")
        )));
    }

    let devos: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = '_sqlx_migrations'",
    )
    .fetch_one(pool)
    .await
    .map_err(|e| KernelError::InvalidInput(format!("{name} could not be inspected: {e}")))?;
    if devos == 0 {
        return Err(KernelError::InvalidInput(format!(
            "{name} is a healthy SQLite database but not a DevOS one — it has no migration table"
        )));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Staging
// ---------------------------------------------------------------------------

/// Stage `backup` to be installed on the next boot.
///
/// Nothing about the live database changes here — this only writes into
/// `restore-pending/`. The caller is expected to restart the app afterwards;
/// until it does, [`cancel_restore`] undoes this completely.
///
/// The staged file is a *copy*, not a reference to the original. A marker
/// naming a path is a promise about a file somebody can still delete, rename,
/// or overwrite before the restart; a copy inside the staging directory is the
/// thing that will actually be installed, so it is also the thing that gets
/// validated.
pub async fn stage_restore(db_path: &Path, backup: &Path) -> KernelResult<PendingRestore> {
    validate_backup(backup).await?;

    let dir = restore_dir(db_path);
    // Whatever is here is a superseded request. Clearing it first means the
    // directory only ever holds one candidate, and clearing it *before* the
    // marker is written means an interrupted replacement leaves no marker.
    discard_staging(&dir);
    create_dir(&dir)?;

    let staged = dir.join(STAGED_FILE);
    let partial = dir.join(format!("{STAGED_FILE}{PARTIAL_SUFFIX}"));
    std::fs::copy(backup, &partial).map_err(|e| io_err("stage the backup at", &partial, &e))?;

    // Validate the copy, not just the original: a disk that filled up halfway
    // through the copy has to fail here, in front of somebody who can react,
    // rather than at the next boot in front of nobody.
    validate_backup(&partial).await?;
    std::fs::rename(&partial, &staged).map_err(|e| io_err("stage the backup at", &staged, &e))?;

    let pending = PendingRestore {
        source: display_name(backup),
        requested_at: Local::now().timestamp_millis(),
        size: std::fs::metadata(&staged)
            .map(|m| m.len() as i64)
            .unwrap_or(0),
    };
    write_marker(&dir, &pending)?;
    tracing::info!(source = %pending.source, "restore staged; it applies on the next boot");
    Ok(pending)
}

/// The staged restore waiting for a restart, if there is one.
pub fn pending_restore(db_path: &Path) -> Option<PendingRestore> {
    read_marker(&restore_dir(db_path))
}

/// Throw away a staged restore. Safe to call when there is nothing staged.
pub fn cancel_restore(db_path: &Path) -> KernelResult<()> {
    let dir = restore_dir(db_path);
    let marker = dir.join(MARKER_FILE);
    // Marker first, and reported if it fails: while it exists this is a live
    // request, and "cancelled" has to mean the next boot will not act on it.
    match std::fs::remove_file(&marker) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(io_err("remove", &marker, &e)),
    }
    if let Err(e) = std::fs::remove_dir_all(&dir) {
        if e.kind() != std::io::ErrorKind::NotFound {
            // The marker is gone, so the restore is genuinely cancelled; a
            // leftover staged file is wasted disk and nothing more, and the
            // next boot sweeps it.
            tracing::warn!(error = %e, dir = %dir.display(), "staged restore cancelled but not swept");
        }
    }
    Ok(())
}

/// Write the marker through a rename, so it is never half-present.
///
/// A partially written marker is the one failure this feature cannot tolerate:
/// boot would read a truncated JSON object, and every reading of it — apply
/// anyway, refuse, guess — is wrong in a different way.
fn write_marker(dir: &Path, pending: &PendingRestore) -> KernelResult<()> {
    let marker = dir.join(MARKER_FILE);
    let partial = dir.join(format!("{MARKER_FILE}{PARTIAL_SUFFIX}"));
    let json = serde_json::to_vec_pretty(pending)
        .map_err(|e| KernelError::Other(format!("could not encode the restore marker: {e}")))?;
    std::fs::write(&partial, &json).map_err(|e| io_err("write", &partial, &e))?;
    std::fs::rename(&partial, &marker).map_err(|e| io_err("commit", &marker, &e))
}

fn read_marker(dir: &Path) -> Option<PendingRestore> {
    let bytes = std::fs::read(dir.join(MARKER_FILE)).ok()?;
    // A marker that will not parse is treated as no marker at all. That is the
    // safe reading: it means the next boot leaves the database alone.
    serde_json::from_slice(&bytes).ok()
}

/// Remove the marker, then the directory. Marker first, always — it is the
/// only file whose presence means "act on this".
fn discard_staging(dir: &Path) {
    let marker = dir.join(MARKER_FILE);
    if let Err(e) = std::fs::remove_file(&marker) {
        if e.kind() != std::io::ErrorKind::NotFound {
            tracing::warn!(error = %e, "could not clear the restore marker");
        }
    }
    if let Err(e) = std::fs::remove_dir_all(dir) {
        if e.kind() != std::io::ErrorKind::NotFound {
            tracing::warn!(error = %e, dir = %dir.display(), "could not remove the staging directory");
        }
    }
}

// ---------------------------------------------------------------------------
// Applying
// ---------------------------------------------------------------------------

/// What a boot did about a staged restore. Reported to the user as a
/// notification rather than swallowed into the log — a restore is the most
/// consequential thing this app does to itself, and silence is not a report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RestoreOutcome {
    Restored {
        source: String,
        /// Name of the file the previous database was preserved as. `None`
        /// only when there was no database to preserve.
        preserved: Option<String>,
    },
    Refused {
        source: String,
        error: String,
        preserved: Option<String>,
    },
}

impl RestoreOutcome {
    /// `(level, title, body)` for [`Kernel::notify`](crate::Kernel::notify).
    pub fn notification(&self) -> (&'static str, String, String) {
        match self {
            RestoreOutcome::Restored { source, preserved } => (
                "info",
                format!("Database restored from {source}"),
                match preserved {
                    Some(name) => format!(
                        "The database that was in use before the restore is preserved in the \
                         backups folder as {name}. Restore that file to undo this."
                    ),
                    None => "There was no existing database to preserve.".to_string(),
                },
            ),
            RestoreOutcome::Refused {
                source,
                error,
                preserved,
            } => (
                "error",
                format!("Restore from {source} was refused"),
                match preserved {
                    Some(name) => format!(
                        "{error}. The database in use before the attempt is preserved in the \
                         backups folder as {name}."
                    ),
                    None => format!("{error}. Your database was left untouched."),
                },
            ),
        }
    }
}

/// Apply a staged restore, if one is waiting.
///
/// **Must be called before the pool is opened** — this renames the database
/// file, which cannot be done underneath live connections, and every handle
/// obtained from an already-booted kernel would be pointing at the old inode.
/// [`Kernel::boot`](crate::Kernel::boot) calls it as its first act.
///
/// Returns `None` when there was nothing to do. Never returns `Err`: a restore
/// that cannot proceed leaves the database alone and is reported as
/// [`RestoreOutcome::Refused`], because refusing to start is a worse answer
/// than starting on the database the user already had.
///
/// # What survives being killed
///
/// Two files decide everything, and their four combinations are exhaustive:
///
/// | marker | staged | meaning | action |
/// |---|---|---|---|
/// | absent | absent | nothing staged | nothing |
/// | absent | present | staging died before it committed | sweep the debris |
/// | present | present | a committed request | apply it |
/// | present | absent | the swap landed, the cleanup did not | sweep, it is done |
///
/// The last row is the one that makes this safe to interrupt: the staged file
/// stops existing at the instant it becomes the database, so its absence
/// beside a live marker is positive proof the restore completed.
pub async fn apply_pending_restore(db_path: &Path) -> Option<RestoreOutcome> {
    let dir = restore_dir(db_path);
    if !dir.exists() {
        return None;
    }

    let Some(pending) = read_marker(&dir) else {
        tracing::debug!("restore staging directory with no committed marker; sweeping");
        discard_staging(&dir);
        return None;
    };

    let staged = dir.join(STAGED_FILE);
    if !staged.is_file() {
        tracing::info!(
            source = %pending.source,
            "restore had already been applied before the marker was cleared"
        );
        discard_staging(&dir);
        return None;
    }

    let source = pending.source;

    // Re-validate. The staged copy was checked when it was written, but a
    // process died since then and the file has been sitting on a disk that
    // may have filled, a volume that may have been yanked, or a sync client's
    // watch list. Nothing has been touched yet, so refusing here is free.
    if let Err(e) = validate_backup(&staged).await {
        tracing::error!(source = %source, error = %e, "staged restore failed re-validation");
        discard_staging(&dir);
        return Some(RestoreOutcome::Refused {
            source,
            error: e.to_string(),
            preserved: None,
        });
    }

    // Preserve before replacing — the rule the whole feature hangs on. If the
    // current database cannot be copied somewhere safe, the restore does not
    // happen, because the alternative is destroying the state someone may
    // turn out to have needed.
    let preserved = match preserve_current(db_path).await {
        Ok(preserved) => preserved,
        Err(e) => {
            tracing::error!(error = %e, "could not preserve the current database; restore abandoned");
            discard_staging(&dir);
            return Some(RestoreOutcome::Refused {
                source,
                error: format!("the current database could not be preserved first ({e})"),
                preserved: None,
            });
        }
    };

    let outcome = match swap_in(&staged, db_path).await {
        Ok(()) => {
            tracing::info!(source = %source, preserved = ?preserved, "database restored from backup");
            RestoreOutcome::Restored { source, preserved }
        }
        Err(e) => {
            tracing::error!(source = %source, error = %e, "restore failed during the swap");
            RestoreOutcome::Refused {
                source,
                error: e.to_string(),
                preserved,
            }
        }
    };
    discard_staging(&dir);
    Some(outcome)
}

/// Copy the database about to be replaced into `backups/`, returning its name.
///
/// `VACUUM INTO` rather than `fs::copy`, for the same reason the rest of this
/// module uses it and one more: the live file's most recent commits are in
/// `devos.db-wal`, and [`swap_in`] is about to delete that file. A plain copy
/// here would preserve a database missing exactly the work someone is most
/// likely to want back.
///
/// The pool is opened and closed inside this function. That is the point of
/// running before [`Kernel::boot`](crate::Kernel::boot) opens the real one:
/// the rename that follows needs every handle on the file released, and on
/// Windows an open handle makes it fail outright rather than quietly.
async fn preserve_current(db_path: &Path) -> KernelResult<Option<String>> {
    if !db_path.is_file() {
        // Nothing to preserve: a first run, or an earlier attempt that died
        // between the sidecar sweep and the rename. Both are fine — in the
        // second case the preserved copy from that attempt already exists.
        return Ok(None);
    }

    let dir = backup_dir(db_path);
    create_dir(&dir)?;
    let dest = unique_path(
        &dir,
        &format!("{REPLACED_PREFIX}{}", Local::now().format("%Y%m%d-%H%M%S")),
    );

    let pool = crate::db::connect(db_path).await?;
    let written = snapshot(&pool, db_path, &dest).await;
    pool.close().await;
    written?;

    tracing::info!(preserved = %dest.display(), "database preserved before restore");
    Ok(Some(display_name(&dest)))
}

/// `<dir>/<stem>.db`, with `-2`, `-3`… appended until the name is free.
///
/// Two restores inside one second is not a realistic workflow, but
/// `VACUUM INTO` refuses an existing destination, and the failure it would
/// produce aborts a restore for a reason nobody could act on.
fn unique_path(dir: &Path, stem: &str) -> PathBuf {
    let first = dir.join(format!("{stem}.db"));
    if !first.exists() {
        return first;
    }
    for n in 2..1000 {
        let candidate = dir.join(format!("{stem}-{n}.db"));
        if !candidate.exists() {
            return candidate;
        }
    }
    first
}

/// Install the staged file as the database.
///
/// Sidecars go **before** the rename, never after. A `devos.db-wal` left
/// beside a freshly restored `devos.db` is not inert: the next open replays
/// it, which either fails the checksum and corrupts the file or — far worse,
/// because it looks like success — silently reapplies committed frames from
/// the database the user was trying to get away from. The `-shm` is only a
/// shared-memory index and is rebuilt on demand, but it is stale for the same
/// reason and there is no argument for keeping it.
///
/// The rename replaces the destination atomically on both Windows
/// (`MoveFileEx` with `MOVEFILE_REPLACE_EXISTING`) and Unix, which is why the
/// staging directory has to sit on the same filesystem as the database.
///
/// `-shm` goes first even though `-wal` is the dangerous one. If something
/// else still holds the database — a second DevOS process, a SQLite browser —
/// the `-shm` is the file whose removal fails, and failing on it means the
/// `-wal` is still intact for whoever is using it. Deleting the write-ahead
/// log of a database another process has open would take their committed
/// transactions with it.
async fn swap_in(staged: &Path, db_path: &Path) -> KernelResult<()> {
    for suffix in ["-shm", "-wal"] {
        remove_sidecar(&sidecar(db_path, suffix)).await?;
    }
    std::fs::rename(staged, db_path)
        .map_err(|e| io_err("install the restored database at", db_path, &e))
}

/// Delete a sidecar, retrying briefly before believing a failure.
///
/// Windows tears a file's memory mapping down asynchronously: for a few
/// milliseconds after the last SQLite connection closes, deleting `-shm` still
/// fails with *"cannot be performed on a file with a user-mapped section
/// open"*. [`preserve_current`] closes a pool immediately before this runs, so
/// that window is not hypothetical — without the retry, whether a restore
/// works is a coin flip. A genuine holder (another process with the database
/// open) outlives the retries and is reported.
async fn remove_sidecar(path: &Path) -> KernelResult<()> {
    const ATTEMPTS: u32 = 50;
    for attempt in 1..=ATTEMPTS {
        match std::fs::remove_file(path) {
            Ok(()) => {
                tracing::debug!(file = %path.display(), "removed sidecar before restore");
                return Ok(());
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) if attempt == ATTEMPTS => return Err(io_err("remove", path, &e)),
            Err(_) => tokio::time::sleep(std::time::Duration::from_millis(20)).await,
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

    /// A real WAL-mode DevOS database with one extra table for test markers.
    async fn wal_db(dir: &Path) -> (PathBuf, SqlitePool) {
        let db_path = dir.join("devos.db");
        let pool = crate::db::open_pool(&db_path).await.expect("open pool");
        sqlx::query("CREATE TABLE backup_marker (note TEXT NOT NULL)")
            .execute(&pool)
            .await
            .expect("create marker table");
        (db_path, pool)
    }

    async fn commit_marker(pool: &SqlitePool, note: &str) {
        sqlx::query("INSERT INTO backup_marker (note) VALUES (?)")
            .bind(note)
            .execute(pool)
            .await
            .expect("insert marker");
    }

    /// Markers readable from a standalone database file. `Err` for "this file
    /// isn't a usable database", which is what a naive mid-WAL copy produces.
    async fn markers_in(path: &Path) -> Result<Vec<String>, sqlx::Error> {
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(false)
            .read_only(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await?;
        let notes = sqlx::query_scalar("SELECT note FROM backup_marker ORDER BY note")
            .fetch_all(&pool)
            .await;
        pool.close().await;
        notes
    }

    /// Workspace names from a standalone database file — the marker table is
    /// created by hand, but workspaces come through `Kernel::boot`, so this is
    /// what proves a restored file is the one the kernel is now using.
    async fn workspace_names_in(path: &Path) -> Result<Vec<String>, sqlx::Error> {
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(false)
            .read_only(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await?;
        let names = sqlx::query_scalar("SELECT name FROM workspaces ORDER BY name")
            .fetch_all(&pool)
            .await;
        pool.close().await;
        names
    }

    fn daily_backups(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .expect("read backup dir")
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    /// The assumption the whole module rests on: the SQLite sqlx links
    /// against is new enough for `VACUUM INTO`. If this ever fails, the
    /// checkpoint fallback is what runs — and it has its own test below.
    #[tokio::test]
    async fn linked_sqlite_supports_vacuum_into() {
        let dir = tempfile::tempdir().unwrap();
        let (_db_path, pool) = wal_db(dir.path()).await;
        assert!(
            supports_vacuum_into(&pool).await,
            "sqlx 0.8 is expected to bundle SQLite >= 3.27"
        );
    }

    /// The WAL correctness case: a row committed immediately before the
    /// backup must be *in* the backup, even though it is still sitting in
    /// `devos.db-wal` and not in `devos.db` at all.
    #[tokio::test]
    async fn backup_contains_a_row_committed_moments_before_it() {
        let dir = tempfile::tempdir().unwrap();
        let (db_path, pool) = wal_db(dir.path()).await;
        commit_marker(&pool, "committed-just-before-backup").await;

        let backup = run_daily_backup(&pool, &db_path).await.expect("backup");
        assert_eq!(
            markers_in(&backup)
                .await
                .expect("backup is a valid database"),
            vec!["committed-just-before-backup".to_string()],
        );

        // And prove the problem is real rather than hypothetical: the naive
        // "just copy devos.db" backup that VACUUM INTO replaces does not have
        // the row (it is still in the uncheckpointed WAL).
        let naive = dir.path().join("naive.db");
        std::fs::copy(&db_path, &naive).expect("naive copy");
        assert!(
            markers_in(&naive).await.unwrap_or_default().is_empty(),
            "a plain file copy of a WAL database must not be trusted; if this \
             starts passing, the test is no longer proving anything"
        );
    }

    /// Same guarantee, via the pre-3.27 path, so the fallback is not
    /// untested code that only runs on the machines we cannot reproduce.
    #[tokio::test]
    async fn checkpoint_fallback_also_captures_the_latest_commit() {
        let dir = tempfile::tempdir().unwrap();
        let (db_path, pool) = wal_db(dir.path()).await;
        commit_marker(&pool, "in-the-wal").await;

        let dest = dir.path().join("fallback.db");
        checkpoint_and_copy(&pool, &db_path, &dest)
            .await
            .expect("checkpoint + copy");
        assert_eq!(
            markers_in(&dest).await.expect("fallback backup is valid"),
            vec!["in-the-wal".to_string()],
        );
    }

    #[tokio::test]
    async fn daily_rotation_keeps_the_newest_and_deletes_the_oldest() {
        let dir = tempfile::tempdir().unwrap();
        let (db_path, pool) = wal_db(dir.path()).await;
        let backups = backup_dir(&db_path);
        std::fs::create_dir_all(&backups).unwrap();

        // Nine stale dailies, all older than today, plus one unrelated file
        // that rotation must leave alone.
        for day in 1..=9 {
            std::fs::write(
                backups.join(format!("{DAILY_PREFIX}2000-01-0{day}.db")),
                b"stale",
            )
            .unwrap();
        }
        std::fs::write(
            backups.join(format!("{PRE_MIGRATION_PREFIX}20000101-000000-v0001.db")),
            b"keep me",
        )
        .unwrap();

        let today = run_daily_backup(&pool, &db_path).await.expect("backup");

        let names = daily_backups(&backups);
        let dailies: Vec<&String> = names
            .iter()
            .filter(|n| n.starts_with(DAILY_PREFIX))
            .collect();
        assert_eq!(dailies.len(), DAILY_RETENTION, "rotation keeps exactly N");
        assert!(names.iter().any(|n| n.starts_with(PRE_MIGRATION_PREFIX)));
        assert!(today.exists(), "today's backup survives its own pruning");

        // The three oldest went; the ones just above the cut stayed.
        for day in 1..=3 {
            assert!(!backups
                .join(format!("{DAILY_PREFIX}2000-01-0{day}.db"))
                .exists());
        }
        for day in 4..=9 {
            assert!(backups
                .join(format!("{DAILY_PREFIX}2000-01-0{day}.db"))
                .exists());
        }
    }

    #[tokio::test]
    async fn at_most_one_daily_backup_per_calendar_day() {
        let dir = tempfile::tempdir().unwrap();
        let (db_path, pool) = wal_db(dir.path()).await;
        commit_marker(&pool, "before-first-backup").await;

        let first = run_daily_backup(&pool, &db_path).await.expect("backup");

        commit_marker(&pool, "after-first-backup").await;
        assert!(
            run_daily_backup(&pool, &db_path).await.is_none(),
            "second call on the same day must not back up again"
        );
        assert!(run_daily_backup(&pool, &db_path).await.is_none());

        assert_eq!(daily_backups(&backup_dir(&db_path)).len(), 1);
        assert_eq!(
            markers_in(&first).await.expect("backup is valid"),
            vec!["before-first-backup".to_string()],
            "the existing backup was left untouched, not silently rewritten"
        );
    }

    #[tokio::test]
    async fn missing_database_file_is_not_backed_up() {
        let dir = tempfile::tempdir().unwrap();
        let (_db_path, pool) = wal_db(dir.path()).await;

        let absent = dir.path().join("nested").join("gone.db");
        assert!(run_daily_backup(&pool, &absent).await.is_none());
        assert!(
            !backup_dir(&absent).exists(),
            "nothing should be created for a database that isn't there"
        );
    }

    #[tokio::test]
    async fn unwritable_destination_is_swallowed_not_propagated() {
        let dir = tempfile::tempdir().unwrap();
        let (db_path, pool) = wal_db(dir.path()).await;
        // A regular file where the backup directory needs to go: creating it
        // can never succeed, on any platform.
        std::fs::write(backup_dir(&db_path), b"not a directory").unwrap();

        assert!(run_daily_backup(&pool, &db_path).await.is_none());
        let migrator = crate::db::migrator();
        assert!(run_pre_migration_backup(&pool, &db_path, &migrator)
            .await
            .is_none());

        // The database itself is untouched and still usable.
        commit_marker(&pool, "still-working").await;
    }

    /// Requirement 4 end to end: a broken backup path must not stop the app.
    #[tokio::test]
    async fn boot_succeeds_when_backups_cannot_be_written() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("devos.db");
        std::fs::write(backup_dir(&db_path), b"not a directory").unwrap();

        let kernel = crate::Kernel::boot(&db_path)
            .await
            .expect("boot must survive a failing backup");
        assert_eq!(
            crate::repo::list_workspaces(&kernel.pool)
                .await
                .unwrap()
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn pre_migration_backup_skipped_on_first_run_and_when_up_to_date() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("devos.db");
        let migrator = crate::db::migrator();

        // Fresh file, nothing recorded in `_sqlx_migrations` yet.
        let options = SqliteConnectOptions::new()
            .filename(&db_path)
            .create_if_missing(true);
        let pool = SqlitePoolOptions::new()
            .connect_with(options)
            .await
            .unwrap();
        assert!(
            run_pre_migration_backup(&pool, &db_path, &migrator)
                .await
                .is_none(),
            "a brand new database has nothing worth preserving"
        );

        // Now fully migrated: nothing pending, so nothing to snapshot either.
        migrator.run(&pool).await.unwrap();
        assert!(
            run_pre_migration_backup(&pool, &db_path, &migrator)
                .await
                .is_none(),
            "every-boot copies are the daily rotation's job"
        );
        assert!(!backup_dir(&db_path).exists());
    }

    #[tokio::test]
    async fn pre_migration_backup_runs_when_a_migration_is_pending() {
        let dir = tempfile::tempdir().unwrap();
        let (db_path, pool) = wal_db(dir.path()).await;
        commit_marker(&pool, "pre-existing-data").await;

        // Rewrite history so the embedded migrations look one step ahead of
        // what this database has applied — the upgrade case, without needing
        // a second migration file that would then have to exist forever.
        let migrator = crate::db::migrator();
        let earliest = migrator.iter().map(|m| m.version).min().unwrap();
        sqlx::query("UPDATE _sqlx_migrations SET version = version - ? WHERE version = ?")
            .bind(1000)
            .bind(earliest)
            .execute(&pool)
            .await
            .unwrap();

        let backup = run_pre_migration_backup(&pool, &db_path, &migrator)
            .await
            .expect("pending migration must trigger a backup");
        assert!(backup
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with(PRE_MIGRATION_PREFIX));
        assert_eq!(
            markers_in(&backup).await.expect("backup is valid"),
            vec!["pre-existing-data".to_string()],
            "the snapshot is of the database as it stood before migrating"
        );
    }

    // -----------------------------------------------------------------------
    // Restore
    // -----------------------------------------------------------------------

    /// A real DevOS database opened through `Kernel::boot`, plus one extra
    /// workspace so the file has content that is recognisably *this* one.
    async fn booted(dir: &Path, workspace: &str) -> (PathBuf, crate::Kernel) {
        let db_path = dir.join("devos.db");
        let kernel = crate::Kernel::boot(&db_path).await.expect("boot");
        crate::repo::create_workspace(&kernel.pool, workspace)
            .await
            .expect("create workspace");
        (db_path, kernel)
    }

    /// Shut a kernel down the way process exit would, so the file is free for
    /// the next boot to rename. Windows will not rename a file with an open
    /// handle, which is exactly the constraint that put restore at boot.
    ///
    /// The wait is the part a real restart gets for free: closing the pool
    /// makes SQLite drop the sidecars, but Windows unmaps `-shm` on its own
    /// schedule, and a test that plants a fake one before that lands is
    /// testing the OS rather than the code.
    async fn shut_down(kernel: crate::Kernel, db_path: &Path) {
        kernel.pool.close().await;
        drop(kernel);
        for _ in 0..200 {
            if !sidecar(db_path, "-shm").exists() {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        panic!("SQLite never released {}-shm", db_path.display());
    }

    /// A backup of the database *as it is now*, in `backups/`, under a daily
    /// name from a past date.
    ///
    /// Not `run_daily_backup`: `Kernel::boot` already took today's, so a
    /// second call the same day correctly returns `None` — and today's was
    /// taken before the test made the state it wants to restore to.
    async fn snapshot_now(pool: &SqlitePool, db_path: &Path, day: &str) -> PathBuf {
        let dir = backup_dir(db_path);
        std::fs::create_dir_all(&dir).unwrap();
        let dest = dir.join(format!("{DAILY_PREFIX}{day}.db"));
        snapshot(pool, db_path, &dest).await.expect("snapshot");
        dest
    }

    fn names_in_backup_dir(db_path: &Path) -> Vec<String> {
        list_backups(db_path)
            .into_iter()
            .map(|entry| entry.name)
            .collect()
    }

    #[tokio::test]
    async fn listing_reports_every_kind_with_real_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let (db_path, pool) = wal_db(dir.path()).await;
        commit_marker(&pool, "listed").await;

        // One of each kind, written the way the app writes them.
        let daily = run_daily_backup(&pool, &db_path).await.expect("daily");
        let backups = backup_dir(&db_path);
        let premigration = backups.join(format!("{PRE_MIGRATION_PREFIX}20240102-030405-v0001.db"));
        std::fs::copy(&daily, &premigration).unwrap();
        let replaced = backups.join(format!("{REPLACED_PREFIX}20240102-030406.db"));
        std::fs::copy(&daily, &replaced).unwrap();
        // Two files rotation and listing must both ignore: a WAL sidecar the
        // pre-3.27 fallback can leave behind, and something unrelated.
        std::fs::write(wal_sidecar(&daily), b"sidecar").unwrap();
        std::fs::write(backups.join("notes.txt"), b"not a backup").unwrap();
        // ...and one file that is none of the three names but is still a `.db`.
        let strange = backups.join("from-the-other-laptop.db");
        std::fs::copy(&daily, &strange).unwrap();

        let entries = list_backups(&db_path);
        let by_name = |name: &str| {
            entries
                .iter()
                .find(|e| e.name == name)
                .unwrap_or_else(|| panic!("{name} missing from the listing"))
                .clone()
        };

        assert_eq!(entries.len(), 4, "one entry per .db file, and nothing else");
        assert_eq!(
            by_name(daily.file_name().unwrap().to_str().unwrap()).kind,
            BackupKind::Daily
        );
        assert_eq!(
            by_name(premigration.file_name().unwrap().to_str().unwrap()).kind,
            BackupKind::PreMigration
        );
        assert_eq!(
            by_name(replaced.file_name().unwrap().to_str().unwrap()).kind,
            BackupKind::Replaced
        );
        assert_eq!(
            by_name("from-the-other-laptop.db").kind,
            BackupKind::Unknown,
            "a file somebody dropped in is listed, not hidden — validation is \
             what decides whether it can be used"
        );

        for entry in &entries {
            assert!(entry.size > 0, "{} has a real size", entry.name);
            assert!(entry.modified_at > 0, "{} has a real mtime", entry.name);
            assert!(Path::new(&entry.path).is_file(), "path points at the file");
        }

        // Newest first: the three copies were written after the daily.
        let ordered: Vec<i64> = entries.iter().map(|e| e.modified_at).collect();
        let mut sorted = ordered.clone();
        sorted.sort_by(|a, b| b.cmp(a));
        assert_eq!(ordered, sorted, "newest first");
    }

    #[tokio::test]
    async fn listing_a_missing_backup_directory_is_empty_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        assert!(list_backups(&dir.path().join("devos.db")).is_empty());
    }

    /// The four ways a candidate is not a database anyone should restore.
    /// Each would otherwise replace a working database with rubbish.
    #[tokio::test]
    async fn validation_refuses_everything_that_is_not_a_healthy_devos_database() {
        let dir = tempfile::tempdir().unwrap();
        let (db_path, pool) = wal_db(dir.path()).await;
        commit_marker(&pool, "real").await;
        let good = run_daily_backup(&pool, &db_path).await.expect("backup");
        validate_backup(&good)
            .await
            .expect("a real backup validates");

        // 1. Not a database at all — and long enough that it is rejected for
        //    its content rather than for being short.
        let text = dir.path().join("notes.db");
        std::fs::write(&text, "a text file with a misleading extension\n".repeat(8)).unwrap();
        let err = validate_backup(&text).await.unwrap_err().to_string();
        assert!(
            err.contains("not a SQLite database"),
            "message should say what is wrong, got: {err}"
        );

        // 2. Empty — too small even for a header.
        let empty = dir.path().join("empty.db");
        std::fs::write(&empty, b"").unwrap();
        let err = validate_backup(&empty).await.unwrap_err().to_string();
        assert!(err.contains("too small"), "got: {err}");

        // 3. Truncated: a valid SQLite prefix, which is what an interrupted
        //    copy leaves and what SQLite is most likely to open regardless.
        let truncated = dir.path().join("truncated.db");
        let bytes = std::fs::read(&good).unwrap();
        assert!(bytes.len() > 4096, "the fixture needs several pages");
        std::fs::write(&truncated, &bytes[..bytes.len() / 2]).unwrap();
        let err = validate_backup(&truncated).await.unwrap_err().to_string();
        assert!(
            err.contains("truncated"),
            "a half file must be named as truncated, got: {err}"
        );

        // 4. A perfectly healthy SQLite database belonging to something else.
        let foreign = dir.path().join("someone-elses.db");
        {
            let options = SqliteConnectOptions::new()
                .filename(&foreign)
                .create_if_missing(true);
            let other = SqlitePoolOptions::new()
                .connect_with(options)
                .await
                .unwrap();
            sqlx::query("CREATE TABLE contacts (name TEXT)")
                .execute(&other)
                .await
                .unwrap();
            other.close().await;
        }
        let err = validate_backup(&foreign).await.unwrap_err().to_string();
        assert!(
            err.contains("not a DevOS one"),
            "restoring a stranger's database is the quiet disaster, got: {err}"
        );

        // Nothing above left a journal or sidecar beside the file it inspected.
        for candidate in [&good, &foreign] {
            assert!(!wal_sidecar(candidate).exists());
            assert!(!sidecar(candidate, "-shm").exists());
        }
    }

    #[tokio::test]
    async fn staging_refuses_a_bad_candidate_and_creates_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("devos.db");
        let junk = dir.path().join("junk.db");
        std::fs::write(&junk, b"nope").unwrap();

        assert!(stage_restore(&db_path, &junk).await.is_err());
        assert!(
            !restore_dir(&db_path).exists(),
            "a refused candidate must not leave a staging directory that a \
             later boot would have to reason about"
        );
        assert!(pending_restore(&db_path).is_none());
    }

    /// The whole feature, end to end and through `Kernel::boot`: the restored
    /// rows are readable *and* the database that was replaced is preserved.
    #[tokio::test]
    async fn restoring_installs_the_backup_and_preserves_what_it_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let (db_path, kernel) = booted(dir.path(), "In the backup").await;
        let backup = snapshot_now(&kernel.pool, &db_path, "2024-05-05").await;

        // Work done after the backup. This is the state a restore destroys,
        // and therefore the state that has to survive somewhere.
        crate::repo::create_workspace(&kernel.pool, "After the backup")
            .await
            .unwrap();
        shut_down(kernel, &db_path).await;

        let pending = stage_restore(&db_path, &backup).await.expect("stage");
        assert_eq!(pending.source, display_name(&backup));
        assert_eq!(
            pending_restore(&db_path).as_ref(),
            Some(&pending),
            "what the UI reads back is the marker boot will act on"
        );

        // The restart.
        let kernel = crate::Kernel::boot(&db_path).await.expect("boot");
        let names: Vec<String> = crate::repo::list_workspaces(&kernel.pool)
            .await
            .unwrap()
            .into_iter()
            .map(|w| w.name)
            .collect();
        assert!(names.contains(&"In the backup".to_string()));
        assert!(
            !names.contains(&"After the backup".to_string()),
            "the restore did not take effect"
        );
        assert!(
            pending_restore(&db_path).is_none() && !restore_dir(&db_path).exists(),
            "an applied restore must not apply a second time"
        );

        // ...and the replaced database is still on disk, under a name that
        // says what it is, holding exactly the work the restore undid.
        let replaced: Vec<String> = names_in_backup_dir(&db_path)
            .into_iter()
            .filter(|n| n.starts_with(REPLACED_PREFIX))
            .collect();
        assert_eq!(
            replaced.len(),
            1,
            "one preserved database, got {replaced:?}"
        );
        let preserved = backup_dir(&db_path).join(&replaced[0]);
        let preserved_names = workspace_names_in(&preserved)
            .await
            .expect("the preserved database is a usable database");
        assert!(
            preserved_names.contains(&"After the backup".to_string()),
            "the preserved copy must hold the work the restore threw away, \
             including commits that were still in the WAL"
        );
        // And it is offered back through the same list the restore came from.
        assert!(list_backups(&db_path)
            .iter()
            .any(|e| e.name == replaced[0] && e.kind == BackupKind::Replaced));
        shut_down(kernel, &db_path).await;
    }

    /// The restore is reported where a person will see it, with the name of
    /// the preserved database — the one fact needed to undo a mistaken restore.
    #[tokio::test]
    async fn a_restore_announces_itself_and_names_the_preserved_database() {
        let dir = tempfile::tempdir().unwrap();
        let (db_path, kernel) = booted(dir.path(), "Original").await;
        let backup = snapshot_now(&kernel.pool, &db_path, "2024-05-05").await;
        shut_down(kernel, &db_path).await;
        stage_restore(&db_path, &backup).await.expect("stage");

        let kernel = crate::Kernel::boot(&db_path).await.expect("boot");
        let notifications = crate::repo::list_notifications(&kernel.pool, 10)
            .await
            .unwrap();
        let reported = notifications
            .iter()
            .find(|n| n.module == "backup")
            .expect("a restore must produce a notification");
        assert_eq!(reported.level, "info");
        assert!(reported.title.contains(&display_name(&backup)));

        let preserved = names_in_backup_dir(&db_path)
            .into_iter()
            .find(|n| n.starts_with(REPLACED_PREFIX))
            .expect("preserved database");
        assert!(
            reported
                .body
                .as_deref()
                .unwrap_or_default()
                .contains(&preserved),
            "the notification has to name the file, or the preserved database \
             is only findable by someone who already knows the naming scheme"
        );
        shut_down(kernel, &db_path).await;
    }

    /// A `-wal` left beside a restored database is replayed on the next open:
    /// at best it fails a checksum, at worst it silently reapplies frames from
    /// the database that was just replaced. Both sidecars must be gone.
    #[tokio::test]
    async fn sidecars_do_not_survive_the_swap() {
        let dir = tempfile::tempdir().unwrap();
        let (db_path, kernel) = booted(dir.path(), "In the backup").await;
        let backup = snapshot_now(&kernel.pool, &db_path, "2024-05-05").await;
        shut_down(kernel, &db_path).await;

        // A clean shutdown checkpoints and deletes the sidecars, so plant them
        // back: the case that matters is a database whose process was killed.
        std::fs::write(wal_sidecar(&db_path), b"stale write-ahead log").unwrap();
        std::fs::write(sidecar(&db_path, "-shm"), b"stale shared index").unwrap();

        stage_restore(&db_path, &backup).await.expect("stage");
        let outcome = apply_pending_restore(&db_path).await.expect("applied");
        assert!(matches!(outcome, RestoreOutcome::Restored { .. }));

        assert!(
            !wal_sidecar(&db_path).exists(),
            "a stale -wal beside the restored database is the corruption path"
        );
        assert!(!sidecar(&db_path, "-shm").exists());
        assert!(
            !restore_dir(&db_path).exists(),
            "the staged copy stops existing at the moment it becomes the database"
        );
        // The restored file is usable on its own, with no sidecar to recover.
        assert!(workspace_names_in(&db_path)
            .await
            .expect("restored database opens")
            .contains(&"In the backup".to_string()));
    }

    /// Interrupt #1: killed after the staged copy landed but before the marker
    /// was committed. There is no request, so nothing happens — and the debris
    /// does not sit there being ambiguous forever.
    #[tokio::test]
    async fn a_stage_that_never_committed_is_swept_and_never_applied() {
        let dir = tempfile::tempdir().unwrap();
        let (db_path, kernel) = booted(dir.path(), "Untouched").await;
        let backup = snapshot_now(&kernel.pool, &db_path, "2024-05-05").await;
        crate::repo::create_workspace(&kernel.pool, "Live work")
            .await
            .unwrap();
        shut_down(kernel, &db_path).await;

        stage_restore(&db_path, &backup).await.expect("stage");
        // Rewind to the instant before the commit: staged file, no marker.
        std::fs::remove_file(restore_dir(&db_path).join(MARKER_FILE)).unwrap();

        assert!(
            apply_pending_restore(&db_path).await.is_none(),
            "no marker, no restore"
        );
        assert!(!restore_dir(&db_path).exists(), "the debris is swept");

        let kernel = crate::Kernel::boot(&db_path).await.expect("boot");
        let names: Vec<String> = crate::repo::list_workspaces(&kernel.pool)
            .await
            .unwrap()
            .into_iter()
            .map(|w| w.name)
            .collect();
        assert!(
            names.contains(&"Live work".to_string()),
            "the database must be exactly as it was"
        );
        assert!(
            names_in_backup_dir(&db_path)
                .iter()
                .all(|n| !n.starts_with(REPLACED_PREFIX)),
            "nothing was replaced, so nothing should have been preserved"
        );
        shut_down(kernel, &db_path).await;
    }

    /// Interrupt #2: killed after the swap but before the marker was cleared.
    /// The staged file is gone because it *became* the database, so a marker
    /// without one is proof the restore finished — not a reason to redo it.
    #[tokio::test]
    async fn a_marker_left_after_the_swap_does_not_restore_twice() {
        let dir = tempfile::tempdir().unwrap();
        let (db_path, kernel) = booted(dir.path(), "Restored already").await;
        let backup = snapshot_now(&kernel.pool, &db_path, "2024-05-05").await;
        shut_down(kernel, &db_path).await;

        stage_restore(&db_path, &backup).await.expect("stage");
        // Rewind to the instant after the rename: marker, no staged file.
        std::fs::remove_file(restore_dir(&db_path).join(STAGED_FILE)).unwrap();

        assert!(
            apply_pending_restore(&db_path).await.is_none(),
            "an already-applied restore is not applied again"
        );
        assert!(!restore_dir(&db_path).exists());
        assert!(
            names_in_backup_dir(&db_path)
                .iter()
                .all(|n| !n.starts_with(REPLACED_PREFIX)),
            "a second preservation pass would be a second copy of the same \
             database and a second confusing entry in the list"
        );
        assert!(workspace_names_in(&db_path)
            .await
            .expect("database untouched")
            .contains(&"Restored already".to_string()));
    }

    /// Interrupt #3: killed part-way through writing the marker itself. A
    /// half-written request is read as no request, which is the only answer
    /// that cannot destroy anything.
    #[tokio::test]
    async fn a_half_written_marker_is_not_a_request() {
        let dir = tempfile::tempdir().unwrap();
        let (db_path, kernel) = booted(dir.path(), "Untouched").await;
        let backup = snapshot_now(&kernel.pool, &db_path, "2024-05-05").await;
        shut_down(kernel, &db_path).await;

        stage_restore(&db_path, &backup).await.expect("stage");
        let marker = restore_dir(&db_path).join(MARKER_FILE);
        let json = std::fs::read_to_string(&marker).unwrap();
        std::fs::write(&marker, &json[..json.len() / 2]).unwrap();

        assert!(
            pending_restore(&db_path).is_none(),
            "the UI must not describe a request boot will refuse to act on"
        );
        assert!(apply_pending_restore(&db_path).await.is_none());
        assert!(!restore_dir(&db_path).exists());
        assert!(workspace_names_in(&db_path)
            .await
            .expect("database untouched")
            .contains(&"Untouched".to_string()));
    }

    /// The rule that outranks restoring: if the current database cannot be
    /// preserved, it is not replaced. A restore that eats the state someone is
    /// recovering *from* is worse than a restore that did not happen.
    #[tokio::test]
    async fn a_restore_that_cannot_preserve_the_current_database_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let (db_path, kernel) = booted(dir.path(), "Only copy").await;
        // Snapshot somewhere other than `backups/`, which is about to become
        // unusable.
        let backup = dir.path().join("hand-made.db");
        snapshot(&kernel.pool, &db_path, &backup).await.unwrap();
        shut_down(kernel, &db_path).await;

        stage_restore(&db_path, &backup).await.expect("stage");
        // A regular file where the backup directory needs to go: preserving
        // can never succeed here, on any platform. (Boot already created the
        // directory, hence the removal first.)
        std::fs::remove_dir_all(backup_dir(&db_path)).unwrap();
        std::fs::write(backup_dir(&db_path), b"not a directory").unwrap();

        match apply_pending_restore(&db_path).await {
            Some(RestoreOutcome::Refused { preserved, .. }) => assert_eq!(preserved, None),
            other => panic!("expected a refusal, got {other:?}"),
        }
        assert!(
            !restore_dir(&db_path).exists(),
            "a refused request is dropped rather than retried on every boot"
        );

        // The database still opens, still has its data, and still has boot.
        let kernel = crate::Kernel::boot(&db_path).await.expect("boot");
        let names: Vec<String> = crate::repo::list_workspaces(&kernel.pool)
            .await
            .unwrap()
            .into_iter()
            .map(|w| w.name)
            .collect();
        assert!(names.contains(&"Only copy".to_string()));
        shut_down(kernel, &db_path).await;
    }

    #[tokio::test]
    async fn staging_again_replaces_the_previous_request() {
        let dir = tempfile::tempdir().unwrap();
        let (db_path, kernel) = booted(dir.path(), "First").await;
        let first = snapshot_now(&kernel.pool, &db_path, "2024-05-05").await;
        let second = backup_dir(&db_path).join(format!("{DAILY_PREFIX}1999-01-01.db"));
        std::fs::copy(&first, &second).unwrap();
        shut_down(kernel, &db_path).await;

        stage_restore(&db_path, &first).await.expect("stage first");
        let latest = stage_restore(&db_path, &second)
            .await
            .expect("stage second");

        assert_eq!(pending_restore(&db_path), Some(latest));
        assert_eq!(
            pending_restore(&db_path).unwrap().source,
            display_name(&second),
            "the newest request wins; there is only ever one"
        );
        let staged: Vec<String> = std::fs::read_dir(restore_dir(&db_path))
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(staged.len(), 2, "exactly the staged file and the marker");
    }

    #[tokio::test]
    async fn cancelling_removes_the_request_entirely() {
        let dir = tempfile::tempdir().unwrap();
        let (db_path, kernel) = booted(dir.path(), "Kept").await;
        let backup = snapshot_now(&kernel.pool, &db_path, "2024-05-05").await;
        shut_down(kernel, &db_path).await;

        stage_restore(&db_path, &backup).await.expect("stage");
        cancel_restore(&db_path).expect("cancel");

        assert!(pending_restore(&db_path).is_none());
        assert!(!restore_dir(&db_path).exists());
        assert!(apply_pending_restore(&db_path).await.is_none());
        // Cancelling nothing is not an error — the UI can call it freely.
        cancel_restore(&db_path).expect("cancelling twice is fine");
    }
}
