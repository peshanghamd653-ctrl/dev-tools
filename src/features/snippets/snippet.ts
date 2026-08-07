/**
 * The parts of the snippets page that are decisions rather than markup: how a
 * tag box turns into tags, whether an editor has unsaved work, and — the one
 * worth care — *why* a search result matched.
 *
 * The backend decides *what* matches (`snippets_search`, a substring `LIKE`
 * over title/language/tags/body). `matchField` re-derives which field it was,
 * so a result whose title has nothing to do with the query can say so instead
 * of looking like a bug. The two must agree: the rules here are deliberately
 * the same ones, in the same order, as `devos-snippets/src/repo.rs`.
 */
import type { Snippet, SnippetDraft } from "@/shared/ipc/client";

/**
 * Offered in a datalist, not enforced. The backend stores whatever string it
 * is given and nothing parses it — there is no syntax highlighting — so an
 * unlisted language costs the user nothing.
 */
export const SUGGESTED_LANGUAGES = [
  "bash",
  "css",
  "dockerfile",
  "go",
  "html",
  "json",
  "javascript",
  "markdown",
  "plaintext",
  "powershell",
  "python",
  "rust",
  "sql",
  "toml",
  "typescript",
  "yaml",
] as const;

/** What the editor pane holds. `tags` is the raw text of the input box. */
export interface SnippetForm {
  id: string | null;
  title: string;
  language: string;
  tags: string;
  body: string;
}

export const EMPTY_FORM: SnippetForm = {
  id: null,
  title: "",
  language: "",
  tags: "",
  body: "",
};

/**
 * Split a tag box into tags, matching what the backend will store: split on
 * commas, trim, lowercase, drop blanks, drop repeats, keep the typing order.
 *
 * Applying the same rules here rather than trusting the round trip means the
 * chips under the box show what will be saved, not what was typed.
 */
export function parseTags(raw: string): string[] {
  const out: string[] = [];
  for (const part of raw.split(",")) {
    const tag = part.trim().toLowerCase();
    if (tag && !out.includes(tag)) out.push(tag);
  }
  return out;
}

/** Tags back into a tag box. */
export function formatTags(tags: string[]): string {
  return tags.join(", ");
}

export function formFor(snippet: Snippet): SnippetForm {
  return {
    id: snippet.id,
    title: snippet.title,
    language: snippet.language,
    tags: formatTags(snippet.tags),
    body: snippet.body,
  };
}

export function toDraft(form: SnippetForm): SnippetDraft {
  return {
    id: form.id,
    title: form.title.trim(),
    language: form.language.trim().toLowerCase(),
    tags: parseTags(form.tags),
    body: form.body,
  };
}

/**
 * Whether the form differs from the snippet it was loaded from. Compared
 * against the *normalized* form of both sides, so retyping `React` over
 * `react`, or adding a trailing comma to the tag box, is not "unsaved work".
 * A null snippet means a new one: any content at all is a change.
 */
export function isDirty(form: SnippetForm, snippet: Snippet | null): boolean {
  const draft = toDraft(form);
  if (!snippet) {
    return Boolean(draft.title || draft.body || draft.language || draft.tags.length);
  }
  return (
    draft.title !== snippet.title ||
    draft.language !== snippet.language ||
    draft.body !== snippet.body ||
    draft.tags.join(",") !== snippet.tags.join(",")
  );
}

export type SnippetMatchField = "title" | "language" | "tags" | "body";

/**
 * Which field a search term hit, or null if none did.
 *
 * Order matters: it is the order the backend's `WHERE` clause lists, and it
 * is also the order a reader cares about — a title match needs no explaining,
 * a body match does.
 */
export function matchField(
  snippet: Snippet,
  query: string,
): SnippetMatchField | null {
  const needle = query.trim().toLowerCase();
  if (!needle) return null;
  if (snippet.title.toLowerCase().includes(needle)) return "title";
  if (snippet.language.toLowerCase().includes(needle)) return "language";
  // Joined, not per-tag: the backend searches one delimited column, so
  // `t,u` matching across two tags is a quirk both sides share rather than a
  // disagreement between them.
  if (snippet.tags.join(",").toLowerCase().includes(needle)) return "tags";
  if (snippet.body.toLowerCase().includes(needle)) return "body";
  return null;
}

/** How much context to keep either side of a body match. */
const EXCERPT_PADDING = 24;

/**
 * The line a body match sits on, trimmed to something a list row can hold.
 *
 * Returns null when the term is not in the body, so the caller renders
 * nothing rather than an empty quote. Ellipses are only added where text was
 * genuinely cut.
 */
export function bodyExcerpt(body: string, query: string): string | null {
  const needle = query.trim().toLowerCase();
  if (!needle) return null;
  const at = body.toLowerCase().indexOf(needle);
  if (at === -1) return null;

  const lineStart = body.lastIndexOf("\n", at) + 1;
  const lineEnd = body.indexOf("\n", at);
  const line = body.slice(lineStart, lineEnd === -1 ? undefined : lineEnd);
  const inLine = at - lineStart;

  const from = Math.max(0, inLine - EXCERPT_PADDING);
  const to = Math.min(line.length, inLine + needle.length + EXCERPT_PADDING);
  const cut = line.slice(from, to).trim();
  return `${from > 0 ? "…" : ""}${cut}${to < line.length ? "…" : ""}`;
}

/** "12 lines · 340 chars", for the list row and the editor footer. */
export function describeBody(body: string): string {
  if (body.length === 0) return "empty";
  const lines = body.split("\n").length;
  return `${lines} ${lines === 1 ? "line" : "lines"} · ${body.length} ${
    body.length === 1 ? "char" : "chars"
  }`;
}
