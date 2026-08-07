import { describe, expect, it } from "vitest";

import { ansiSchemeFor, buildXtermTheme, relativeLuminance } from "./theme";

describe("relativeLuminance", () => {
  it("puts black at 0 and white at 1", () => {
    expect(relativeLuminance("#000000")).toBeCloseTo(0, 5);
    expect(relativeLuminance("#ffffff")).toBeCloseTo(1, 5);
  });

  it("reads both DevOS backgrounds on the correct side of the midpoint", () => {
    // Measured in the app: these are what --background rasterises to in
    // Midnight and Daylight (see theme.ts on why rasterising is required).
    expect(relativeLuminance("#0f0f12")).toBeLessThan(0.5);
    expect(relativeLuminance("#fafafb")).toBeGreaterThan(0.5);
  });

  it("tolerates a missing leading hash", () => {
    expect(relativeLuminance("0f0f12")).toBeCloseTo(
      relativeLuminance("#0f0f12"),
      10,
    );
  });
});

describe("ansiSchemeFor", () => {
  it("gives a dark background only the two slots the app has always set", () => {
    const scheme = ansiSchemeFor("#0f0f12");
    // Leaving the other fourteen unset is what keeps the dark themes looking
    // exactly as they did before the terminal was themed.
    expect(Object.keys(scheme).sort()).toEqual(["black", "brightBlack"]);
  });

  it("gives a light background a complete sixteen-colour scheme", () => {
    const scheme = ansiSchemeFor("#fafafb") as Record<string, string>;
    const expected = [
      "black",
      "red",
      "green",
      "yellow",
      "blue",
      "magenta",
      "cyan",
      "white",
      "brightBlack",
      "brightRed",
      "brightGreen",
      "brightYellow",
      "brightBlue",
      "brightMagenta",
      "brightCyan",
      "brightWhite",
    ];
    for (const slot of expected) {
      expect(scheme[slot], `missing ANSI slot: ${slot}`).toMatch(
        /^#[0-9a-f]{6}$/i,
      );
    }
    expect(Object.keys(scheme)).toHaveLength(expected.length);
  });

  it("keeps every light ANSI colour readable on white", () => {
    // xterm's defaults assume a dark background; several (yellow worst of all)
    // are unreadable on paper. 4.5:1 against Daylight's real background is the
    // bar — this test is what caught four of GitHub's values falling short.
    const background = relativeLuminance("#fafafb");
    const scheme = ansiSchemeFor("#fafafb") as Record<string, string>;
    for (const [slot, color] of Object.entries(scheme)) {
      const ratio = (background + 0.05) / (relativeLuminance(color) + 0.05);
      // "white"/"brightWhite" mean *dim* on a light background and are used
      // for de-emphasised text, so they are held to the 3:1 non-text bar.
      const floor = slot.toLowerCase().includes("white") ? 3 : 4.5;
      expect(ratio, `${slot} (${color}) on white`).toBeGreaterThanOrEqual(floor);
    }
  });
});

describe("buildXtermTheme", () => {
  it("falls back to the pre-theme Midnight colours when tokens cannot be resolved", () => {
    // jsdom has no 2d canvas context, so this exercises the same path as any
    // engine that will not rasterise an oklch token — the terminal must still
    // come up looking like itself rather than unstyled.
    const theme = buildXtermTheme();
    expect(theme.background).toBe("#121214");
    expect(theme.foreground).toBe("#d7d7de");
    expect(theme.cursor).toBe("#8f8ff5");
    expect(theme.cursorAccent).toBe(theme.background);
  });

  it("derives selection from the cursor colour rather than hardcoding it", () => {
    const theme = buildXtermTheme();
    expect(theme.selectionBackground).toBe("rgba(143, 143, 245, 0.32)");
  });
});
