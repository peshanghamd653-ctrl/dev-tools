//! A long-lived view of the machine.
//!
//! CPU usage is the single easy thing to get wrong here. sysinfo derives it
//! from the *delta* between two refreshes, so a freshly constructed `System`
//! always reports 0% — and refreshing twice back to back does not help
//! either, because the kernel's counters need at least
//! [`MINIMUM_CPU_UPDATE_INTERVAL`] to move. Building a `System` per call
//! therefore pins CPU usage at zero forever, which looks like working code.
//!
//! So the `System` lives here for the process lifetime: it is sampled once at
//! construction, and every snapshot measures against the previous sample. A
//! snapshot asked for sooner than the minimum interval sleeps out the
//! remainder rather than returning a zero it knows to be false.

use std::sync::Mutex;
use std::time::Instant;

use sysinfo::{Disks, ProcessesToUpdate, System, MINIMUM_CPU_UPDATE_INTERVAL};

use crate::{DiskInfo, ProcessInfo, SystemSnapshot};

/// How many of the busiest processes a snapshot carries.
const TOP_PROCESSES: usize = 5;

struct Probe {
    system: System,
    disks: Disks,
    /// When the last CPU sample was taken. The next one has to be at least
    /// [`MINIMUM_CPU_UPDATE_INTERVAL`] later to mean anything.
    sampled_at: Instant,
}

/// The shared metrics source. Held in `AppState` behind an `Arc`.
pub struct SystemProbe {
    inner: Mutex<Probe>,
}

impl Default for SystemProbe {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemProbe {
    /// Build the probe and take the baseline the first snapshot compares
    /// against.
    pub fn new() -> Self {
        let mut system = System::new();
        system.refresh_cpu_all();
        system.refresh_memory();
        system.refresh_processes(ProcessesToUpdate::All, true);
        Self {
            inner: Mutex::new(Probe {
                system,
                disks: Disks::new_with_refreshed_list(),
                sampled_at: Instant::now(),
            }),
        }
    }

    /// A fresh snapshot.
    ///
    /// Blocking on purpose: refreshing sysinfo is synchronous work, and when
    /// the previous sample is too recent this waits out the remainder of the
    /// minimum interval. Callers on an async runtime hand it to
    /// `spawn_blocking`.
    pub fn snapshot(&self) -> SystemSnapshot {
        let mut probe = self.inner.lock().expect("system probe poisoned");

        // `checked_sub` is `None` once enough time has passed, which is the
        // common case — a UI polling every few seconds never sleeps here.
        if let Some(remaining) = MINIMUM_CPU_UPDATE_INTERVAL.checked_sub(probe.sampled_at.elapsed())
        {
            std::thread::sleep(remaining);
        }

        probe.system.refresh_cpu_all();
        probe.system.refresh_memory();
        probe.system.refresh_processes(ProcessesToUpdate::All, true);
        // Disks carry no delta, so a plain refresh is enough; `true` drops
        // volumes that have been unmounted since the last look.
        probe.disks.refresh(true);
        probe.sampled_at = Instant::now();

        let disks: Vec<DiskInfo> = probe
            .disks
            .list()
            .iter()
            .map(|disk| DiskInfo {
                name: disk.name().to_string_lossy().into_owned(),
                mount: disk.mount_point().to_string_lossy().into_owned(),
                total: disk.total_space() as i64,
                available: disk.available_space() as i64,
            })
            .collect();

        let mut top_processes: Vec<ProcessInfo> = probe
            .system
            .processes()
            .values()
            .map(|process| ProcessInfo {
                pid: process.pid().as_u32() as i64,
                name: process.name().to_string_lossy().into_owned(),
                cpu_usage: process.cpu_usage(),
                memory: process.memory() as i64,
            })
            .collect();
        top_processes.sort_by(|a, b| b.cpu_usage.total_cmp(&a.cpu_usage));
        top_processes.truncate(TOP_PROCESSES);

        SystemSnapshot {
            cpu_usage: probe.system.global_cpu_usage(),
            cpu_cores: probe.system.cpus().len() as u32,
            mem_total: probe.system.total_memory() as i64,
            mem_used: probe.system.used_memory() as i64,
            swap_total: probe.system.total_swap() as i64,
            swap_used: probe.system.used_swap() as i64,
            uptime_secs: System::uptime() as i64,
            disks,
            top_processes,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_reports_plausible_hardware() {
        let snapshot = SystemProbe::new().snapshot();

        assert!(snapshot.cpu_cores > 0, "a machine has at least one core");
        assert!(snapshot.mem_total > 0, "a machine has memory");
        assert!(
            snapshot.mem_used > 0 && snapshot.mem_used <= snapshot.mem_total,
            "used {} of total {}",
            snapshot.mem_used,
            snapshot.mem_total
        );
        assert!(snapshot.swap_used <= snapshot.swap_total);
        assert!(snapshot.uptime_secs > 0);
        assert!(snapshot.cpu_usage >= 0.0 && snapshot.cpu_usage <= 100.0);

        // Disks are deliberately not asserted non-empty: a container or a
        // locked-down CI image can legitimately expose none.
        for disk in &snapshot.disks {
            assert!(!disk.mount.is_empty(), "a listed disk has a mount point");
            assert!(
                disk.available <= disk.total,
                "{} reports {} available of {}",
                disk.mount,
                disk.available,
                disk.total
            );
        }
    }

    #[test]
    fn top_processes_are_capped_and_ordered_by_cpu() {
        let snapshot = SystemProbe::new().snapshot();

        assert!(!snapshot.top_processes.is_empty(), "this test is a process");
        assert!(snapshot.top_processes.len() <= TOP_PROCESSES);
        for pair in snapshot.top_processes.windows(2) {
            assert!(
                pair[0].cpu_usage >= pair[1].cpu_usage,
                "busiest first: {:?}",
                snapshot.top_processes
            );
        }
        for process in &snapshot.top_processes {
            assert!(process.pid > 0);
            assert!(process.memory >= 0);
        }
    }

    #[test]
    fn cpu_usage_is_not_stuck_at_zero_across_snapshots() {
        // The regression this guards: a probe that rebuilds `System` per call
        // reports 0% forever, and so does one that refreshes twice inside the
        // minimum sampling interval. Burn a core between the two snapshots so
        // there is real movement for the second one to find.
        let probe = SystemProbe::new();
        let first = probe.snapshot();

        let spin_until = Instant::now() + MINIMUM_CPU_UPDATE_INTERVAL * 2;
        let mut burn: u64 = 0;
        while Instant::now() < spin_until {
            burn = burn.wrapping_add(1);
        }
        assert!(burn > 0, "the spin loop must not be optimized away");

        let second = probe.snapshot();
        assert!(
            second.cpu_usage > 0.0,
            "CPU usage stuck at zero across snapshots (first {}, second {})",
            first.cpu_usage,
            second.cpu_usage
        );
    }
}
