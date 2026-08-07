/**
 * The xterm colour scheme, derived from the active DevOS theme.
 *
 * Two halves, decided separately:
 *
 * **Chrome** (background, foreground, cursor, selection) comes from the
 * semantic tokens, so the terminal is the same surface as the page it sits
 * on rather than a dark rectangle pasted onto a light app.
 *
 * **The 16 ANSI colours** do not. The obvious move — map them onto the
 * Tailwind palette variables that `.theme-daylight` already re-points — falls
 * apart on two of the sixteen: no `cyan` or `fuchsia`/`purple` family is
 * re-pointed, so ANSI cyan and magenta would read correctly in the dark
 * themes and turn invisible on white. That is exactly the failure
 * `docs/design-system.md` warns about for lone palette families. An ANSI
 * scheme is a terminal concern anyway, not app chrome, so it lives here as
 * two explicit schemes chosen by the background's luminance.
 *
 * Dark keeps only the two slots the app has always overridden and lets xterm
 * fill the rest from its defaults — so the dark themes look exactly as they
 * did before this file existed. Light needs a full scheme, because xterm's
 * defaults assume a dark background and several of them (yellow especially)
 * are unreadable on white.
 */
import type { ITheme } from "@xterm/xterm";

/**
 * Pre-theme Midnight values, used whenever a colour cannot be resolved —
 * under jsdom in tests, or in any engine that will not rasterise the token.
 * Falling back to how the terminal has always looked beats falling back to
 * an unstyled one.
 */
const FALLBACK_BACKGROUND = "#121214";
const FALLBACK_FOREGROUND = "#d7d7de";
const FALLBACK_CURSOR = "#8f8ff5";

/** Overrides the dark themes have always carried. Everything else is xterm's. */
const DARK_ANSI: ITheme = {
  black: "#1f1f23",
  brightBlack: "#5c5c66",
};

/**
 * GitHub's light-theme ANSI values rather than something hand-mixed: they are
 * widely exercised on white and already solve the hard cases (yellow becomes a
 * dark amber; ANSI "white", which means *normal foreground on dark*, becomes a
 * mid grey instead of disappearing).
 *
 * Four deviations, all darkenings, all found by the test rather than by eye.
 * GitHub's set is built for UI accents; terminal output is body text, so the
 * bar here is 4.5:1 against Daylight's background (3:1 for `white` and
 * `brightWhite`, which mean *dim* on a light background and are used for
 * de-emphasised text):
 *
 *   brightBlue    #218bff → #1a70d8   (3.28 → 4.67)
 *   brightMagenta #a475f9 → #835ec7   (3.13 → 4.63)
 *   brightCyan    #3192aa → #2a7c90   (3.49 → 4.63)
 *   brightWhite   #8c959f → #848c96   (2.93 → 3.26)
 *
 * Each stays lighter than its non-bright counterpart, so the bright/normal
 * distinction survives — the thing a naive darkening would quietly destroy.
 * `brightWhite` is darkened past its floor rather than to it: the minimum
 * passing value sat at 3.02, and a threshold that close would flip on any
 * future nudge to Daylight's background.
 */
const LIGHT_ANSI: ITheme = {
  black: "#24292f",
  red: "#cf222e",
  green: "#116329",
  yellow: "#4d2d00",
  blue: "#0969da",
  magenta: "#8250df",
  cyan: "#1b7c83",
  white: "#6e7781",
  brightBlack: "#57606a",
  brightRed: "#a40e26",
  brightGreen: "#1a7f37",
  brightYellow: "#633c01",
  brightBlue: "#1a70d8",
  brightMagenta: "#835ec7",
  brightCyan: "#2a7c90",
  brightWhite: "#848c96",
};

/**
 * Resolve a CSS colour expression to `#rrggbb`.
 *
 * `getComputedStyle` is not enough on its own: this app's tokens are `oklch()`
 * and Chromium returns them *unresolved*, while xterm's colour parser handles
 * only hex/rgb/hsl and would reject the string. So the computed value is
 * rasterised into a 1×1 canvas and read back as sRGB bytes, which works
 * whatever colour space the token was authored in.
 *
 * Returns `null` rather than guessing when anything is unavailable.
 */
function resolveColor(expression: string): string | null {
  if (typeof document === "undefined") return null;

  const probe = document.createElement("span");
  probe.style.display = "none";
  document.body.appendChild(probe);
  try {
    probe.style.color = expression;
    const computed = getComputedStyle(probe).color;
    if (!computed) return null;

    const canvas = document.createElement("canvas");
    canvas.width = 1;
    canvas.height = 1;
    const ctx = canvas.getContext("2d", { willReadFrequently: true });
    if (!ctx) return null;

    // Seed with a known value: an unparseable assignment is ignored by the
    // canvas rather than throwing, and would otherwise read back as whatever
    // was there before.
    ctx.fillStyle = "#000000";
    ctx.fillStyle = computed;
    ctx.fillRect(0, 0, 1, 1);
    const pixel = ctx.getImageData(0, 0, 1, 1).data;
    const channel = (index: number) =>
      (pixel[index] ?? 0).toString(16).padStart(2, "0");
    return `#${channel(0)}${channel(1)}${channel(2)}`;
  } catch {
    // jsdom has no 2d context; anything else here is equally not worth
    // failing a terminal over.
    return null;
  } finally {
    probe.remove();
  }
}

/** WCAG relative luminance of `#rrggbb`. */
export function relativeLuminance(hex: string): number {
  const value = hex.replace("#", "");
  const channel = (offset: number) => {
    const srgb = parseInt(value.slice(offset, offset + 2), 16) / 255;
    return srgb <= 0.03928 ? srgb / 12.92 : ((srgb + 0.055) / 1.055) ** 2.4;
  };
  return 0.2126 * channel(0) + 0.7152 * channel(2) + 0.0722 * channel(4);
}

/**
 * Which ANSI scheme suits a background.
 *
 * Keyed off measured luminance rather than the theme's id, so a theme added
 * later gets a readable terminal without anyone remembering to update a list
 * here.
 */
export function ansiSchemeFor(background: string): ITheme {
  return relativeLuminance(background) > 0.5 ? LIGHT_ANSI : DARK_ANSI;
}

/** Add an alpha channel to `#rrggbb`. */
function withAlpha(hex: string, alpha: number): string {
  const value = hex.replace("#", "");
  const channel = (offset: number) => parseInt(value.slice(offset, offset + 2), 16);
  return `rgba(${channel(0)}, ${channel(2)}, ${channel(4)}, ${alpha})`;
}

/** The xterm theme for whatever is painted right now. */
export function buildXtermTheme(): ITheme {
  const background = resolveColor("var(--background)") ?? FALLBACK_BACKGROUND;
  const foreground = resolveColor("var(--foreground)") ?? FALLBACK_FOREGROUND;
  const cursor = resolveColor("var(--primary)") ?? FALLBACK_CURSOR;

  return {
    background,
    foreground,
    cursor,
    cursorAccent: background,
    selectionBackground: withAlpha(cursor, 0.32),
    ...ansiSchemeFor(background),
  };
}
