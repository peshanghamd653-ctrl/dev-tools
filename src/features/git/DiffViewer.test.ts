import { describe, expect, it } from "vitest";

import { parseUnifiedDiff } from "./diff";

const SAMPLE = `diff --git a/a.txt b/a.txt
index 1234567..89abcde 100644
--- a/a.txt
+++ b/a.txt
@@ -1,3 +1,3 @@
 keep
-old line
+new line
 tail
`;

describe("parseUnifiedDiff", () => {
  it("classifies hunks, additions, deletions, and context", () => {
    const lines = parseUnifiedDiff(SAMPLE);
    const kinds = lines.map((l) => l.kind);
    expect(kinds).toContain("hunk");
    expect(kinds).toContain("add");
    expect(kinds).toContain("del");
    expect(kinds).toContain("ctx");
  });

  it("numbers lines from the hunk header", () => {
    const lines = parseUnifiedDiff(SAMPLE);
    const ctx = lines.find((l) => l.kind === "ctx");
    expect(ctx?.oldNo).toBe(1);
    expect(ctx?.newNo).toBe(1);
    const del = lines.find((l) => l.kind === "del");
    expect(del?.oldNo).toBe(2);
    expect(del?.newNo).toBeUndefined();
    const add = lines.find((l) => l.kind === "add");
    expect(add?.newNo).toBe(2);
  });

  it("handles synthesized untracked-file diffs", () => {
    const lines = parseUnifiedDiff(
      "--- /dev/null\n+++ b/new.txt\n@@ -0,0 +1,2 @@\n+one\n+two\n",
    );
    const adds = lines.filter((l) => l.kind === "add");
    expect(adds).toHaveLength(2);
    expect(adds[1]?.newNo).toBe(2);
  });
});
