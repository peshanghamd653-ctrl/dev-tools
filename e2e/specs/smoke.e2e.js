/**
 * Boot + navigation smoke test.
 *
 * Scope is deliberately narrow: this proves the app *starts* and that the
 * shell renders and routes — the failure mode no unit test can reach, because
 * unit tests never build the Tauri binary, never run `lib.rs` `setup()`, and
 * never open a WebView2 window.
 *
 * Deliberately NOT covered here: terminal, git, AI, Docker, DB interaction.
 * Those are slow, stateful, and depend on the machine (a real repo, a running
 * daemon, an API key), which would make this suite flaky. They are covered by
 * Rust unit tests instead.
 *
 * The app boots against a throwaway database — see `DEVOS_DATA_DIR` in
 * `wdio.conf.js` — so everything below runs against genuine first-run state.
 */
import { existsSync } from "node:fs";
import os from "node:os";
import path from "node:path";

import { browser, expect, $ } from "@wdio/globals";

const NAV_LABELS = [
  "Dashboard",
  "Projects",
  "Terminal",
  "Git",
  "AI Assistant",
  "Files",
  "Docker",
  "API Client",
  "Database",
  "Monitors",
  "Deployments",
  "Snippets",
  "Settings",
];

/**
 * Read text out of the page in a single round trip.
 *
 * WebdriverIO charges a full driver round trip per `$`/`getText`, so pulling
 * a dozen nav labels one element at a time is a dozen round trips. One
 * `execute` is one.
 */
function textsOf(selector) {
  return browser.execute(
    (sel) => [...document.querySelectorAll(sel)].map((el) => el.textContent),
    selector,
  );
}

function textOf(selector) {
  return browser.execute(
    (sel) => document.querySelector(sel)?.textContent ?? null,
    selector,
  );
}

/** Wait for the `<h1>` that every page renders at the top of `<main>`. */
function waitForHeading(expected) {
  return browser.waitUntil(
    async () => (await textOf("main h1")) === expected,
    { timeout: 20_000, timeoutMsg: `"${expected}" heading never appeared` },
  );
}

/**
 * Click a sidebar nav item by its visible label.
 *
 * XPath rather than WDIO's `a=Label` text selector: the link also contains a
 * `<kbd>` with the shortcut, so its full text is "ProjectsCtrl+2" and an exact
 * text match never hits. (WDIO also rejects `aside nav a=Label` outright —
 * a CSS path cannot be combined with the `=text` form.)
 */
async function clickNav(label) {
  const link = await $(
    `//aside//nav//a[.//span[normalize-space(text())="${label}"]]`,
  );
  await link.waitForClickable({ timeout: 20_000 });
  await link.click();
}

before(async () => {
  // DevOS is single-window, but @wdio/tauri-service still runs a focus-recovery
  // probe before every element command, and that probe asks a Tauri plugin we
  // deliberately don't ship — so it fails slowly, every time (~10s per command).
  // An explicit switchToWindow marks the window as user-selected, which turns
  // the probe off for the rest of the session.
  const [handle] = await browser.getWindowHandles();
  await browser.switchToWindow(handle);

  // The bundle is served by Vite on first hit; give it room to mount.
  //
  // 120s is not padding for a slow assertion — it is the ceiling on a cold
  // Vite transforming the whole module graph one request at a time inside
  // WebView2, which is what a CI runner does every single time. Observed at
  // ~5s warm and comfortably over 60s cold on a loaded machine. This wait is
  // the suite's only real source of flakiness, so it is sized for the worst
  // case; a healthy run never spends it.
  await browser.waitUntil(
    async () => (await textOf("aside")) !== null,
    { timeout: 120_000, timeoutMsg: "React shell never mounted" },
  );
});

