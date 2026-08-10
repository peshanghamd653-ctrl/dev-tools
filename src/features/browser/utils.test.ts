import { describe, expect, it } from "vitest";

import { normalizeUrl } from "./utils";

describe("normalizeUrl", () => {
  it("leaves a URL with a scheme alone", () => {
    expect(normalizeUrl("https://example.com")).toBe("https://example.com");
  });

  it("prepends http:// to a bare host:port", () => {
    expect(normalizeUrl("localhost:3000")).toBe("http://localhost:3000");
  });

  it("prepends http:// to a bare hostname", () => {
    expect(normalizeUrl("example.com")).toBe("http://example.com");
  });

  it("trims surrounding whitespace before checking", () => {
    expect(normalizeUrl("  localhost:3000  ")).toBe("http://localhost:3000");
  });
});
