// Generates THIRD-PARTY-NOTICES.md.
//
//   pnpm gen:notices
//
// DevOS ships two bodies of other people's code in one binary: Rust crates
// compiled into the executable, and npm packages bundled into the webview
// assets. Almost all of them are MIT or BSD-family, and almost all of those
// require the copyright notice to travel with the binary. Publishing source
// does not trigger that duty; handing someone an installer does.
//
// The Rust half comes from `cargo about`, which resolves the real build graph.
// The npm half is derived here, and the method matters — see below.

import { execFileSync } from "node:child_process";
import { createRequire } from "node:module";
import fs from "node:fs";
import path from "node:path";
import process from "node:process";

const require = createRequire(import.meta.url);
const root = path.resolve(import.meta.dirname, "..");
const OUT = path.join(root, "THIRD-PARTY-NOTICES.md");

// A scratch build directory, so regenerating notices never clobbers the real
// dist/ that a release might be about to package.
//
// Vite is handed the *relative* name. The absolute path can contain spaces —
// it does on the machine this was written on — and these commands run through
// a shell on Windows, which re-splits an unquoted argument on them.
const PROBE_REL = ".notices-build";
const PROBE_DIR = path.join(root, PROBE_REL);

const LICENSE_FILES = [
  "LICENSE", "LICENSE.md", "LICENSE.txt", "LICENCE", "LICENCE.md", "LICENCE.txt",
  "LICENSE-MIT", "LICENSE-MIT.txt", "LICENSE-APACHE", "LICENSE-APACHE.txt",
  "COPYING", "COPYING.txt", "UNLICENSE", "OFL.txt", "LICENSE.OFL",
];

function log(msg) {
  process.stderr.write(`${msg}\n`);
}

// ---------------------------------------------------------------------------
// npm: what actually ships
//
// `pnpm licenses list --prod` is the obvious source and it is wrong for this
// purpose. Tailwind v4 puts `tailwindcss` and `@tailwindcss/vite` in
// `dependencies`, which drags esbuild and lightningcss into the "production"
// tree even though both only ever run at build time and neither reaches a
// browser. Attributing them would pad this file with code we do not ship,
// which makes the obligations we *do* have harder to see.
//
// So instead: build with sourcemaps and read the `sources` array out of the
// emitted bundle. That is a record of every module the bundler actually pulled
// in, after tree-shaking — the ground truth for what a user receives.
// ---------------------------------------------------------------------------

function buildWithSourcemaps() {
  log("building frontend with sourcemaps to discover what actually ships…");
  // Vite's JS entrypoint is run directly with this same node, rather than
  // through `pnpm exec`. Two Windows problems disappear at once: `shell: true`
  // does not escape arguments, so a project path containing a space (this one
  // does) gets re-split; and since Node 20 closed CVE-2024-27980, spawning
  // `pnpm.cmd` *without* a shell fails outright with EINVAL. Calling the .js
  // file needs neither.
  const viteBin = path.join(root, "node_modules", "vite", "bin", "vite.js");
  if (!fs.existsSync(viteBin)) {
    throw new Error(`vite not found at ${viteBin} — run \`pnpm install\` first`);
  }
  execFileSync(
    process.execPath,
    [viteBin, "build", "--sourcemap", "--outDir", PROBE_REL, "--emptyOutDir"],
    { cwd: root, stdio: ["ignore", "ignore", "inherit"] },
  );
}

const PNPM_RE =
  /node_modules[\\/]\.pnpm[\\/][^\\/]+[\\/]node_modules[\\/](@[^\\/]+[\\/][^\\/]+|[^\\/]+)/;
const PLAIN_RE = /node_modules[\\/](@[^\\/]+[\\/][^\\/]+|[^\\/]+)/;

/**
 * Maps every package referenced by a sourcemap to its directory on disk.
 *
 * The directory is recovered from the sourcemap path rather than looked up
 * with `require.resolve`, because pnpm's layout defeats resolution from the
 * project root: only *direct* dependencies are linked into ./node_modules, and
 * everything transitive lives solely under node_modules/.pnpm/<pkg>@<ver>/.
 * Resolving by name found 28 of 141 packages and silently dropped the rest,
 * which would have shipped a notices file missing most of its attributions.
 *
 * The sourcemap already knows exactly which copy was bundled, so the path it
 * records is both easier and more accurate than any re-resolution.
 */
function packagesFromSourcemaps(dir) {
  /** @type {Map<string, string>} package name -> absolute directory */
  const found = new Map();
  const walk = (d) => {
    for (const entry of fs.readdirSync(d, { withFileTypes: true })) {
      const p = path.join(d, entry.name);
      if (entry.isDirectory()) walk(p);
      else if (entry.name.endsWith(".map")) {
        let map;
        try {
          map = JSON.parse(fs.readFileSync(p, "utf8"));
        } catch {
          continue;
        }
        for (const src of map.sources ?? []) {
          const m = PNPM_RE.exec(src) ?? PLAIN_RE.exec(src);
          if (!m) continue;
          const name = m[1].replace(/\\/g, "/");
          if (found.has(name)) continue;
          // Sourcemap paths are relative to the emitted chunk ("../../node_
          // modules/…"). Everything before node_modules is that traversal, so
          // slicing from it and joining to the root yields the real directory.
          const at = src.search(/node_modules[\\/]/);
          if (at === -1) continue;
          const end = m.index + m[0].length;
          found.set(name, path.join(root, src.slice(at, end)));
        }
      }
    }
  };
  walk(dir);
  return found;
}

