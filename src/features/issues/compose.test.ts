import { describe, expect, it } from "vitest";

import type { IssueTarget } from "@/shared/ipc/client";
import {
  classifyIssueError,
  clampRect,
  copyPlan,
  defaultTargetKey,
  finalBody,
  initialIssueBody,
  isDrawableRect,
  normalizeRect,
  normalizeTitle,
  osName,
  pointerToImage,
  rectFromDrag,
  remotesLabel,
  strokeWidth,
  systemContextBlock,
  targetChoices,
} from "./compose";

function target(over: Partial<IssueTarget> = {}): IssueTarget {
  return { owner: "octocat", name: "hello-world", remote: "origin", ...over };
}

describe("normalizeRect", () => {
  it("keeps a down-right drag as it was drawn", () => {
    expect(normalizeRect({ x: 10, y: 20 }, { x: 40, y: 60 })).toEqual({
      x: 10,
      y: 20,
      width: 30,
      height: 40,
    });
  });

  it("never produces a negative width from an up-left drag", () => {
    // A negative width fills nothing on a canvas, so this is the difference
    // between a redaction the user thinks they made and one that exists.
    expect(normalizeRect({ x: 40, y: 60 }, { x: 10, y: 20 })).toEqual({
      x: 10,
      y: 20,
      width: 30,
      height: 40,
    });
  });

  it("handles the two mixed diagonals", () => {
    expect(normalizeRect({ x: 40, y: 20 }, { x: 10, y: 60 })).toEqual({
      x: 10,
      y: 20,
      width: 30,
      height: 40,
    });
    expect(normalizeRect({ x: 10, y: 60 }, { x: 40, y: 20 })).toEqual({
      x: 10,
      y: 20,
      width: 30,
      height: 40,
    });
  });

  it("collapses a click into a zero-size rect rather than throwing", () => {
    expect(normalizeRect({ x: 5, y: 5 }, { x: 5, y: 5 })).toEqual({
      x: 5,
      y: 5,
      width: 0,
      height: 0,
    });
  });
});

describe("clampRect", () => {
  const image = { width: 100, height: 80 };

  it("leaves a rect that already fits alone", () => {
    const rect = { x: 10, y: 10, width: 20, height: 20 };
    expect(clampRect(rect, image)).toEqual(rect);
  });

  it("trims a rect that runs off the right and bottom edges", () => {
    expect(clampRect({ x: 90, y: 70, width: 40, height: 40 }, image)).toEqual({
      x: 90,
      y: 70,
      width: 10,
      height: 10,
    });
  });

  it("pulls a negative origin back onto the bitmap", () => {
    expect(clampRect({ x: -10, y: -5, width: 30, height: 30 }, image)).toEqual({
      x: 0,
      y: 0,
      width: 30,
      height: 30,
    });
  });
});

describe("rectFromDrag", () => {
  it("normalizes and clamps in one step", () => {
    expect(
      rectFromDrag({ x: 90, y: 70 }, { x: 500, y: 500 }, { width: 100, height: 80 }),
    ).toEqual({ x: 90, y: 70, width: 10, height: 10 });
  });
});

describe("isDrawableRect", () => {
  it("rejects a stray click", () => {
    expect(isDrawableRect({ x: 0, y: 0, width: 0, height: 0 })).toBe(false);
    expect(isDrawableRect({ x: 0, y: 0, width: 2, height: 40 })).toBe(false);
  });

  it("accepts a real drag", () => {
    expect(isDrawableRect({ x: 0, y: 0, width: 40, height: 12 })).toBe(true);
  });
});

describe("pointerToImage", () => {
  it("scales a point from the displayed box up to bitmap pixels", () => {
    expect(
      pointerToImage(
        { x: 100, y: 50 },
        { width: 800, height: 450 },
        { width: 1600, height: 900 },
      ),
    ).toEqual({ x: 200, y: 100 });
  });

  it("clamps a pointer dragged outside the element", () => {
    expect(
      pointerToImage(
        { x: -20, y: 999 },
        { width: 800, height: 450 },
        { width: 1600, height: 900 },
      ),
    ).toEqual({ x: 0, y: 900 });
  });

  it("does not divide by an unlaid-out element", () => {
    expect(
      pointerToImage({ x: 5, y: 5 }, { width: 0, height: 0 }, { width: 100, height: 100 }),
    ).toEqual({ x: 5, y: 5 });
  });
});

