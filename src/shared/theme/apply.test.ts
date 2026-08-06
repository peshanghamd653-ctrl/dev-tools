import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  applyThemeClass,
  DARK_SCHEME_QUERY,
  prefersDark,
  toasterTheme,
  watchSystemAppearance,
} from "./apply";
import { resolveTheme, SYSTEM_THEME } from "./themes";

/**
 * A fake MediaQueryList. Chrome's own `prefers-color-scheme` emulation flips
 * `matches` without dispatching `change`, so the live-follow path cannot be
 * exercised in a real browser — it gets pinned here instead.
 */
function fakeQuery(matches: boolean, { legacy = false } = {}) {
  const listeners = new Set<(e: MediaQueryListEvent) => void>();
  const query = {
    matches,
    media: DARK_SCHEME_QUERY,
    addEventListener: legacy
      ? undefined
      : vi.fn((_: string, cb: (e: MediaQueryListEvent) => void) =>
          listeners.add(cb),
        ),
    removeEventListener: legacy
      ? undefined
      : vi.fn((_: string, cb: (e: MediaQueryListEvent) => void) =>
          listeners.delete(cb),
        ),
    addListener: legacy
      ? vi.fn((cb: (e: MediaQueryListEvent) => void) => listeners.add(cb))
      : undefined,
    removeListener: legacy
      ? vi.fn((cb: (e: MediaQueryListEvent) => void) => listeners.delete(cb))
      : undefined,
  };
  const emit = (next: boolean) => {
    query.matches = next;
    for (const cb of listeners) cb({ matches: next } as MediaQueryListEvent);
  };
  return { query, emit, listeners };
}

function stubMatchMedia(query: unknown) {
  const matchMedia = vi.fn().mockReturnValue(query);
  vi.stubGlobal("matchMedia", matchMedia);
  return matchMedia;
}

describe("watchSystemAppearance", () => {
  beforeEach(() => vi.unstubAllGlobals());

  it("reports live changes to prefers-color-scheme", () => {
    const { query, emit } = fakeQuery(true);
    stubMatchMedia(query);

    const seen: boolean[] = [];
    watchSystemAppearance((dark) => seen.push(dark));

    emit(false);
    emit(true);
    expect(seen).toEqual([false, true]);
  });

  it("drives the resolved theme when the preference is 'system'", () => {
    const { query, emit } = fakeQuery(true);
    stubMatchMedia(query);

    let systemDark = prefersDark();
    watchSystemAppearance((dark) => (systemDark = dark));
    expect(resolveTheme("system", systemDark)).toBe(SYSTEM_THEME.dark);

    emit(false);
    expect(resolveTheme("system", systemDark)).toBe(SYSTEM_THEME.light);

    emit(true);
    expect(resolveTheme("system", systemDark)).toBe(SYSTEM_THEME.dark);
  });

  it("leaves an explicit choice alone when the OS flips", () => {
    const { query, emit } = fakeQuery(true);
    stubMatchMedia(query);

    let systemDark = prefersDark();
    watchSystemAppearance((dark) => (systemDark = dark));
    emit(false);

    expect(resolveTheme("obsidian", systemDark)).toBe("obsidian");
    expect(resolveTheme("midnight", systemDark)).toBe("midnight");
  });

  it("unsubscribes", () => {
    const { query, emit, listeners } = fakeQuery(true);
    stubMatchMedia(query);

    const seen: boolean[] = [];
    const stop = watchSystemAppearance((dark) => seen.push(dark));
    emit(false);
    stop();
    emit(true);

    expect(seen).toEqual([false]);
    expect(listeners.size).toBe(0);
  });

  it("falls back to addListener on webviews without addEventListener", () => {
    const { query, emit, listeners } = fakeQuery(true, { legacy: true });
    stubMatchMedia(query);

    const seen: boolean[] = [];
    const stop = watchSystemAppearance((dark) => seen.push(dark));
    emit(false);
    expect(seen).toEqual([false]);

    stop();
    expect(listeners.size).toBe(0);
  });

  it("is a no-op where matchMedia does not exist", () => {
    vi.stubGlobal("matchMedia", undefined);
    expect(() => watchSystemAppearance(() => {})()).not.toThrow();
  });
});

describe("applyThemeClass", () => {
  it("targets an explicit root when given one", () => {
    const el = document.createElement("html");
    el.classList.add("theme-daylight");
    applyThemeClass("obsidian", el);
    expect([...el.classList]).toEqual(["theme-obsidian"]);
  });
});

describe("toasterTheme", () => {
  it("maps each theme to the appearance sonner expects", () => {
    expect(toasterTheme("midnight")).toBe("dark");
    expect(toasterTheme("obsidian")).toBe("dark");
    expect(toasterTheme("daylight")).toBe("light");
  });
});
