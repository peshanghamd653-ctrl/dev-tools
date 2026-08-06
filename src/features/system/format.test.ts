import { describe, expect, it } from "vitest";

import {
  formatBytes,
  formatPercent,
  formatUptime,
  loadColor,
  usageRatio,
} from "./format";

describe("formatBytes", () => {
  it("scales through binary units", () => {
    expect(formatBytes(512)).toBe("512 B");
    expect(formatBytes(2048)).toBe("2.0 KB");
    expect(formatBytes(5 * 1024 ** 2)).toBe("5.0 MB");
    expect(formatBytes(13.5 * 1024 ** 3)).toBe("13.5 GB");
  });

  it("drops the decimal once the number is three digits wide", () => {
    expect(formatBytes(512 * 1024 ** 3)).toBe("512 GB");
  });

  it("never renders a raw byte count for zero or nonsense input", () => {
    expect(formatBytes(0)).toBe("0 B");
    expect(formatBytes(-1)).toBe("0 B");
    expect(formatBytes(Number.NaN)).toBe("0 B");
  });
});

describe("formatUptime", () => {
  it("renders days, hours and minutes, skipping empty leading units", () => {
    expect(formatUptime(3 * 86_400 + 4 * 3600 + 12 * 60)).toBe("3d 4h 12m");
    expect(formatUptime(4 * 3600 + 12 * 60)).toBe("4h 12m");
    expect(formatUptime(12 * 60)).toBe("12m");
  });

  it("keeps a zero-minute reading rather than rendering nothing", () => {
    expect(formatUptime(2 * 3600)).toBe("2h");
    expect(formatUptime(60)).toBe("1m");
  });

  it("falls back to seconds below a minute", () => {
    expect(formatUptime(42)).toBe("42s");
  });
});

describe("formatPercent", () => {
  it("uses at most one decimal", () => {
    expect(formatPercent(42)).toBe("42%");
    expect(formatPercent(42.37)).toBe("42.4%");
    expect(formatPercent(99.99)).toBe("100%");
  });
});

describe("usageRatio", () => {
  it("clamps and guards against a zero total", () => {
    expect(usageRatio(1, 4)).toBe(25);
    expect(usageRatio(9, 4)).toBe(100);
    expect(usageRatio(1, 0)).toBe(0);
  });
});

describe("loadColor", () => {
  it("escalates green -> yellow -> red", () => {
    expect(loadColor(10)).toContain("emerald");
    expect(loadColor(80)).toContain("yellow");
    expect(loadColor(95)).toContain("red");
  });
});