describe("strokeWidth", () => {
  it("scales the outline with the bitmap so it stays visible when shrunk", () => {
    expect(strokeWidth(2560)).toBeGreaterThan(strokeWidth(960));
  });

  it("never goes below a visible minimum", () => {
    expect(strokeWidth(100)).toBe(2);
    expect(strokeWidth(0)).toBe(2);
    expect(strokeWidth(Number.NaN)).toBe(2);
  });
});

describe("osName", () => {
  it("names the desktop families DevOS runs on", () => {
    expect(osName("Mozilla/5.0 (Windows NT 10.0; Win64; x64)")).toBe("Windows");
    expect(osName("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)")).toBe(
      "macOS",
    );
    expect(osName("Mozilla/5.0 (X11; Linux x86_64)")).toBe("Linux");
  });

  it("says Unknown rather than guessing", () => {
    expect(osName("")).toBe("Unknown");
    expect(osName("something else entirely")).toBe("Unknown");
  });
});

describe("systemContextBlock", () => {
  const facts = {
    appVersion: "0.1.0",
    userAgent: "Mozilla/5.0 (Windows NT 10.0; Win64; x64)",
    shot: { width: 2560, height: 1440 },
  };

  it("reports the version, the OS and the capture size", () => {
    const block = systemContextBlock(facts);
    expect(block).toContain("- DevOS: 0.1.0");
    expect(block).toContain("- OS: Windows");
    expect(block).toContain("- Screenshot: 2560 × 1440");
  });

  it("says the user may edit or delete it", () => {
    expect(systemContextBlock(facts)).toContain("edit or delete");
  });

  it("omits a fact it does not have instead of inventing one", () => {
    const block = systemContextBlock({ ...facts, appVersion: null, shot: null });
    expect(block).not.toContain("DevOS:");
    expect(block).not.toContain("Screenshot:");
    expect(block).not.toContain("unknown");
    expect(block).toContain("- OS: Windows");
  });

  it("never carries the database path or terminal output", () => {
    // dbPath is an absolute path with the account name in it, and the terminal
    // ring buffer is where an exported API_KEY= lands. Neither is in scope.
    const block = systemContextBlock(facts);
    expect(block.toLowerCase()).not.toContain("dbpath");
    expect(block).not.toContain("\\");
    expect(block.toLowerCase()).not.toContain("terminal");
  });
});

describe("initialIssueBody", () => {
  const facts = {
    appVersion: "0.1.0",
    userAgent: "Windows NT 10.0",
    shot: null,
  };

  it("leaves room above the block for the user's own words", () => {
    expect(initialIssueBody(facts).startsWith("\n\n")).toBe(true);
  });

  it("still contains the block verbatim", () => {
    expect(initialIssueBody(facts)).toContain(systemContextBlock(facts));
  });
});

describe("finalBody", () => {
  it("drops the blank lines an untouched draft starts with", () => {
    expect(finalBody("\n\n---\nbody")).toBe("---\nbody");
  });

  it("keeps the interior of the text exactly as typed", () => {
    expect(finalBody("first\n\n  indented\n\nlast")).toBe(
      "first\n\n  indented\n\nlast",
    );
  });
});

describe("normalizeTitle", () => {
  it("flattens a pasted multi-line title", () => {
    expect(normalizeTitle("  crash on\n  save  ")).toBe("crash on save");
  });

  it("returns empty for whitespace so the submit button stays disabled", () => {
    expect(normalizeTitle("   \n ")).toBe("");
  });
});

describe("targetChoices", () => {
  it("keeps one row per repository, not per remote", () => {
    const [only, ...rest] = targetChoices([
      target({ remote: "origin" }),
      target({ remote: "push" }),
    ]);
    expect(rest).toEqual([]);
    expect(only?.remotes).toEqual(["origin", "push"]);
  });

  it("folds the casing GitHub itself ignores", () => {
    const [only, ...rest] = targetChoices([
      target({ owner: "Octocat", name: "Hello-World", remote: "origin" }),
      target({ owner: "octocat", name: "hello-world", remote: "upstream" }),
    ]);
    expect(rest).toEqual([]);
    // The first spelling seen is what the user is shown.
    expect(only?.label).toBe("Octocat/Hello-World");
    expect(only?.remotes).toEqual(["origin", "upstream"]);
  });

  it("keeps genuinely different repositories apart", () => {
    const choices = targetChoices([
      target({ owner: "octocat", name: "hello-world" }),
      target({ owner: "acme", name: "hello-world", remote: "upstream" }),
    ]);
    expect(choices.map((c) => c.label)).toEqual([
      "octocat/hello-world",
      "acme/hello-world",
    ]);
  });

  it("drops an entry that could not be filed against", () => {
    expect(targetChoices([target({ owner: "" })])).toEqual([]);
    expect(targetChoices([target({ name: "  " })])).toEqual([]);
  });

  it("does not repeat a remote name", () => {
    const [only] = targetChoices([
      target({ remote: "origin" }),
      target({ remote: "origin" }),
    ]);
    expect(only?.remotes).toEqual(["origin"]);
  });

  it("survives a remote git did not name", () => {
    const [only] = targetChoices([target({ remote: "" })]);
    expect(only?.remotes).toEqual([]);
    expect(only && remotesLabel(only)).toBe("");
  });
});

