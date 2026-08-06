# 0008 — Background watchers run in-process and notify on state transitions

Status: accepted
Date: 2026-08-06

## Context

M4's website monitor has to check sites on a clock, indefinitely, and tell
the user when something changes. DevOS's only prior watcher — the OSC 133
terminal failure watcher (M2) — is event-driven: it reacts to output the
user's own shell produced, so it is necessarily live exactly when the thing
it watches is. An uptime monitor has no such luck. It needs a timer, and
the useful version of it needs to be running when nobody is looking.

Two decisions follow, and they are separable: where the schedule lives, and
what one check turns into for the user.

## Decision

**In-process.** A tokio task starts at boot alongside the module, ticks
every ~15 seconds, selects enabled monitors whose newest recorded check is
older than their `interval_secs`, performs those checks, and writes the
results to `monitor_checks`. No second binary, no OS scheduler entry, no
daemon.

**Notify on transitions only.** After recording a result the scheduler
compares it with the monitor's previous state. `ok → fail` raises a
`warning` through `Kernel::notify`; `fail → ok` raises an `info`. An
unchanged state raises nothing. The current state of every monitor is
always on the `/monitors` page; the bell is for changes.

## Alternatives considered

- **An OS-scheduled headless checker** (Task Scheduler / cron driving a
  small companion binary) — the only alternative here that actually closes
  the app-closed hole, and it lost on cost rather than merit. It means a
  second executable to build, sign, and update; a second writer against
  `devos.db` (WAL tolerates it, but "one process owns the database" is an
  assumption every module currently enjoys); an install step that registers
  a scheduled task; and a path for results to get back into the running
  app. That is a milestone, not a feature.
- **A tray or autostart daemon that owns the checking, with the app as a
  viewer** — same benefit, and it also changes what DevOS *is*. Today it is
  an application the user opens and closes. A daemon is something that runs
  on their machine permanently and has to be trusted, updated, and stopped
  independently of the window.
- **Hosted checking** — the only option that gives real uptime monitoring:
  results that survive a laptop lid closing, and multi-region checks that a
  single developer machine cannot produce at all. It is also DevOS's first
  server dependency, and it contradicts the local-first posture the app has
  held throughout (strict CSP, no remote code, everything in one SQLite
  file). Not rejected on merit — rejected as a different product decision,
  one that should be made deliberately rather than smuggled in under "add a
  monitor."
- **Notify on every check** — one line of code, and genuinely worse. At the
  60-second interval floor an overnight outage is hundreds of identical
  bell entries, which teaches the user to ignore the bell — including for
  the terminal failure watcher, which shares it.
- **Threshold or flap damping** (notify after N consecutive failures, or
  when 24h uptime crosses a percentage) — better than what shipped for
  targets that flap, but it is a refinement *of* transition detection
  rather than an alternative to it; the state comparison is needed either
  way. Shipped at N = 1 with no tuning knob. Raising N is a local change
  inside the scheduler, not a redesign.

## Consequences

- **Monitoring stops when DevOS is closed.** This is the significant one
  and it should be read without softening: this is not UptimeRobot or
  Pingdom. Checks run while the developer's tool is open, which for most
  people means working hours on one machine. Closing the app stops them.
- Therefore `uptimePct` is uptime **across the checks that actually ran**,
  not true uptime. Gaps are absent rather than counted as failures — the
  number is honest about what was observed and silent about what wasn't,
  and it is biased toward the hours the user is at their desk.
- An outage that begins *and* ends while the app is closed produces no
  notification at all. Monitor state is read back from the newest stored
  check, so at the next launch the previous stored check is `ok`, the fresh
  check is `ok`, and there is no transition to report. The failing checks
  that would have caught it were never performed.
- Timing is quantized to the tick, so a 60-second monitor fires every
  60–75 seconds. Fine for "is the site answering"; useless as a latency SLA
  instrument. There is no catch-up either — a monitor whose interval
  elapsed while the app was closed is checked once on the next tick, not
  backfilled. There is nothing to backfill.
- A monitor that stays broken goes quiet after its first warning. Anyone
  reading only the bell will not be reminded that a site is still down; the
  `/monitors` page is the surface that always shows current state. That is
  the deliberate price of not being noisy.
- Notification volume is decoupled from check volume, so the interval can
  be tuned for freshness without also tuning how irritating the app is.
- Cheap in every other respect: one tokio task, no extra process, no
  elevated install step, no cross-process IPC, and the scheduler lives
  inside the module that owns the tables it writes.
- **To monitor while the app is closed**, one of two things has to happen,
  and neither is a patch. Either the check moves behind a headless
  companion process the OS starts — which turns `monitors` /
  `monitor_checks` into a shared boundary and forces a real answer about
  who owns `devos.db` — or checking moves to a hosted service and DevOS
  becomes its client. The module boundary helps in both cases, since the
  check itself is a pure function of a `Monitor` row, but scheduling,
  storage, and the notification path all get re-answered.
