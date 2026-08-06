/**
 * WebdriverIO config for DevOS end-to-end smoke tests.
 *
 * This drives the *real* Tauri application: `@wdio/tauri-service` spawns
 * `tauri-driver`, which in turn spawns `msedgedriver.exe` to attach to the
 * WebView2 instance inside the app window. The Rust backend is running for
 * real, so `inDesktopShell` is true and IPC commands actually execute.
 *
 * The run is hermetic with respect to the database: `DEVOS_DATA_DIR` (see
 * `src-tauri/src/lib.rs`) is pointed at a fresh directory under the OS temp
 * dir, so every run boots an empty `devos.db` through the first-run path and
 * the developer's real `%APPDATA%\com.peshang.devos\devos.db` is never opened.
 * It is *not* hermetic in every other respect: the WebView2 profile (and thus
 * `localStorage`) and the OS keystore entry holding the secrets master key are
 * still the machine's real ones.
 *
 * Prerequisites (all checked/automated below except the first two):
 *   1. `cargo install tauri-driver --locked`   (once, ~5 min)
 *   2. A built app binary. By default the *debug* binary is used:
 *        cargo build -p devos-desktop
 *      A debug binary loads its frontend from the Vite dev server, so this
 *      config starts `pnpm dev` on :1420 automatically and shuts it down after.
 *      To test a self-contained release build instead:
 *        pnpm tauri build --no-bundle
 *        DEVOS_E2E_BIN=target/release/devos-desktop.exe pnpm e2e
 *      (release binaries embed `dist/`, so no dev server is started).
 *   3. msedgedriver.exe matching the installed WebView2 runtime — the service
 *      detects the version and downloads a match on first run.
 *
 * Environment knobs:
 *   DEVOS_E2E_BIN        path to the binary under test (default: debug build)
 *   DEVOS_E2E_KEEP_DATA  set to 1 to keep the run's database for inspection
 *                        instead of deleting it (the path is printed either way)
 *
 * Run with: pnpm e2e
 */
import { spawn, spawnSync } from "node:child_process";
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readdirSync,
  rmSync,
  statSync,
} from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(here, "..");

const appBinary = process.env.DEVOS_E2E_BIN
  ? path.resolve(repoRoot, process.env.DEVOS_E2E_BIN)
  : path.join(repoRoot, "target", "debug", "devos-desktop.exe");

/** Release builds embed `dist/`; debug builds point at Vite's dev server. */
const needsDevServer = !appBinary
  .toLowerCase()
  .includes(`${path.sep}release${path.sep}`);

const DEV_SERVER_URL = "http://localhost:1420";

/**
 * Parent of every run's throwaway data directory.
 *
 * A named parent rather than bare `mkdtemp` in `os.tmpdir()` so that orphans
 * are identifiable and sweepable: `onComplete` does not run if the process is
 * killed (Ctrl-C, a CI cancellation), and without the sweep below those
 * directories would accumulate silently.
 */
const DATA_ROOT = path.join(os.tmpdir(), "devos-e2e");

/**
 * Only sweep orphans older than this. A shorter window risks deleting the data
 * directory out from under a concurrently running suite; this one is long
 * enough that no live run can be caught by it and short enough that leftovers
 * do not survive a working day.
 */
const STALE_AFTER_MS = 2 * 60 * 60 * 1000;

const keepData = process.env.DEVOS_E2E_KEEP_DATA === "1";

/** @type {string | null} */
let dataDir = null;

/** @type {import('node:child_process').ChildProcess | null} */
let devServer = null;

/**
 * Vite's entry module. Fetched as part of the readiness check on purpose.
 *
 * Serving `index.html` proves only that the HTTP server is listening; Vite
 * will happily do that while dependency pre-bundling is still running, and it
 * then blocks every module request until that finishes. On a cold cache
 * (always, in CI) that gap is tens of seconds, and paying it here — where the
 * budget is a 90s loop — rather than inside the 'React shell never mounted'
 * wait is the difference between a slow run and a red one.
 */
const DEV_SERVER_ENTRY = `${DEV_SERVER_URL}/src/main.tsx`;

async function devServerIsUp() {
  try {
    const page = await fetch(DEV_SERVER_URL, {
      signal: AbortSignal.timeout(1500),
    });
    if (!page.ok) return false;
    // Generous relative to the 1.5s above: this is the request that waits on
    // pre-bundling, so a timeout here means "not ready yet", not "broken".
    const entry = await fetch(DEV_SERVER_ENTRY, {
      signal: AbortSignal.timeout(30_000),
    });
    return entry.ok;
  } catch {
    return false;
  }
}

/** Delete data directories left behind by runs that never reached onComplete. */
function sweepStaleDataDirs() {
  if (!existsSync(DATA_ROOT)) return;
  const cutoff = Date.now() - STALE_AFTER_MS;
  for (const entry of readdirSync(DATA_ROOT)) {
    const candidate = path.join(DATA_ROOT, entry);
    try {
      if (statSync(candidate).mtimeMs < cutoff) {
        rmSync(candidate, { recursive: true, force: true });
      }
    } catch {
      // Raced with another process, or a file is still locked. A leftover temp
      // directory is not worth failing a test run over.
    }
  }
}

