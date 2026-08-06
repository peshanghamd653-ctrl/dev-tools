/**
 * These tests exist because the same formatting used to live in four places:
 * one correct version here and three `formatSize` copies that stopped at MB.
 * The GB/TB cases below are the ones the copies got wrong — a 500 GB disk read
 * as "512000.0 MB" — so they are the load-bearing assertions, not padding.
 */
import { describe, expect, it } from "vitest";

import { formatBytes } from "./format";

const KB = 1024;
const MB = 1024 ** 2;
const GB = 1024 ** 3;
const TB = 1024 ** 4;

describe("formatBytes", () => {
  it("scales through binary units", () => {
    expect(formatBytes(512)).toBe("512 B");
    expect(formatBytes(2 * KB)).toBe("2.0 KB");
    expect(formatBytes(5 * MB)).toBe("5.0 MB");
    expect(formatBytes(13.5 * GB)).toBe("13.5 GB");
  });

  it("keeps going past MB, which the old per-page copies did not", () => {
    // The exact regression: three pages divided by 1024^2 and stopped, so a
    // half-terabyte disk was reported as "512000.0 MB".
    expect(formatBytes(500 * GB)).toBe("500 GB");
    expect(formatBytes(1.5 * TB)).toBe("1.5 TB");
    expect(formatBytes(2 * GB)).toBe("2.0 GB");
  });

  it("tops out at PB instead of inventing a unit", () => {
    expect(formatBytes(4 * 1024 ** 5)).toBe("4.0 PB");
    // Beyond the table the exponent is clamped, so the number grows, not the
    // unit — never an `undefined` suffix.
    expect(formatBytes(4096 * 1024 ** 5)).toBe("4096 PB");
  });

  it("drops the decimal once the number is three digits wide", () => {
    expect(formatBytes(512 * GB)).toBe("512 GB");
    expect(formatBytes(99 * KB)).toBe("99.0 KB");
    expect(formatBytes(100 * KB)).toBe("100 KB");
  });

  it("renders whole bytes without a fraction", () => {
    expect(formatBytes(1)).toBe("1 B");
    expect(formatBytes(34)).toBe("34 B");
    expect(formatBytes(1023)).toBe("1023 B");
    expect(formatBytes(1024)).toBe("1.0 KB");
  });

  it("never renders a raw byte count for zero or nonsense input", () => {
    expect(formatBytes(0)).toBe("0 B");
    expect(formatBytes(-1)).toBe("0 B");
    expect(formatBytes(Number.NaN)).toBe("0 B");
    expect(formatBytes(Number.POSITIVE_INFINITY)).toBe("0 B");
  });
});
