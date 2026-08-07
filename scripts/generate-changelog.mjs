// Generates CHANGELOG.md from the commit history.
//
//   pnpm gen:changelog
//
// The history follows Conventional Commits without exception (all 42 commits
// as of writing), which is what makes generating this preferable to
// maintaining it by hand: a hand-written changelog is a second description of
// the same work, and the two disagree the first time someone is in a hurry.
//
// ---------------------------------------------------------------------------
// What this deliberately leaves out
// ---------------------------------------------------------------------------
// Only `feat`, `fix` and `perf` reach the file, plus anything marked breaking.
// A changelog answers "what changed for me?", and `docs`, `ci`, `test`,
// `chore`, `refactor` and `spike` commits do not change anything for a user —
// including, pointedly, the `spike` commit whose own subject says the thing it
// built is not shipped.
//
// The omitted commits are counted rather than silently dropped, so the file
// says how much of the history it is not showing and where to find the rest.
// A reader who cannot tell the difference between "nothing else happened" and
// "the rest was not user-facing" has been misled by omission.
//
// ---------------------------------------------------------------------------
// Why there is no "changelog is stale" CI gate
// ---------------------------------------------------------------------------
// Regenerate-and-diff is the obvious gate, and it is what CI does for the ts-rs
// bindings. It does not work here yet, because the output is not identical
// across environments: commit links are derived from `origin`, and a checkout
// on a runner has one while a clone that was never pushed does not. CI would
// regenerate with links, compare against a file committed without them, and
// fail on every PR for a reason that has nothing to do with the PR — which is
// precisely how a required check gets switched off.
//
// The fix, when the repository has a fixed home: put it in package.json's
// `repository` field and read the base from there instead of from the remote.
// The output becomes environment-independent and the gate becomes safe to add.

import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import process from "node:process";

const root = path.resolve(import.meta.dirname, "..");
const OUT = path.join(root, "CHANGELOG.md");

const SECTIONS = [
  ["feat", "Added"],
  ["fix", "Fixed"],
  ["perf", "Performance"],
];
const INCLUDED = new Set(SECTIONS.map(([type]) => type));

// ASCII record/unit separators, written as escapes rather than literal
// bytes so no editor, linter or diff tool can quietly eat them. Commit
// bodies contain newlines and almost any printable character, so splitting
// on anything typeable would eventually corrupt an entry.
const RECORD = String.fromCharCode(30); // ASCII record separator
const FIELD = String.fromCharCode(31); // ASCII unit separator

function git(args, { quiet = false } = {}) {
  return execFileSync("git", args, {
    cwd: root,
    encoding: "utf8",
    maxBuffer: 64 * 1024 * 1024,
    // `quiet` is for calls whose failure is an expected answer rather than a
    // problem. Without it, `git remote get-url origin` prints "error: No such
    // remote 'origin'" straight to the terminal on a run that then succeeds,
    // which trains a reader to ignore the word error in this script's output.
    stdio: quiet ? ["ignore", "pipe", "ignore"] : ["ignore", "pipe", "inherit"],
  });
}

/**
 * The commit-link base, or null when there is no remote to link to.
 *
 * Deriving it from the remote rather than hardcoding it means this starts
 * producing links the moment the repository gains an origin, and produces a
 * clean file with plain short SHAs until then — rather than links to a
 * placeholder URL that would 404 for every reader.
 */
function commitBase() {
  let url;
  try {
    url = git(["remote", "get-url", "origin"], { quiet: true }).trim();
  } catch {
    return null;
  }
  const m =
    /^git@([^:]+):(.+?)(?:\.git)?$/.exec(url) ??
    /^https?:\/\/([^/]+)\/(.+?)(?:\.git)?$/.exec(url);
  return m ? `https://${m[1]}/${m[2]}/commit/` : null;
}

