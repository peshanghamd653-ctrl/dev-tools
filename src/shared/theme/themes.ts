/**
 * Theme registry and resolution.
 *
 * Pure — no DOM, no store. Everything here is shared with the boot script in
 * `index.html`, which must reproduce `resolveTheme` in a few lines of inline
 * JS to beat first paint; `themes.test.ts` pins the two in sync.
 */

export const THEME_IDS = ["midnight", "daylight", "obsidian"] as const;

export type ThemeId = (typeof THEME_IDS)[number];

/** What the user picked. `"system"` follows `prefers-color-scheme`. */
export type ThemePreference = "system" | ThemeId;

export type ThemeAppearance = "light" | "dark";

export interface ThemeMeta {
  id: ThemeId;
  label: string;
  description: string;
  appearance: ThemeAppearance;
}

/**
 * Midnight is the pre-M5 palette, verbatim. It stays the default so an
 * existing install that has never opened Settings looks exactly as it did.
 */
export const DEFAULT_THEME: ThemeId = "midnight";

/** Which theme `"system"` resolves to at each end. */
export const SYSTEM_THEME: Record<ThemeAppearance, ThemeId> = {
  dark: "midnight",
  light: "daylight",
};

/** localStorage key — must match the zustand `persist` name and the boot script. */
export const THEME_STORAGE_KEY = "devos-theme";

export const THEMES: readonly ThemeMeta[] = [
  {
    id: "midnight",
    label: "Midnight",
    description: "The DevOS default. Cool near-black with an indigo accent.",
    appearance: "dark",
  },
  {
    id: "daylight",
    label: "Daylight",
    description: "Light theme for bright rooms and screen sharing.",
    appearance: "light",
  },
  {
    id: "obsidian",
    label: "Obsidian",
    description: "Deeper black, higher contrast. Kind to OLED panels.",
    appearance: "dark",
  },
];

const BY_ID = new Map(THEMES.map((theme) => [theme.id, theme]));

export function isThemeId(value: unknown): value is ThemeId {
  return typeof value === "string" && BY_ID.has(value as ThemeId);
}

export function isThemePreference(value: unknown): value is ThemePreference {
  return value === "system" || isThemeId(value);
}

export function themeMeta(id: ThemeId): ThemeMeta {
  const meta = BY_ID.get(id);
  if (!meta) throw new Error(`Unknown theme: ${id}`);
  return meta;
}

export function themeAppearance(id: ThemeId): ThemeAppearance {
  return themeMeta(id).appearance;
}

/** The single class DevOS puts on `<html>` (and on preview subtrees). */
export function themeClass(id: ThemeId): string {
  return `theme-${id}`;
}

export const THEME_CLASSES: readonly string[] = THEME_IDS.map(themeClass);

/**
 * Stored preference + live OS preference → the theme to paint.
 *
 * Anything unrecognised (a downgrade, a hand-edited localStorage value)
 * falls back to the default rather than leaving the app unstyled.
 */
export function resolveTheme(
  preference: unknown,
  systemPrefersDark: boolean,
): ThemeId {
  if (isThemeId(preference)) return preference;
  if (preference === "system") {
    return SYSTEM_THEME[systemPrefersDark ? "dark" : "light"];
  }
  return DEFAULT_THEME;
}
