/** DOM side of the theme system: the class on <html> and the OS preference. */

import {
  THEME_CLASSES,
  themeAppearance,
  themeClass,
  type ThemeId,
} from "@/shared/theme/themes";

export const DARK_SCHEME_QUERY = "(prefers-color-scheme: dark)";

/**
 * Swap the theme class on `<html>`.
 *
 * Also drops the two inline properties the boot script set. Those exist only
 * to beat first paint in dev, where the stylesheet is injected by JS; leaving
 * them would pin the page to the booted theme forever, because an inline
 * style outranks every cascade layer.
 */
export function applyThemeClass(
  id: ThemeId,
  root: HTMLElement = document.documentElement,
): void {
  root.classList.remove(...THEME_CLASSES);
  root.classList.add(themeClass(id));
  root.style.removeProperty("background-color");
  root.style.removeProperty("color-scheme");
}

export function prefersDark(): boolean {
  // Default to dark: DevOS is dark-first, and a headless/jsdom environment
  // reporting nothing should not flip the app to light.
  if (typeof window === "undefined" || !window.matchMedia) return true;
  return window.matchMedia(DARK_SCHEME_QUERY).matches;
}

/**
 * Subscribe to live OS appearance changes. Returns an unsubscribe function.
 *
 * `addEventListener` on a MediaQueryList is guarded because the Tauri webview
 * on older Windows builds still exposes only the deprecated `addListener`.
 */
export function watchSystemAppearance(
  onChange: (systemPrefersDark: boolean) => void,
): () => void {
  if (typeof window === "undefined" || !window.matchMedia) return () => {};

  const query = window.matchMedia(DARK_SCHEME_QUERY);
  const handler = (event: MediaQueryListEvent) => onChange(event.matches);

  if (typeof query.addEventListener === "function") {
    query.addEventListener("change", handler);
    return () => query.removeEventListener("change", handler);
  }

  const legacy = query as MediaQueryList & {
    addListener?: (cb: (e: MediaQueryListEvent) => void) => void;
    removeListener?: (cb: (e: MediaQueryListEvent) => void) => void;
  };
  legacy.addListener?.(handler);
  return () => legacy.removeListener?.(handler);
}

/** `sonner` needs the appearance by name; keep the mapping in one place. */
export function toasterTheme(id: ThemeId): "light" | "dark" {
  return themeAppearance(id);
}