/** Every commit in `range`, newest first, parsed as a Conventional Commit. */
function commits(range) {
  const raw = git([
    "log",
    ...(range ? [range] : []),
    "--format=%H%x1f%h%x1f%s%x1f%b%x1e",
  ]);
  const out = [];
  for (const rec of raw.split(RECORD)) {
    const line = rec.replace(/^\n+/, "");
    if (!line.trim()) continue;
    const [sha, short, subject, body = ""] = line.split(FIELD);
    const m = /^([a-z]+)(?:\(([^)]+)\))?(!)?: (.+)$/.exec(subject);
    if (!m) continue;
    out.push({
      sha,
      short,
      type: m[1],
      scope: m[2] ?? null,
      breaking: Boolean(m[3]) || /^BREAKING CHANGE:/m.test(body),
      description: m[4],
    });
  }
  return out;
}

function releases() {
  // Tags oldest-first; each release spans from the previous tag to its own.
  const tags = git(["tag", "--list", "--sort=creatordate"])
    .split("\n")
    .map((t) => t.trim())
    .filter(Boolean);

  const spans = [];
  let previous = null;
  for (const tag of tags) {
    spans.push({
      name: tag.replace(/^v/, ""),
      range: previous ? `${previous}..${tag}` : tag,
      date: git(["log", "-1", "--format=%ad", "--date=short", tag]).trim(),
      released: true,
    });
    previous = tag;
  }

  // Anything after the newest tag — or the whole history when there are none.
  const headRange = previous ? `${previous}..HEAD` : null;
  if (commits(headRange).length > 0) {
    const version = JSON.parse(
      fs.readFileSync(path.join(root, "package.json"), "utf8"),
    ).version;
    spans.push({
      name: version,
      range: headRange,
      date: null,
      released: false,
    });
  }
  return spans.reverse(); // newest first
}

function renderRelease(release, base) {
  const all = commits(release.range);
  const shown = all.filter((c) => INCLUDED.has(c.type) || c.breaking);
  const omitted = all.length - shown.length;

  const link = (c) =>
    base ? `[\`${c.short}\`](${base}${c.sha})` : `\`${c.short}\``;
  const entry = (c) =>
    `- ${c.scope ? `**${c.scope}** — ` : ""}${c.description} ${link(c)}`;

  let out = release.released
    ? `## ${release.name} — ${release.date}\n\n`
    : `## ${release.name} — unreleased\n\n`;

  const breaking = shown.filter((c) => c.breaking);
  if (breaking.length) {
    out += `### Breaking changes\n\n${breaking.map(entry).join("\n")}\n\n`;
  }

  for (const [type, heading] of SECTIONS) {
    const group = shown.filter((c) => c.type === type && !c.breaking);
    if (group.length === 0) continue;
    out += `### ${heading}\n\n${group.map(entry).join("\n")}\n\n`;
  }

  if (omitted > 0) {
    out +=
      `_${omitted} further ${omitted === 1 ? "commit" : "commits"} in this release ` +
      `changed no user-facing behaviour (documentation, CI, tests, refactors) and ` +
      `${omitted === 1 ? "is" : "are"} not listed. \`git log\` has them._\n\n`;
  }

  return out;
}

const base = commitBase();
const spans = releases();

const header = `# Changelog

Notable changes to DevOS, newest first. Versions follow [SemVer](https://semver.org);
DevOS is pre-1.0, so the interface may still move between minor versions.

**This file is generated** from the commit history by \`pnpm gen:changelog\` —
do not edit it by hand. Only \`feat\`, \`fix\` and \`perf\` commits appear, plus
anything marked breaking: those are the ones that change something for a person
using the app. Each release says how many other commits it contains.

`;

fs.writeFileSync(OUT, header + spans.map((r) => renderRelease(r, base)).join(""), "utf8");

const total = spans.reduce((n, r) => n + commits(r.range).length, 0);
process.stderr.write(
  `wrote CHANGELOG.md — ${spans.length} release(s), ${total} commits scanned` +
    (base ? "" : "\n  no git remote yet, so commits are listed as plain SHAs rather than links") +
    "\n",
);
