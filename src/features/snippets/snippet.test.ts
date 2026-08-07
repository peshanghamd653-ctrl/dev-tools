import { describe, expect, it } from "vitest";

import type { Snippet } from "@/shared/ipc/client";
import {
  bodyExcerpt,
  describeBody,
  EMPTY_FORM,
  formatTags,
  formFor,
  isDirty,
  matchField,
  parseTags,
  toDraft,
} from "./snippet";

function snippet(over: Partial<Snippet> = {}): Snippet {
  return {
    id: "s1",
    title: "Rebase onto main",
    language: "shell",
    body: "git rebase origin/main",
    tags: ["git", "danger"],
    createdAt: 1_700_000_000_000,
    updatedAt: 1_700_000_000_000,
    ...over,
  };
}

describe("parseTags", () => {
  it("splits, trims, lowercases and dedupes", () => {
    expect(parseTags("React, hooks , REACT")).toEqual(["react", "hooks"]);
  });

  it("survives the punctuation a half-typed tag box contains", () => {
    expect(parseTags("")).toEqual([]);
    expect(parseTags("   ")).toEqual([]);
    expect(parseTags(",,,")).toEqual([]);
    expect(parseTags("git,")).toEqual(["git"]);
    expect(parseTags(",git")).toEqual(["git"]);
  });

  it("round-trips through the box it came from", () => {
    expect(parseTags(formatTags(["git", "danger"]))).toEqual(["git", "danger"]);
    expect(formatTags([])).toBe("");
  });
});

describe("toDraft", () => {
  it("normalizes the way the backend will, so the UI shows what gets saved", () => {
    const draft = toDraft({
      id: null,
      title: "  Reset a branch  ",
      language: "  Shell ",
      tags: "Git, git , danger",
      body: "  git reset --hard\n",
    });
    expect(draft.title).toBe("Reset a branch");
    expect(draft.language).toBe("shell");
    expect(draft.tags).toEqual(["git", "danger"]);
  });

  it("leaves the body byte-for-byte — indentation is the content here", () => {
    expect(toDraft({ ...EMPTY_FORM, title: "x", body: "  a\n\n" }).body).toBe(
      "  a\n\n",
    );
  });

  it("carries the id through, since it is the insert-vs-update switch", () => {
    expect(toDraft(formFor(snippet())).id).toBe("s1");
    expect(toDraft(EMPTY_FORM).id).toBeNull();
  });
});

describe("isDirty", () => {
  it("is false for a form loaded straight from its snippet", () => {
    expect(isDirty(formFor(snippet()), snippet())).toBe(false);
  });

  it("notices a change in any field", () => {
    const base = snippet();
    expect(isDirty({ ...formFor(base), title: "Other" }, base)).toBe(true);
    expect(isDirty({ ...formFor(base), language: "python" }, base)).toBe(true);
    expect(isDirty({ ...formFor(base), body: "other" }, base)).toBe(true);
    expect(isDirty({ ...formFor(base), tags: "git" }, base)).toBe(true);
  });

  it("ignores edits that normalize away, so Save does not light up for nothing", () => {
    const base = snippet();
    expect(isDirty({ ...formFor(base), title: "  Rebase onto main  " }, base)).toBe(
      false,
    );
    expect(isDirty({ ...formFor(base), language: "SHELL" }, base)).toBe(false);
    expect(isDirty({ ...formFor(base), tags: "git, danger, " }, base)).toBe(false);
    expect(isDirty({ ...formFor(base), tags: "GIT,  DANGER" }, base)).toBe(false);
  });

  it("treats an empty new form as clean and any content as dirty", () => {
    expect(isDirty(EMPTY_FORM, null)).toBe(false);
    expect(isDirty({ ...EMPTY_FORM, title: "x" }, null)).toBe(true);
    expect(isDirty({ ...EMPTY_FORM, body: "x" }, null)).toBe(true);
    expect(isDirty({ ...EMPTY_FORM, tags: "x" }, null)).toBe(true);
    // Whitespace alone is not work worth warning about.
    expect(isDirty({ ...EMPTY_FORM, title: "   " }, null)).toBe(false);
  });
});

describe("matchField", () => {
  it("reports the field a term was found in, most obvious first", () => {
    expect(matchField(snippet(), "rebase")).toBe("title");
    expect(matchField(snippet(), "shell")).toBe("language");
    expect(matchField(snippet(), "danger")).toBe("tags");
    expect(matchField(snippet(), "origin")).toBe("body");
  });

  it("matches case-insensitively in both directions, like the backend", () => {
    expect(matchField(snippet(), "REBASE")).toBe("title");
    expect(matchField(snippet({ title: "REBASE" }), "rebase")).toBe("title");
  });

  it("matches inside a word — the reason search is LIKE and not FTS5", () => {
    const ts = snippet({
      title: "Debounced input",
      body: "const x = useQueryClient();",
      tags: [],
      language: "typescript",
    });
    expect(matchField(ts, "Query")).toBe("body");
    expect(matchField(ts, "script")).toBe("language");
  });

  it("returns null for a term in no field, and for a blank query", () => {
    expect(matchField(snippet(), "kubernetes")).toBeNull();
    expect(matchField(snippet(), "")).toBeNull();
    expect(matchField(snippet(), "   ")).toBeNull();
  });

  it("is one substring, not an AND of terms — the same limit the backend has", () => {
    // Both words are in the body ("git rebase origin/main"), but not adjacent.
    expect(matchField(snippet(), "rebase main")).toBeNull();
    expect(matchField(snippet(), "rebase origin")).toBe("body");
    expect(matchField(snippet(), "rebase onto")).toBe("title");
  });
});

describe("bodyExcerpt", () => {
  it("returns the matching line, not the whole body", () => {
    const body = "line one\nconst timer = setTimeout(fn, 300);\nline three";
    expect(bodyExcerpt(body, "setTimeout")).toBe(
      "const timer = setTimeout(fn, 300);",
    );
  });

  it("elides only the side it actually cut", () => {
    const long = `${"a".repeat(60)}NEEDLE${"b".repeat(60)}`;
    const out = bodyExcerpt(long, "needle");
    expect(out?.startsWith("…")).toBe(true);
    expect(out?.endsWith("…")).toBe(true);
    expect(out).toContain("NEEDLE");
    expect(bodyExcerpt("NEEDLE trailing", "needle")?.startsWith("…")).toBe(false);
  });

  it("renders nothing rather than an empty quote when there is no match", () => {
    expect(bodyExcerpt("git rebase", "kubernetes")).toBeNull();
    expect(bodyExcerpt("git rebase", "")).toBeNull();
    expect(bodyExcerpt("", "git")).toBeNull();
  });
});

describe("describeBody", () => {
  it("counts lines and characters, singular where it should be", () => {
    expect(describeBody("one line")).toBe("1 line · 8 chars");
    expect(describeBody("a\nb\nc")).toBe("3 lines · 5 chars");
    expect(describeBody("x")).toBe("1 line · 1 char");
  });

  it("says empty instead of '1 line · 0 chars'", () => {
    expect(describeBody("")).toBe("empty");
  });
});
