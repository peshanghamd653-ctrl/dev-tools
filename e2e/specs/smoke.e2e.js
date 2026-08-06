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
 */
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
  await browser.waitUntil(
    async () => (await textOf("aside")) !== null,
    { timeout: 60_000, timeoutMsg: "React shell never mounted" },
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
