//! Screen capture and the scratch directory the PNGs live in.
//!
//! Capture is blocking — it talks to the display driver and then encodes
//! several megabytes of PNG — so nothing in here is `async`. The command
//! layer puts it on a blocking thread; doing it on the async runtime would
//! stall every other IPC call for the duration.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use xcap::Monitor;

use crate::{CapturedShot, IssueError, IssueResult};

/// How many PNGs survive a capture.
///
/// **One**, and the reason is worth stating rather than tuning. This file is
/// the *raw, unredacted, full-resolution desktop* — the annotator loads it,
/// paints redactions into a canvas, and exports a flattened copy. Redaction is
/// destructive in the export; it does nothing to this file. A security review
/// found the previous value of 20 kept nineteen unredacted desktops on disk as
/// plain PNGs with default ACLs, which is exactly what a file-level backup or
/// a sync client picks up, and exactly where nobody looks when investigating
/// how a key got out.
///
/// One means the directory holds only the capture currently being worked on.
/// The remaining exposure is that window, and closing it properly means
/// deleting the source once the flattened export exists — see
/// `docs/security.md`.
pub const KEEP_SHOTS: usize = 1;

/// Subdirectory of the app data directory. One place knows this name, so
/// pruning cannot drift from writing.
const SHOTS_DIR: &str = "screenshots";

const PREFIX: &str = "shot-";
const SUFFIX: &str = ".png";

/// Capture the primary monitor, write it as a PNG under `app_data`, and prune
/// the directory back to [`KEEP_SHOTS`].
pub fn capture_primary(app_data: &Path) -> IssueResult<CapturedShot> {
    let monitors = Monitor::all().map_err(|e| IssueError::Capture(e.to_string()))?;
    // `is_primary` is itself fallible per monitor; a monitor that will not say
    // is simply not the primary one, and if none claims it the first is a
    // better answer than an error.
    let monitor = monitors
        .iter()
        .find(|monitor| monitor.is_primary().unwrap_or(false))
        .or_else(|| monitors.first())
        .ok_or_else(|| IssueError::Capture("no monitor to capture".into()))?;

    let image = monitor
        .capture_image()
        .map_err(|e| IssueError::Capture(e.to_string()))?;
    let (width, height) = image.dimensions();

    let dir = shots_dir(app_data);
    fs::create_dir_all(&dir)?;
    let captured_at = now_millis();
    let path = shot_path(&dir, captured_at);
    image
        .save(&path)
        .map_err(|e| IssueError::Capture(e.to_string()))?;

    // After the write, so the shot just taken counts as one of the newest.
    prune(&dir, KEEP_SHOTS)?;

    Ok(CapturedShot {
        path: path.display().to_string(),
        width,
        height,
        captured_at,
    })
}

fn shots_dir(app_data: &Path) -> PathBuf {
    app_data.join(SHOTS_DIR)
}

/// Named by capture time, which makes the newest shot identifiable from the
/// filename alone — pruning then needs no `mtime` call, and a file copied or
/// restored into the directory keeps its place in the order.
fn shot_path(dir: &Path, captured_at: i64) -> PathBuf {
    dir.join(format!("{PREFIX}{captured_at}{SUFFIX}"))
}