describe("DevOS boots", () => {
  it("opens a window and mounts the React shell", async () => {
    const handles = await browser.getWindowHandles();
    expect(handles.length).toBeGreaterThan(0);

    // #root is where main.tsx mounts; if the bundle threw, this stays empty.
    const rootChildren = await browser.execute(
      () => document.querySelector("#root")?.childElementCount ?? 0,
    );
    expect(rootChildren).toBeGreaterThan(0);

    const sidebar = await $("aside");
    await expect(sidebar).toHaveText(expect.stringContaining("DevOS"));
  });

  it("is running inside the real desktop shell, not a bare browser", async () => {
    // Two independent proofs that the Rust side is attached:
    //
    // 1. The IPC bridge exists at all.
    const hasBridge = await browser.execute(
      () => typeof window.__TAURI_INTERNALS__ !== "undefined",
    );
    expect(hasBridge).toBe(true);

    // 2. The kernel answered a real `invoke`: DashboardPage prints this line
    //    only when `inDesktopShell` is true, and the version stat below is
    //    filled from the `app_info` command, which round-trips through Rust.
    await browser.waitUntil(
      async () => {
        const main = await textOf("main");
        return main?.includes("Your development operating center") ?? false;
      },
      { timeout: 30_000, timeoutMsg: "dashboard never reported desktop shell" },
    );

    await browser.waitUntil(
      async () => {
        const stats = await textsOf("main .grid p");
        return stats.some((t) => /^\d+\.\d+\.\d+$/.test(t.trim()));
      },
      { timeout: 30_000, timeoutMsg: "app_info version stat never populated" },
    );
  });

  /**
   * Isolation, asserted rather than assumed.
   *
   * Two halves, because either alone is weak. The filesystem half proves a
   * database was created under the temp directory the config handed the app
   * (if `DEVOS_DATA_DIR` failed to propagate through tauri-driver, no file
   * appears there). The UI half proves that *this* is the database the running
   * app is answering IPC from: a fresh kernel boot creates exactly one
   * workspace and no projects, so the dashboard must show the first-run empty
   * state. A run that leaked into `%APPDATA%` fails the second half on any
   * machine where DevOS has ever been used.
   */
  it("runs against an isolated, first-run database", async () => {
    const dataDir = process.env.DEVOS_DATA_DIR;
    expect(dataDir).toBeTruthy();
    expect(dataDir.startsWith(os.tmpdir())).toBe(true);
    expect(existsSync(path.join(dataDir, "devos.db"))).toBe(true);

    await browser.waitUntil(
      async () => (await textOf("main"))?.includes("No projects yet") ?? false,
      {
        timeout: 30_000,
        timeoutMsg: "dashboard did not show first-run empty state",
      },
    );

    // Each stat card renders "<value>" then "<label>" as sibling <p>s, so the
    // count sits one index before its label. Polled rather than read once:
    // "No projects yet" also renders while the workspace query is still in
    // flight, and an in-flight card shows an em dash.
    await browser.waitUntil(
      async () => {
        const stats = await textsOf("main .grid p");
        const label = stats.findIndex((t) => t.trim() === "Workspaces");
        // Exactly one: the default "Personal" workspace a fresh kernel boot
        // creates. Anything else means this is not a first-run database.
        return label > 0 && stats[label - 1].trim() === "1";
      },
      {
        timeout: 30_000,
        timeoutMsg: "workspace count was never exactly 1 on a fresh database",
      },
    );
  });

  it("renders every primary nav entry", async () => {
    const labels = await textsOf("aside nav a");
    for (const expected of NAV_LABELS) {
      expect(labels.some((l) => l.includes(expected))).toBe(true);
    }
  });
});

describe("DevOS navigates", () => {
  beforeEach(async () => {
    await clickNav("Dashboard");
    // The dashboard's <h1> is the active workspace name, not a fixed string,
    // so key off a card title that is stable.
    await browser.waitUntil(
      async () => (await textOf("main"))?.includes("Quick actions") ?? false,
      { timeout: 20_000, timeoutMsg: "never returned to the dashboard" },
    );
  });

  it("routes to Projects from the sidebar", async () => {
    await clickNav("Projects");
    await waitForHeading("Projects");
  });

  it("routes to Settings from the sidebar", async () => {
    await clickNav("Settings");
    await waitForHeading("Settings");
  });

  it("routes to Docker via the command palette", async () => {
    await browser.keys(["Control", "k"]);

    const input = await $("[cmdk-input]");
    await input.waitForDisplayed({ timeout: 20_000 });
    await input.setValue("Docker");

    const item = await $('(//*[@cmdk-item][contains(., "Docker")])[1]');
    await item.waitForClickable({ timeout: 20_000 });
    await item.click();

    await waitForHeading("Docker");
  });
});
