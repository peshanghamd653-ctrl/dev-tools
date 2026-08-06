import { describe, expect, it } from "vitest";

import {
  normalizeSecretName,
  secretNameStatus,
  sortSecretNames,
} from "./names";

describe("normalizeSecretName", () => {
  it("trims without otherwise rewriting the name", () => {
    expect(normalizeSecretName("  vercel_token  ")).toBe("vercel_token");
    expect(normalizeSecretName("Anthropic-API-Key")).toBe("Anthropic-API-Key");
  });
});

describe("secretNameStatus", () => {
  const existing = ["anthropic-api-key", "vercel_token"];

  it("reports an empty draft", () => {
    expect(secretNameStatus(existing, "")).toBe("empty");
    expect(secretNameStatus(existing, "   ")).toBe("empty");
  });

  it("warns that an existing name will be overwritten", () => {
    expect(secretNameStatus(existing, "vercel_token")).toBe("overwrite");
    expect(secretNameStatus(existing, "  vercel_token ")).toBe("overwrite");
  });

  it("treats a different name as new", () => {
    expect(secretNameStatus(existing, "github_token")).toBe("new");
  });

  it("is case-sensitive, because the store is", () => {
    expect(secretNameStatus(existing, "VERCEL_TOKEN")).toBe("new");
  });
});

describe("sortSecretNames", () => {
  it("sorts case-insensitively without mutating the source", () => {
    const source = ["vercel_token", "Anthropic-api-key", "github_token"];
    expect(sortSecretNames(source)).toEqual([
      "Anthropic-api-key",
      "github_token",
      "vercel_token",
    ]);
    expect(source[0]).toBe("vercel_token");
  });
});
