import { useEffect, useState } from "react";

import { useThemeStore } from "@/shared/stores/theme";
import { applyThemeClass, prefersDark, watchSystemAppearance } from "./apply";
import { resolveTheme, type ThemeId, type ThemePreference } from "./themes";

/** Read/write the stored preference. `"system"` included. */
export function useThemePreference(): [
  ThemePreference,
  (preference: ThemePreference) => void,
] {
  const preference = useThemeStore((s) => s.preference);
  const setPreference = useThemeStore((s) => s.setPreference);
  return [preference, setPreference];
}

/** The OS appearance, kept live. */
export function useSystemPrefersDark(): boolean {
  const [systemPrefersDark, setSystemPrefersDark] = useState(prefersDark);
  useEffect(() => watchSystemAppearance(setSystemPrefersDark), []);
  return systemPrefersDark;
}

/** The theme actually painted right now. */
export function useResolvedTheme(): ThemeId {
  const preference = useThemeStore((s) => s.preference);
  return resolveTheme(preference, useSystemPrefersDark());
}

/**
 * Keeps `<html>` in sync with the store. Mounted once, in the shell.
 *
 * The effect deliberately does *not* handle first paint — the boot script in
 * `index.html` already put the right class on `<html>` before this module was
 * even fetched. This only reacts to changes: the user picking a theme, or the
 * OS flipping appearance while the app is open.
 */
export function useThemeSync(): ThemeId {
  const resolved = useResolvedTheme();
  useEffect(() => {
    applyThemeClass(resolved);
  }, [resolved]);
  return resolved;
}
