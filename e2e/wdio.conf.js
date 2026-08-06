/**
 * WebdriverIO config for DevOS end-to-end smoke tests.
 *
 * This drives the *real* Tauri application: `@wdio/tauri-service` spawns
 * `tauri-driver`, which in turn spawns `msedgedriver.exe` to attach to the
 * WebView2 instance inside the app window. The Rust backend is running for
 * real, so `inDesktopShell` is true and IPC commands actually execute.
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
 * Run with: pnpm e2e
 */
import { spawn, spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
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

/** @type {import('node:child_process').ChildProcess | null} */
let devServer = null;

async function devServerIsUp() {
  try {
    const res = await fetch(DEV_SERVER_URL, {
      signal: AbortSignal.timeout(1500),
    });
    return res.ok;
  } catch {
    return false;
  }
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
    timeout: 120_000,
  },

  async onPrepare() {
    if (!existsSync(appBinary)) {
      throw new Error(
        `DevOS binary not found at ${appBinary}\n` +
          `Build it first:  cargo build -p devos-desktop\n` +
          `(or point DEVOS_E2E_BIN at another build).`,
      );
    }

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
  },
};
