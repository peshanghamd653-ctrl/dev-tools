import { readFileSync } from "node:fs";
import { cwd } from "node:process";
import { describe, expect, it } from "vitest";

import { DEFAULT_THEME, THEME_IDS, themeClass, type ThemeId } from "./themes";

// Read off disk rather than `import "…css?raw"` — vitest stubs every request
// matching `.css` to an empty module, so the import would silently yield ""
// and every assertion below would pass against nothing. (`import.meta.url` is
// no help either: vitest rewrites it to the jsdom location.) See ./node.d.ts.
const globalsCss = readFileSync(`${cwd()}/src/shared/styles/globals.css`, "utf8");

/** The class the app applies, as a CSS selector. */
const selectorFor = (id: ThemeId) => `.${themeClass(id)}`;

/**
 * A theme is a *complete* assignment of the token set. If a theme forgets
 * `--muted-foreground`, the value from whatever rule matched previously wins —
 * in practice the light theme keeps the dark theme's pale grey and renders
 * near-invisible text on white. That failure is silent, cheap to introduce and
 * expensive to notice, so it gets pinned here.
 *
 * These assert the *shape* of the token map, never the colour values — those
 * are a design decision and must stay free to change.
 */

interface Rule {
  selectors: string[];
  declarations: Record<string, string>;
}

function parseRules(css: string): Rule[] {
  // Comments first: they sit between rules and would otherwise be captured as
  // part of the following selector.
  const source = css.replace(/\/\*[\s\S]*?\*\//g, "");

  return [...source.matchAll(/([^{}]+)\{([^{}]*)\}/g)].map((match) => {
    // At-rules and imports before a selector end in `;`; keep only what
    // follows the last one.
    const head = (match[1] ?? "").split(";").pop() ?? "";
    const declarations: Record<string, string> = {};
    for (const [, name, value] of (match[2] ?? "").matchAll(
      /(--[\w-]+)\s*:\s*([^;]+);/g,
    )) {
      if (name && value) declarations[name] = value.trim();
    }
    return {
      selectors: head.split(",").map((part) => part.trim()).filter(Boolean),
      declarations,
    };
  });
}

const RULES = parseRules(globalsCss);

/** Every custom property a selector ends up setting, across all its rules. */
function tokensFor(selector: string): Record<string, string> {
  const matching = RULES.filter((rule) => rule.selectors.includes(selector));
  if (matching.length === 0) throw new Error(`No CSS rule for "${selector}"`);
  return Object.assign({}, ...matching.map((rule) => rule.declarations));
}

const isPalette = (token: string) => token.startsWith("--color-");
const pick = (tokens: Record<string, string>, palette: boolean) =>
  Object.fromEntries(
    Object.entries(tokens).filter(([token]) => isPalette(token) === palette),
  );

const semantic = (selector: string) => pick(tokensFor(selector), false);
const palette = (selector: string) => pick(tokensFor(selector), true);

/** Tokens `@theme inline` maps to a Tailwind utility — the real public surface. */
function consumedTokens(): string[] {
  const block = globalsCss.slice(globalsCss.indexOf("@theme inline"));
  const used = new Set<string>();
  for (const [, token] of block.matchAll(
    /--color-[\w-]+\s*:\s*var\((--[\w-]+)\)/g,
  )) {
    if (token) used.add(token);
  }
  return [...used].sort();
}

const ids = [...THEME_IDS];
const reference = semantic(selectorFor(DEFAULT_THEME));
const referenceTokens = Object.keys(reference).sort();

describe("theme token maps", () => {
  it("parses a substantial token block for every theme", () => {
    expect(referenceTokens.length).toBeGreaterThan(20);
    for (const id of ids) {
      expect(Object.keys(semantic(selectorFor(id)))).toHaveLength(
        referenceTokens.length,
      );
    }
  });

  it("keeps the default theme on :root so the app is never unstyled", () => {
    expect(semantic(":root")).toEqual(reference);
  });

  it.each(ids)("%s defines every token the default defines", (id) => {
    expect(Object.keys(semantic(selectorFor(id))).sort()).toEqual(referenceTokens);
  });

  it.each(ids)("%s gives every token a real value", (id) => {
    for (const [token, value] of Object.entries(semantic(selectorFor(id)))) {
      expect(value, `${id} ${token}`).not.toBe("");
      // A token defined as `var(--something-else)` would silently chain back
      // to another theme's value; only --radius derivations are legitimate.
      expect(value, `${id} ${token}`).not.toMatch(/^var\(--(?!radius)/);
    }
  });

  it("covers every token that @theme inline exposes as a utility", () => {
    for (const token of consumedTokens()) {
      expect(referenceTokens, `@theme inline consumes ${token}`).toContain(token);
    }
  });

  it("gives each theme its own values rather than re-declaring the default", () => {
    for (const id of ids) {
      if (id === DEFAULT_THEME) continue;
      const tokens = semantic(selectorFor(id));
      const changed = referenceTokens.filter((t) => tokens[t] !== reference[t]);
      expect(
        changed.length,
        `${id} barely differs from ${DEFAULT_THEME}`,
      ).toBeGreaterThan(20);
    }
  });
});

describe("status palette layer", () => {
  /*
   * Feature modules paint status with raw Tailwind palette utilities
   * (`text-emerald-300`, `bg-yellow-500/10`, …). A shade picked to read on
   * near-black is unreadable on near-white, so the light theme re-points the
   * shades those modules use. Every family it touches must cover the full
   * range, otherwise one status state drops out on a white background.
   */
  const light = palette(".theme-daylight");
  const families = ["emerald", "red", "yellow", "sky", "orange"];

  it.each(families)("%s is re-pointed for the light theme", (family) => {
    for (const shade of ["200", "300", "400", "500"]) {
      expect(light).toHaveProperty(`--color-${family}-${shade}`);
    }
  });

  it("re-points shades in whole families, never a lone shade", () => {
    const byFamily = new Map<string, Set<string>>();
    for (const token of Object.keys(light)) {
      const [, family, shade] = /^--color-([a-z]+)-(\d+)$/.exec(token) ?? [];
      if (!family || !shade) continue;
      byFamily.set(family, (byFamily.get(family) ?? new Set()).add(shade));
    }
    expect(byFamily.size).toBeGreaterThanOrEqual(families.length);
    for (const [family, shades] of byFamily) {
      expect([...shades].sort(), `${family} is partially re-pointed`).toEqual([
        "200",
        "300",
        "400",
        "500",
      ]);
    }
  });

  it("leaves the dark themes on Tailwind's own palette", () => {
    for (const id of ids) {
      if (id === "daylight") continue;
      expect(Object.keys(palette(selectorFor(id))), `${id}`).toEqual([]);
    }
  });
});