/// Delete all but the `keep` newest shots. Only files this module named are
/// considered, so anything else the user has put in the directory survives —
/// a prune that deletes by "oldest file here" would be a small file shredder
/// pointed at a path the user can change.
fn prune(dir: &Path, keep: usize) -> IssueResult<()> {
    let mut shots: Vec<(i64, PathBuf)> = fs::read_dir(dir)?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            let stamp = path
                .file_name()?
                .to_str()?
                .strip_prefix(PREFIX)?
                .strip_suffix(SUFFIX)?
                .parse::<i64>()
                .ok()?;
            Some((stamp, path))
        })
        .collect();

    // Newest first, so "keep the newest `keep`" is a `skip`.
    shots.sort_unstable_by_key(|(stamp, _)| std::cmp::Reverse(*stamp));
    for (_, path) in shots.into_iter().skip(keep) {
        // A file that vanished under us is already in the state we wanted.
        let _ = fs::remove_file(path);
    }
    Ok(())
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_millis() as i64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Nothing here captures the screen. A test that did would depend on a
    // display being attached and on the OS granting capture, so it would fail
    // in CI and on a locked workstation for reasons unrelated to this code.
    // What is testable — where the file goes, what it is called, and which
    // files pruning is willing to delete — is tested instead.

    fn write_shot(dir: &Path, stamp: i64) {
        fs::write(shot_path(dir, stamp), b"not really a png").unwrap();
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn a_shot_is_named_by_capture_time_under_the_screenshots_directory() {
        let dir = shots_dir(Path::new("C:/app-data"));
        assert!(dir.ends_with("screenshots"));

        let path = shot_path(&dir, 1_754_400_000_123);
        assert_eq!(
            path.file_name().unwrap().to_string_lossy(),
            "shot-1754400000123.png"
        );
        assert_eq!(path.parent().unwrap(), dir);
    }

    #[test]
    fn pruning_keeps_exactly_the_newest_shot() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        // 25 shots a second apart, written oldest-first so creation order and
        // timestamp order agree — pruning must follow the timestamp either way.
        for i in 0..25 {
            write_shot(dir, 1_754_400_000_000 + i * 1_000);
        }
        assert_eq!(names(dir).len(), 25);

        prune(dir, KEEP_SHOTS).unwrap();

        let kept = names(dir);
        assert_eq!(kept.len(), KEEP_SHOTS, "kept: {kept:?}");
        assert!(
            kept.contains(&"shot-1754400024000.png".to_string()),
            "the newest shot must survive: {kept:?}"
        );
        // Derived from KEEP_SHOTS rather than hardcoded, so changing the
        // retention changes what this asserts instead of silently passing.
        let oldest_kept = 1_754_400_000_000 + (25 - KEEP_SHOTS as u64) * 1_000;
        assert!(
            kept.contains(&format!("shot-{oldest_kept}.png")),
            "the oldest surviving shot is off: {kept:?}"
        );
        assert!(
            !kept.contains(&format!("shot-{}.png", oldest_kept - 1_000)),
            "the shot past the window must be gone: {kept:?}"
        );
        assert!(
            !kept.contains(&"shot-1754400000000.png".to_string()),
            "the oldest must be gone: {kept:?}"
        );
    }

    #[test]
    fn pruning_a_directory_at_the_limit_deletes_nothing() {
        let temp = tempfile::tempdir().unwrap();
        // Exactly at the limit rather than a fixed count: with KEEP_SHOTS at
        // 1 there is no "under" to test, and hardcoding a number would have
        // this pass vacuously the next time the retention changes.
        for i in 0..KEEP_SHOTS as i64 {
            write_shot(temp.path(), 1_754_400_000_000 + i);
        }

        prune(temp.path(), KEEP_SHOTS).unwrap();

        assert_eq!(names(temp.path()).len(), KEEP_SHOTS);
    }

    #[test]
    fn pruning_an_empty_directory_is_not_an_error() {
        let temp = tempfile::tempdir().unwrap();
        prune(temp.path(), KEEP_SHOTS).unwrap();
        assert!(names(temp.path()).is_empty());
    }

    #[test]
    fn pruning_never_touches_a_file_this_module_did_not_write() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        for i in 0..25 {
            write_shot(dir, 1_754_400_000_000 + i * 1_000);
        }
        // Everything below is old enough to be pruned if the filter went by
        // age alone rather than by name.
        for stranger in [
            "notes.txt",
            "shot-not-a-number.png",
            "shot-1754400000000.jpg",
            "screenshot-1754400000000.png",
            "1754400000000.png",
        ] {
            fs::write(dir.join(stranger), b"someone else's file").unwrap();
        }
        fs::create_dir(dir.join("shot-1754400000001.png.d")).unwrap();

        prune(dir, KEEP_SHOTS).unwrap();

        let kept = names(dir);
        assert_eq!(
            kept.len(),
            KEEP_SHOTS + 6,
            "20 shots plus five strangers and a directory: {kept:?}"
        );
        for stranger in [
            "notes.txt",
            "shot-not-a-number.png",
            "shot-1754400000000.jpg",
            "screenshot-1754400000000.png",
            "1754400000000.png",
            "shot-1754400000001.png.d",
        ] {
            assert!(
                kept.contains(&stranger.to_string()),
                "{stranger} was deleted"
            );
        }
    }

    #[test]
    fn the_capture_timestamp_is_epoch_milliseconds() {
        let now = now_millis();
        // Sometime after 2020 and before 2100 — enough to catch seconds or
        // nanoseconds sneaking into the field the frontend formats as a date.
        assert!(now > 1_577_836_800_000, "{now} is not epoch milliseconds");
        assert!(now < 4_102_444_800_000, "{now} is not epoch milliseconds");
    }
}