describe("remotesLabel", () => {
  it("lists every remote that resolved to the repository", () => {
    const [only] = targetChoices([
      target({ remote: "origin" }),
      target({ remote: "upstream" }),
    ]);
    expect(only && remotesLabel(only)).toBe("origin, upstream");
  });
});

describe("defaultTargetKey", () => {
  it("prefers origin over a fork's upstream", () => {
    const choices = targetChoices([
      target({ owner: "acme", name: "app", remote: "upstream" }),
      target({ owner: "me", name: "app", remote: "origin" }),
    ]);
    expect(defaultTargetKey(choices)).toBe("me/app");
  });

  it("falls back to the first when nothing is called origin", () => {
    const choices = targetChoices([
      target({ owner: "acme", name: "app", remote: "upstream" }),
      target({ owner: "me", name: "app", remote: "fork" }),
    ]);
    expect(defaultTargetKey(choices)).toBe("acme/app");
  });

  it("picks nothing when git found no GitHub remote", () => {
    expect(defaultTargetKey([])).toBeNull();
  });
});

describe("classifyIssueError", () => {
  it("separates a missing token from a rejected one", () => {
    expect(classifyIssueError("no GitHub token configured")).toBe(
      "unconfigured",
    );
    expect(classifyIssueError("GitHub rejected the token (HTTP 401)")).toBe(
      "auth",
    );
    expect(classifyIssueError("no GitHub token configured")).not.toBe("auth");
  });

  it("reads GitHub's own wording for a bad credential", () => {
    expect(
      classifyIssueError('GitHub API error (HTTP 401): {"message":"Bad credentials"}'),
    ).toBe("auth");
  });

  it("treats a 404 as its own case", () => {
    expect(
      classifyIssueError('GitHub API error (HTTP 404): {"message":"Not Found"}'),
    ).toBe("notFound");
    expect(classifyIssueError("repository not found")).toBe("notFound");
  });

  it("does not read a status code out of another error's response body", () => {
    expect(
      classifyIssueError(
        'GitHub API error (HTTP 500): {"documentation_url":"/rest#http-404"}',
      ),
    ).toBe("other");
    expect(classifyIssueError("request failed: dns error, host not found")).toBe(
      "other",
    );
  });

  it("does not send a rate-limited user to rotate a working token", () => {
    expect(
      classifyIssueError(
        'GitHub API error (HTTP 403): {"message":"API rate limit exceeded"}',
      ),
    ).toBe("other");
    expect(
      classifyIssueError(
        'GitHub API error (HTTP 403): {"message":"You have exceeded a secondary rate limit"}',
      ),
    ).toBe("other");
  });

  it("tolerates a reworded message", () => {
    expect(classifyIssueError(new Error("401 Unauthorized"))).toBe("auth");
    expect(classifyIssueError("missing token")).toBe("unconfigured");
  });

  it("falls back to other for everything else", () => {
    expect(classifyIssueError(new Error("request failed: dns error"))).toBe(
      "other",
    );
    expect(classifyIssueError(undefined)).toBe("other");
  });
});

describe("copyPlan", () => {
  it("copies the flattened canvas whenever the webview can", () => {
    expect(copyPlan({ annotated: true, webviewClipboard: true })).toBe("canvas");
    expect(copyPlan({ annotated: false, webviewClipboard: true })).toBe(
      "canvas",
    );
  });

  it("falls back to the captured file only when nothing was drawn", () => {
    expect(copyPlan({ annotated: false, webviewClipboard: false })).toBe("file");
  });

  it("refuses rather than copy a file the user redacted a copy of", () => {
    // The file on disk still holds everything the black rectangles cover.
    expect(copyPlan({ annotated: true, webviewClipboard: false })).toBe(
      "refuse",
    );
  });
});