/**
 * Fonts are the one thing JS sourcemaps cannot see. They are not modules —
 * they are .woff2 binaries copied into the bundle, redistributed verbatim,
 * which is a stronger obligation than code that gets compiled away. OFL-1.1
 * also forbids selling the fonts on their own and reserves the font names, so
 * these are called out separately rather than buried in a list.
 *
 * Attribution is derived from the files actually emitted, not from
 * package.json. Reading the manifest would attribute a font that is declared
 * but never imported, and would keep attributing it after the import is
 * removed. Emitted assets are named `<stem>-<hash>.woff2`, where `<stem>`
 * matches a file inside the owning @fontsource package.
 */
function shippedFontPackages(dir) {
  const emitted = [];
  const walk = (d) => {
    for (const entry of fs.readdirSync(d, { withFileTypes: true })) {
      const p = path.join(d, entry.name);
      if (entry.isDirectory()) walk(p);
      else if (/\.(woff2?|ttf|otf|eot)$/i.test(entry.name)) emitted.push(entry.name);
    }
  };
  walk(dir);
  if (emitted.length === 0) return { names: [], files: [] };

  // Strip vite's content hash to recover the original font file name.
  const stems = new Set(
    emitted.map((f) => f.replace(/-[A-Za-z0-9_-]{6,10}(\.[a-z0-9]+)$/i, "$1")),
  );

  const pkg = JSON.parse(fs.readFileSync(path.join(root, "package.json"), "utf8"));
  const candidates = Object.keys({
    ...(pkg.dependencies ?? {}),
    ...(pkg.devDependencies ?? {}),
  }).filter((n) => n.startsWith("@fontsource"));

  const names = candidates.filter((name) => {
    const dir = resolvePackageDir(name);
    if (!dir) return false;
    const filesDir = path.join(dir, "files");
    if (!fs.existsSync(filesDir)) return false;
    return fs.readdirSync(filesDir).some((f) => stems.has(f));
  });
  return { names, files: emitted };
}

/**
 * Packages that ship as generated *stylesheet* content rather than as modules.
 * Vite emits no CSS sourcemaps, so nothing above can see them, yet Tailwind's
 * preflight and these plugins' utility classes are copied verbatim into the
 * shipped stylesheet. Read from the CSS entrypoint's own @import/@plugin lines
 * so this list cannot drift away from what the stylesheet actually pulls in.
 */
function cssOnlyPackages() {
  const entry = path.join(root, "src", "shared", "styles", "globals.css");
  if (!fs.existsSync(entry)) return [];
  const css = fs.readFileSync(entry, "utf8");
  const names = new Set();
  for (const m of css.matchAll(/^\s*@(?:import|plugin)\s+["']([^"']+)["']/gm)) {
    const spec = m[1];
    if (spec.startsWith(".") || spec.startsWith("/")) continue; // local file
    const parts = spec.split("/");
    names.add(spec.startsWith("@") ? parts.slice(0, 2).join("/") : parts[0]);
  }
  return [...names];
}

function resolvePackageDir(name) {
  try {
    return path.dirname(require.resolve(`${name}/package.json`, { paths: [root] }));
  } catch {
    // Packages with no `exports` entry for package.json — resolve the hard way.
    const guess = path.join(root, "node_modules", ...name.split("/"));
    return fs.existsSync(guess) ? guess : null;
  }
}

function readLicenseText(dir) {
  for (const f of LICENSE_FILES) {
    const p = path.join(dir, f);
    if (fs.existsSync(p) && fs.statSync(p).isFile()) {
      return fs.readFileSync(p, "utf8").trim();
    }
  }
  return null;
}

