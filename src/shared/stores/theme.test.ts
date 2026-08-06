import { beforeEach, describe, expect, it, vi } from "vitest";

import { useThemeStore } from "./theme";
import { applyThemeClass, prefersDark } from "@/shared/theme/apply";
import {
  DEFAULT_THEME,
  resolveTheme,
  SYSTEM_THEME,
  THEME_STORAGE_KEY,
  themeClass,
  type ThemePreference,
} from "@/shared/theme/themes";

const initialState = useThemeStore.getState();

function stored(): ThemePreference | undefined {
  const raw = localStorage.getItem(THEME_STORAGE_KEY);
  return raw ? JSON.parse(raw).state?.preference : undefined;
}

/** Re-read from localStorage the way a fresh app launch would. */
function relaunch(): ThemePreference {
  useThemeStore.persist.rehydrate();
  return useThemeStore.getState().preference;
}

describe("theme store", () => {
  beforeEach(() => {
    localStorage.clear();
    useThemeStore.setState(initialState, true);
  });

  it("defaults to the pre-M5 dark theme", () => {
    expect(useThemeStore.getState().preference).toBe(DEFAULT_THEME);
  });

  it("records a choice", () => {
    useThemeStore.getState().setPreference("daylight");
    expect(useThemeStore.getState().preference).toBe("daylight");
  });

  it("persists the choice under the key the boot script reads", () => {
    useThemeStore.getState().setPreference("obsidian");
    expect(stored()).toBe("obsidian");
  });

  it("persists 'system' as a preference, not as a resolved theme", () => {
    useThemeStore.getState().setPreference("system");
    expect(stored()).toBe("system");
  });

  it.each(["system", "midnight", "daylight", "obsidian"] as const)(
    "round-trips %s across a restart",
    (preference) => {
      useThemeStore.getState().setPreference(preference);
      const onDisk = localStorage.getItem(THEME_STORAGE_KEY);

      // A fresh process: default in-memory state, whatever was written left
      // untouched on disk. (Resetting the store itself re-persists, so the
      // snapshot has to be put back before rehydrating.)
      useThemeStore.setState(initialState, true);
      localStorage.setItem(THEME_STORAGE_KEY, onDisk!);

      expect(relaunch()).toBe(preference);
    },
  );

  it("persists nothing beyond the preference", () => {
    useThemeStore.getState().setPreference("daylight");
    const raw = localStorage.getItem(THEME_STORAGE_KEY);
    expect(Object.keys(JSON.parse(raw!).state)).toEqual(["preference"]);
  });

  it("falls back to the default when storage holds an unknown theme", () => {
    localStorage.setItem(
      THEME_STORAGE_KEY,
      JSON.stringify({ state: { preference: "solarized" }, version: 0 }),
    );
    expect(relaunch()).toBe(DEFAULT_THEME);
  });

  it("falls back to the default when storage is corrupt", () => {
    localStorage.setItem(THEME_STORAGE_KEY, "{ not json");
    expect(relaunch()).toBe(DEFAULT_THEME);
  });
});

describe("stored preference + system preference -> applied theme", () => {
  beforeEach(() => {
    localStorage.clear();
    useThemeStore.setState(initialState, true);
    document.documentElement.className = "";
  });

  it.each([
    ["system", true, SYSTEM_THEME.dark],
    ["system", false, SYSTEM_THEME.light],
    ["midnight", false, "midnight"],
    ["daylight", true, "daylight"],
    ["obsidian", false, "obsidian"],
  ] as const)(
    "%s + systemPrefersDark=%s -> %s",
    (preference, systemPrefersDark, expected) => {
      useThemeStore.getState().setPreference(preference);
      const resolved = resolveTheme(
        relaunch(),
        systemPrefersDark,
      );
      expect(resolved).toBe(expected);

      applyThemeClass(resolved);
      expect(document.documentElement.classList.contains(themeClass(expected))).toBe(
        true,
      );
    },
  );

  it("leaves exactly one theme class on <html>", () => {
    applyThemeClass("daylight");
    applyThemeClass("obsidian");
    const classes = [...document.documentElement.classList].filter((c) =>
      c.startsWith("theme-"),
    );
    expect(classes).toEqual(["theme-obsidian"]);
  });

  it("hands the boot script's inline overrides back to the cascade", () => {
    const root = document.documentElement;
    root.style.backgroundColor = "#0f0f12";
    root.style.colorScheme = "dark";

    applyThemeClass("daylight");

    expect(root.style.backgroundColor).toBe("");
    expect(root.style.colorScheme).toBe("");
  });

  it("keeps other classes on <html> intact", () => {
    document.documentElement.classList.add("some-other-flag");
    applyThemeClass("midnight");
    expect(document.documentElement.classList.contains("some-other-flag")).toBe(true);
  });
});

describe("prefersDark", () => {
  it("reads the OS media query", () => {
    const matchMedia = vi.fn().mockReturnValue({ matches: false });
    vi.stubGlobal("matchMedia", matchMedia);
    expect(prefersDark()).toBe(false);
    expect(matchMedia).toHaveBeenCalledWith("(prefers-color-scheme: dark)");
    vi.unstubAllGlobals();
  });

  it("assumes dark when the environment cannot answer", () => {
    vi.stubGlobal("matchMedia", undefined);
    expect(prefersDark()).toBe(true);
    vi.unstubAllGlobals();
  });
});
