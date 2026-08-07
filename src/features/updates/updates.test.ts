import { describe, expect, it } from "vitest";

import {
  describeUpdateError,
  downloadFraction,
  formatTransferred,
  isBusy,
  type UpdateState,
} from "./updates";

describe("downloadFraction", () => {
  it("reports progress against a known total", () => {
    expect(downloadFraction(0, 100)).toBe(0);
    expect(downloadFraction(50, 100)).toBe(0.5);
    expect(downloadFraction(100, 100)).toBe(1);
  });

  it("returns null when the total is unknowable", () => {
    // A server that omits Content-Length gives us no denominator. An
    // indeterminate bar is honest; a fabricated percentage is not.
    expect(downloadFraction(500, undefined)).toBeNull();
    expect(downloadFraction(500, 0)).toBeNull();
    expect(downloadFraction(500, Number.NaN)).toBeNull();
  });

  it("never exceeds 1 when more arrives than was promised", () => {
    expect(downloadFraction(150, 100)).toBe(1);
  });
});

describe("formatTransferred", () => {
  it("shows both numbers when the total is known", () => {
    expect(formatTransferred(1_048_576, 6_291_456)).toBe("1.0 MB of 6.0 MB");
  });

  it("shows only what has arrived when it is not", () => {
    expect(formatTransferred(2_097_152, undefined)).toBe("2.0 MB");
  });
});

describe("describeUpdateError", () => {
  it("turns an unreachable server into something actionable", () => {
    const message = describeUpdateError(new Error("error sending request: dns error"));
    expect(message).toMatch(/could not reach/i);
  });

  it("treats a signature failure as reportable, not retryable", () => {
    // This is the one failure that must never read as a transient glitch: it
    // means the manifest was not signed by the key this build trusts.
    const message = describeUpdateError(new Error("Invalid signature"));
    expect(message).toMatch(/signature check/i);
    expect(message).toMatch(/report/i);
    expect(message).not.toMatch(/try again/i);
  });

  it("passes anything else through rather than flattening it", () => {
    expect(describeUpdateError(new Error("404 Not Found"))).toBe("404 Not Found");
    expect(describeUpdateError("plain string failure")).toBe("plain string failure");
  });
});

describe("isBusy", () => {
  it("is true exactly while work is in flight", () => {
    const busy: UpdateState[] = [
      { kind: "checking" },
      { kind: "downloading", version: "1.0.0", downloaded: 1, total: 2 },
      { kind: "installing", version: "1.0.0" },
    ];
    const idle: UpdateState[] = [
      { kind: "idle" },
      { kind: "upToDate", checkedAt: 0 },
      { kind: "available", version: "1.0.0" },
      { kind: "failed", message: "x" },
    ];
    for (const s of busy) expect(isBusy(s), s.kind).toBe(true);
    for (const s of idle) expect(isBusy(s), s.kind).toBe(false);
  });
});