function npmSection(packages) {
  /** @type {Map<string, Map<string, string[]>>} license id -> text -> packages */
  const byLicense = new Map();
  const missing = [];

  for (const name of [...packages.keys()].sort()) {
    // Prefer the directory the sourcemap pointed at; fall back to resolution
    // for the font and stylesheet packages, which are direct dependencies and
    // therefore do exist in ./node_modules.
    const dir = packages.get(name) ?? resolvePackageDir(name);
    if (!dir || !fs.existsSync(dir)) {
      missing.push(name);
      continue;
    }
    let meta = {};
    try {
      meta = JSON.parse(fs.readFileSync(path.join(dir, "package.json"), "utf8"));
    } catch {
      /* fall through to UNKNOWN */
    }
    const id =
      typeof meta.license === "string"
        ? meta.license
        : (meta.license?.type ?? meta.licenses?.[0]?.type ?? "UNKNOWN");
    const text = readLicenseText(dir);
    const label = `${name} ${meta.version ?? "?"}`;

    if (!byLicense.has(id)) byLicense.set(id, new Map());
    const bucket = byLicense.get(id);
    // Group identical texts so a hundred MIT copies do not become a hundred
    // near-identical blocks; distinct copyright holders stay distinct, which
    // is exactly what MIT requires to be preserved.
    const key = text ?? " none";
    if (!bucket.has(key)) bucket.set(key, []);
    bucket.get(key).push(label);
  }

  let out = "";
  for (const id of [...byLicense.keys()].sort()) {
    const bucket = byLicense.get(id);
    const total = [...bucket.values()].reduce((n, v) => n + v.length, 0);
    out += `### ${id}\n\n<details>\n<summary>Packages (${total})</summary>\n\n`;
    for (const pkgs of bucket.values()) for (const p of pkgs) out += `- **${p}**\n`;
    out += `\n</details>\n\n`;
    for (const [key, pkgs] of bucket) {
      if (key === " none") {
        out += `> No license file is distributed with ${pkgs.join(", ")}; the package declares \`${id}\`.\n\n`;
      } else {
        out += "```text\n" + key + "\n```\n\n";
      }
    }
  }
  return { markdown: out, missing };
}

function rustSection() {
  log("resolving the Rust build graph with cargo-about…");
  return execFileSync(
    "cargo",
    ["about", "generate", "--config", "about.toml", "about.hbs"],
    { cwd: root, encoding: "utf8", maxBuffer: 256 * 1024 * 1024 },
  );
}

// ---------------------------------------------------------------------------

function main() {
  buildWithSourcemaps();
  const packages = packagesFromSourcemaps(PROBE_DIR);
  const fromModules = packages.size;
  const fonts = shippedFontPackages(PROBE_DIR);
  const css = cssOnlyPackages();
  // Both of these are direct dependencies, so a null directory here means
  // "resolve it by name" rather than "not found".
  for (const name of [...fonts.names, ...css]) {
    if (!packages.has(name)) packages.set(name, null);
  }
  log(
    `  ${fromModules} packages from JS sourcemaps, ` +
      `${fonts.names.length} font packages (${fonts.files.length} files), ` +
      `${css.length} CSS-only — ${packages.size} total`,
  );

  const npm = npmSection(packages);
  const rust = rustSection();
  fs.rmSync(PROBE_DIR, { recursive: true, force: true });

  const header = `# Third-party notices

DevOS is MIT-licensed, but the program it ships is mostly other people's work.
This file reproduces their copyright notices and license terms, which most of
those licenses require to accompany a distributed binary. Publishing source
does not trigger that duty — shipping an installer does.

Nothing here is copyleft in a way that reaches DevOS itself: there is no GPL or
AGPL anywhere in the graph. The MPL-2.0 components are weak, file-level
copyleft that reaches only their own files, and those arrive unmodified.

**Do not edit this file by hand.** Regenerate it:

\`\`\`bash
pnpm gen:notices
\`\`\`

The two halves are gathered differently, and deliberately so. Rust crates come
from \`cargo about\`, which resolves the real build graph for the Windows target.

The npm half is read out of an actual production build rather than the
dependency tree, because the tree is not what ships: Tailwind places build-only
tooling such as esbuild and lightningcss in \`dependencies\`, and none of it
reaches a browser. Three sources are combined — modules named in the JS
sourcemaps (which is post-tree-shaking, so genuinely what a user receives),
the \`@fontsource\` packages whose files were actually emitted, and the packages
the stylesheet \`@import\`s, since Vite produces no CSS sourcemaps and Tailwind's
preflight is copied verbatim into the shipped stylesheet.

---

## Fonts

These are redistributed as \`.woff2\` binaries inside the application, rather
than compiled away like the code below, so their terms apply to files a user
actually receives. SIL OFL-1.1 permits bundling and modification, but the fonts
may not be sold on their own, and the Reserved Font Names may not be reused for
modified versions. DevOS ships them unmodified.

${fonts.names.map((n) => `- **${n}**`).join("\n")}

_${fonts.files.length} font files are emitted into the bundle._

---

## npm packages

`;

  fs.writeFileSync(OUT, header + npm.markdown + "\n---\n\n## Rust crates\n\n" + rust, "utf8");

  const kb = Math.round(fs.statSync(OUT).size / 1024);
  log(`wrote ${path.relative(root, OUT)} (${kb} kB)`);

  // Fatal, not a warning. A package that ships without its notice is the exact
  // failure this file exists to prevent, and a warning at the end of a long
  // build is a warning nobody reads.
  if (npm.missing.length) {
    log(
      `\nFAILED: ${npm.missing.length} package(s) ship but could not be located ` +
        `on disk, so their notices are missing:\n  ${npm.missing.join(", ")}`,
    );
    process.exit(1);
  }
}

main();