/**
 * Remove this run's data directory.
 *
 * `maxRetries` matters: the app is killed moments earlier and Windows keeps
 * `devos.db-wal`/`-shm` handles open briefly after the process dies, so a
 * single `rmSync` loses the race often enough to be annoying. Node retries
 * EBUSY/EPERM synchronously, which is what `onComplete` requires.
 *
 * Failure here warns rather than throws: the run's actual result must not be
 * overwritten by a cleanup problem, and the next run's sweep will catch it.
 */
function removeDataDir() {
  if (!dataDir) return;
  if (keepData) {
    console.log(`[e2e] DEVOS_E2E_KEEP_DATA=1 — keeping ${dataDir}`);
    dataDir = null;
    return;
  }
  try {
    rmSync(dataDir, {
      recursive: true,
      force: true,
      maxRetries: 10,
      retryDelay: 250,
    });
  } catch (err) {
    console.warn(`[e2e] could not remove ${dataDir}: ${err}`);
  }
  dataDir = null;
}

export const config = {
  runner: "local",
  specs: [path.join(here, "specs", "**", "*.e2e.js")],
  maxInstances: 1,

  capabilities: [
    {
      browserName: "tauri",
      "tauri:options": {
        application: appBinary,
      },
    },
  ],

  services: [
    [
      "@wdio/tauri-service",
      {
        // The embedded provider (the service default) needs
        // `tauri-plugin-wdio-webdriver` compiled into src-tauri. We do not
        // want a test-only plugin in the shipping binary, so drive the
        // external `tauri-driver` + msedgedriver pair instead.
        driverProvider: "external",
        autoInstallTauriDriver: false,
        autoDownloadEdgeDriver: true,
        startTimeout: 90_000,
      },
    ],
  ],

  logLevel: "warn",
  bail: 0,
  waitforTimeout: 15_000,
  connectionRetryTimeout: 120_000,
  connectionRetryCount: 2,

  framework: "mocha",
  reporters: ["spec"],
  mochaOpts: {
    ui: "bdd",
    // Must stay comfortably above the longest `waitUntil` in the suite (the
    // 120s first-mount wait in the root `before` hook): if Mocha's timeout
    // fires first the failure reads as a generic timeout instead of naming
    // what never happened.
    timeout: 180_000,
  },

  async onPrepare() {
    if (!existsSync(appBinary)) {
      throw new Error(
        `DevOS binary not found at ${appBinary}\n` +
          `Build it first:  cargo build -p devos-desktop\n` +
          `(or point DEVOS_E2E_BIN at another build).`,
      );
    }

    // Isolate the database before anything can start the app.
    //
    // This must happen in `onPrepare` and it must happen *here* rather than in
    // a spec: WDIO runs the config's own onPrepare before any launcher
    // service's, so `tauri-driver` — and therefore the app process below it —
    // inherits the variable. Specs run in forked workers that also inherit it,
    // which is how `smoke.e2e.js` can assert the database really landed here.
    //
    // Any DEVOS_DATA_DIR the developer already had set is deliberately
    // overridden for this process tree only; their shell is untouched.
    sweepStaleDataDirs();
    mkdirSync(DATA_ROOT, { recursive: true });
    dataDir = mkdtempSync(path.join(DATA_ROOT, "run-"));
    process.env.DEVOS_DATA_DIR = dataDir;
    console.log(`[e2e] isolated app data: ${dataDir}`);

    if (!needsDevServer) return;

    if (await devServerIsUp()) {
      console.log(`[e2e] reusing dev server already on ${DEV_SERVER_URL}`);
      return;
    }

    console.log("[e2e] starting Vite dev server…");
    devServer = spawn("pnpm", ["dev"], {
      cwd: repoRoot,
      shell: true,
      stdio: "ignore",
    });

    const deadline = Date.now() + 90_000;
    while (Date.now() < deadline) {
      if (await devServerIsUp()) {
        console.log("[e2e] dev server ready");
        return;
      }
      await new Promise((r) => setTimeout(r, 500));
    }
    throw new Error(
      `Vite dev server never became reachable on ${DEV_SERVER_URL}`,
    );
  },

  onComplete() {
    // Everything here must be synchronous: WDIO exits the process as soon as
    // onComplete settles, and an async spawn would be orphaned before it ran.

    if (devServer?.pid) {
      // `pnpm dev` runs under a cmd.exe shell with node beneath it — kill the tree.
      spawnSync("taskkill", ["/PID", String(devServer.pid), "/T", "/F"], {
        stdio: "ignore",
      });
      devServer = null;
    }

    // @wdio/tauri-service does not always reap tauri-driver on Windows, and a
    // stray tauri-driver keeps its msedgedriver child (and a locked WebView2
    // user-data dir) alive, which makes the *next* run hang. Killing the
    // tauri-driver tree takes msedgedriver with it. Safe because nothing else
    // on the machine runs tauri-driver, and this suite is single-instance.
    spawnSync("taskkill", ["/IM", "tauri-driver.exe", "/T", "/F"], {
      stdio: "ignore",
    });

    // Last, so the app under test is already gone and its file handles with
    // it. Note the developer's own DevOS instance, if one is open, is never
    // killed — it was never the process holding these files.
    removeDataDir();
  },
};
