import { describe, expect, it } from "vitest";

import { folderNameFromPath } from "./path";

describe("folderNameFromPath", () => {
  it("takes the last segment of a Windows path", () => {
    expect(folderNameFromPath("C:\\code\\my-project")).toBe("my-project");
  });

  it("takes the last segment of a POSIX path", () => {
    expect(folderNameFromPath("/home/peshang/code/devos")).toBe("devos");
  });

  it("ignores trailing separators", () => {
    expect(folderNameFromPath("C:\\code\\devos\\")).toBe("devos");
    expect(folderNameFromPath("/home/peshang/devos//")).toBe("devos");
  });

  it("keeps folder names containing spaces and dots", () => {
    expect(folderNameFromPath("C:\\Users\\peshang\\dev tools")).toBe(
      "dev tools",
    );
    expect(folderNameFromPath("/srv/app.v2")).toBe("app.v2");
  });

  it("handles a UNC share", () => {
    expect(folderNameFromPath("\\\\server\\share\\project")).toBe("project");
  });

  it("returns empty for paths with no usable name", () => {
    // A drive root would otherwise pre-fill the name with "C:".
    expect(folderNameFromPath("C:\\")).toBe("");
    expect(folderNameFromPath("C:")).toBe("");
    expect(folderNameFromPath("/")).toBe("");
    expect(folderNameFromPath("   ")).toBe("");
    expect(folderNameFromPath("")).toBe("");
  });
});
