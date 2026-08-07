// Checks that the three files carrying DevOS's version agree, and optionally
// that they match a release tag.
//
//   pnpm check:versions            # the three files agree
//   pnpm check:versions v0.2.0     # ...and match this tag
//
// Why this is worth a script and a CI step rather than a habit:
//
// `src-tauri/tauri.conf.json` is not just one of three copies. It is the one
// the built binary reports as its own version, the one tauri-action writes
// into latest.json, and therefore the one an installed copy compares against
// when it checks for an update. The other two are a package manifest and a
// crate manifest, and nothing at build time reads them for this purpose.
//
// So the interesting failure is not "three numbers disagree", it is what
// happens next. Bump package.json to 0.2.0, tag v0.2.0, forget
// tauri.conf.json: the workflow succeeds, the GitHub Release is titled 0.2.0,
// the installers upload, latest.json is generated and signed — and it
// advertises 0.1.0. Every installed copy compares 0.1.0 against its own 0.1.0,
// concludes it is current, and never offers the update. Nothing is red. The
// release simply does not exist as far as users are concerned, and the way you
// find out is that nobody is on the new version weeks later.
//
// That is the same class of problem as an unsigned release — invisible at
// build time, load-bearing for users — so it gets the same treatment: fail
// early and loudly, before the twenty-minute build rather than after it.

import fs from "node:fs";
import path from "node:path";
import process from "node:process";

const root = path.resolve(import.meta.dirname, "..");

/** @type {{label: string, file: string, read: (s: string) => string | null}[]} */
const SOURCES = [
  {
    label: "package.json",
    file: "package.json",
    read: (s) => JSON.parse(s).version ?? null,
  },
  {
    label: "Cargo.toml ([workspace.package])",
    file: "Cargo.toml",
    // Deliberately scoped to the [workspace.package] table. A bare /^version/
    // search would also match a version inside [workspace.dependencies], and
    // silently comparing the wrong number is worse than not checking.
    read: (s) => {
      const table = /^\[workspace\.package\]\r?\n([\s\S]*?)(?=^\[|\Z)/m.exec(s);
      if (!table) return null;
      const m = /^\s*version\s*=\s*"([^"]+)"/m.exec(table[1]);
      return m ? m[1] : null;
    },
  },
  {
    label: "src-tauri/tauri.conf.json",
    file: "src-tauri/tauri.conf.json",
    read: (s) => JSON.parse(s).version ?? null,
  },
];

const found = [];
const unreadable = [];

for (const src of SOURCES) {
  const abs = path.join(root, src.file);
  let value = null;
  try {
    value = src.read(fs.readFileSync(abs, "utf8"));
  } catch (error) {
    unreadable.push(`${src.label}: ${error.message}`);
    continue;
  }
  if (!value) {
    unreadable.push(`${src.label}: no version found`);
    continue;
  }
  found.push({ ...src, value });
}

if (unreadable.length) {
  console.error("Could not read a version from:");
  for (const u of unreadable) console.error(`  ${u}`);
  process.exit(1);
}

const width = Math.max(...found.map((f) => f.label.length));
const distinct = new Set(found.map((f) => f.value));

// A tag may be passed as `v0.2.0` or `0.2.0`; both are accepted, and the
// comparison is on the bare version either way.
const rawTag = process.argv[2];
const tag = rawTag ? rawTag.replace(/^v/, "") : null;

let failed = false;

if (distinct.size !== 1) {
  console.error("Version mismatch — these three must always agree:\n");
  for (const f of found) console.error(`  ${f.label.padEnd(width)}  ${f.value}`);
  console.error(
    "\nsrc-tauri/tauri.conf.json is the one that ends up in latest.json, so if\n" +
      "it is the stale one the release will publish and then be ignored by every\n" +
      "installed copy.",
  );
  failed = true;
} else if (tag && tag !== found[0].value) {
  console.error(
    `Tag/version mismatch: tag is ${rawTag} but all three files say ${found[0].value}.\n\n` +
      "Either the version bump was not committed before tagging, or the tag is\n" +
      "wrong. Retagging is the cheaper fix of the two.",
  );
  failed = true;
}

if (failed) process.exit(1);

for (const f of found) console.log(`  ${f.label.padEnd(width)}  ${f.value}`);
console.log(
  tag
    ? `\nversion ${found[0].value} is consistent and matches tag ${rawTag}`
    : `\nversion ${found[0].value} is consistent across all three files`,
);
