import { describe, expect, it } from "vitest";

// Raw source, not a fixture: these assertions are only worth anything if they
// run against the file the app actually ships.
import indexHtml from "../../../index.html?raw";
import {
  DEFAULT_THEME,
  isThemeId,
  isThemePreference,
  resolveTheme,
  SYSTEM_THEME,
  THEME_CLASSES,
  THEME_IDS,
  THEME_STORAGE_KEY,
  themeAppearance,
  themeClass,
  themeMeta,
  THEMES,
} from "./themes";

describe("theme registry", () => {
  it("ships a default dark theme, a light theme and a second dark variant", () => {
    expect(THEMES).toHaveLength(THEME_IDS.length);
    expect(THEMES.filter((t) => t.appearance === "light").length).toBeGreaterThanOrEqual(1);
    expect(THEMES.filter((t) => t.appearance === "dark").length).toBeGreaterThanOrEqual(2);
    expect(themeAppearance(DEFAULT_THEME)).toBe("dark");
  });

  it("describes every theme exactly once", () => {
    expect(THEMES.map((t) => t.id)).toEqual([...THEME_IDS]);
    for (const theme of THEMES) {
      expect(themeMeta(theme.id)).toBe(theme);
      expect(theme.label.length).toBeGreaterThan(0);
      expect(theme.description.length).toBeGreaterThan(0);
    }
  });

  it("maps each theme to a single class", () => {
    expect(THEME_CLASSES).toEqual(["theme-midnight", "theme-daylight", "theme-obsidian"]);
    expect(themeClass("daylight")).toBe("theme-daylight");
  });

  it("resolves system to a theme of the matching appearance", () => {
    expect(themeAppearance(SYSTEM_THEME.dark)).toBe("dark");
    expect(themeAppearance(SYSTEM_THEME.light)).toBe("light");
  });
});

describe("resolveTheme", () => {
  it("returns an explicit choice regardless of the OS preference", () => {
    for (const id of THEME_IDS) {
      expect(resolveTheme(id, true)).toBe(id);
      expect(resolveTheme(id, false)).toBe(id);
    }
  });

  it("follows the OS preference when set to system", () => {
    expect(resolveTheme("system", true)).toBe(SYSTEM_THEME.dark);
    expect(resolveTheme("system", false)).toBe(SYSTEM_THEME.light);
  });

  it("keeps a dark theme when the OS asks for dark and a light one otherwise", () => {
    expect(themeAppearance(resolveTheme("system", true))).toBe("dark");
    expect(themeAppearance(resolveTheme("system", false))).toBe("light");
  });

  it("falls back to the default for anything unrecognised", () => {
    for (const bogus of [undefined, null, "", "solarized", 42, {}, "Midnight"]) {
      expect(resolveTheme(bogus, true)).toBe(DEFAULT_THEME);
      expect(resolveTheme(bogus, false)).toBe(DEFAULT_THEME);
    }
  });

  it("always returns a real theme id", () => {
    for (const input of [...THEME_IDS, "system", "nope", null]) {
      expect(isThemeId(resolveTheme(input, true))).toBe(true);
    }
  });
});

describe("guards", () => {
  it("recognises theme ids", () => {
    expect(isThemeId("midnight")).toBe(true);
    expect(isThemeId("system")).toBe(false);
    expect(isThemeId("nope")).toBe(false);
  });

  it("recognises preferences, including system", () => {
    expect(isThemePreference("system")).toBe(true);
    expect(isThemePreference("obsidian")).toBe(true);
    expect(isThemePreference("nope")).toBe(false);
    expect(isThemePreference(undefined)).toBe(false);
  });
});

/*
 * The boot script in index.html re-implements resolveTheme inline so it can
 * run before first paint. It is the one place the theme list is duplicated,
 * so pin it here — a theme added to the registry but not to the script would
 * silently boot as midnight and then snap to the real theme on mount.
 */
describe("index.html boot script", () => {
  const bootScript = indexHtml.slice(
    indexHtml.indexOf("var THEMES = {"),
    indexHtml.indexOf("})();"),
  );

  it("was found", () => {
    expect(bootScript.length).toBeGreaterThan(100);
  });

  it("knows every theme, with the right appearance", () => {
    for (const theme of THEMES) {
      expect(bootScript).toContain(`${theme.id}: ["${theme.appearance}"`);
    }
  });

  it("uses the same storage key, default and system mapping as the module", () => {
    expect(bootScript).toContain(`var STORAGE_KEY = "${THEME_STORAGE_KEY}";`);
    expect(bootScript).toContain(`var DEFAULT_THEME = "${DEFAULT_THEME}";`);
    expect(bootScript).toContain(
      `var SYSTEM = { dark: "${SYSTEM_THEME.dark}", light: "${SYSTEM_THEME.light}" };`,
    );
  });

  it("applies the class before first paint, in <head>", () => {
    expect(bootScript).toContain('root.classList.add("theme-" + id)');
    const scriptAt = indexHtml.indexOf("var THEMES = {");
    expect(scriptAt).toBeGreaterThan(-1);
    expect(scriptAt).toBeLessThan(indexHtml.indexOf("</head>"));
    expect(scriptAt).toBeLessThan(indexHtml.indexOf("<body>"));
  });

  it("reads the preference at the path zustand persist writes", () => {
    expect(bootScript).toContain("JSON.parse(raw).state");
    expect(bootScript).toContain("state.preference");
  });
});
