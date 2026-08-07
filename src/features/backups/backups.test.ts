import { describe, expect, it } from "vitest";

import type { BackupEntry, BackupKind } from "@/shared/ipc/client";
import {
  CONFIRM_WORD,
  KIND_HINT,
  KIND_LABEL,
  formatBackupTime,
  isRestoreConfirmed,
  sortBackups,
} from "./backups";

function entry(over: Partial<BackupEntry> = {}): BackupEntry {
  return {
    name: "devos-daily-2026-08-01.db",
    path: "C:/data/backups/devos-daily-2026-08-01.db",
    kind: "daily",
    modifiedAt: 1_754_000_000_000,
    size: 1024,
    ...over,
  };
}

const ALL_KINDS: BackupKind[] = [
  "daily",
  "preMigration",
  "replaced",
  "unknown",
];

describe("backup kinds", () => {
  it("names and explains every kind the backend can send", () => {
    for (const kind of ALL_KINDS) {
      expect(KIND_LABEL[kind]).toBeTruthy();
      expect(KIND_HINT[kind]).toBeTruthy();
    }
  });

  it("tells a preserved database apart from an ordinary snapshot", () => {
    // These two are the ones a person must not confuse: one is a routine
    // copy, the other is the database a previous restore displaced.
    expect(KIND_LABEL.daily).not.toBe(KIND_LABEL.replaced);
    expect(KIND_HINT.replaced).toMatch(/undo/i);
  });
});

describe("sortBackups", () => {
  it("puts the newest first whatever order it arrives in", () => {
    const sorted = sortBackups([
      entry({ name: "old.db", modifiedAt: 1_000 }),
      entry({ name: "new.db", modifiedAt: 3_000 }),
      entry({ name: "middle.db", modifiedAt: 2_000 }),
    ]);
    expect(sorted.map((e) => e.name)).toEqual(["new.db", "middle.db", "old.db"]);
  });

  it("breaks ties by name so the order is stable, not incidental", () => {
    const sorted = sortBackups([
      entry({ name: "devos-daily-2026-08-01.db", modifiedAt: 5 }),
      entry({ name: "devos-daily-2026-08-03.db", modifiedAt: 5 }),
      entry({ name: "devos-daily-2026-08-02.db", modifiedAt: 5 }),
    ]);
    expect(sorted.map((e) => e.name)).toEqual([
      "devos-daily-2026-08-03.db",
      "devos-daily-2026-08-02.db",
      "devos-daily-2026-08-01.db",
    ]);
  });

  it("does not mutate what it was given", () => {
    const input = [entry({ modifiedAt: 1 }), entry({ modifiedAt: 2 })];
    const before = input.map((e) => e.modifiedAt);
    sortBackups(input);
    expect(input.map((e) => e.modifiedAt)).toEqual(before);
  });
});

describe("formatBackupTime", () => {
  it("renders a sortable local timestamp", () => {
    // Built from local parts so the assertion does not depend on the
    // machine's timezone, which is the whole point of a local format.
    const at = new Date(2026, 7, 6, 14, 3);
    expect(formatBackupTime(at.getTime())).toBe("2026-08-06 14:03");
  });

  it("pads single digits so the column lines up", () => {
    const at = new Date(2026, 0, 9, 4, 7);
    expect(formatBackupTime(at.getTime())).toBe("2026-01-09 04:07");
  });

  it("says the date is unknown rather than claiming 1970", () => {
    // `0` is the backend's "the filesystem would not tell me".
    for (const bad of [0, -1, Number.NaN, Number.POSITIVE_INFINITY]) {
      expect(formatBackupTime(bad)).toBe("Unknown date");
    }
  });
});

describe("isRestoreConfirmed", () => {
  it("accepts the word, however it was capitalised or padded", () => {
    for (const typed of [CONFIRM_WORD, "RESTORE", " Restore ", "\trestore\n"]) {
      expect(isRestoreConfirmed(typed)).toBe(true);
    }
  });

  it("rejects everything else, so an untouched dialog is disarmed", () => {
    for (const typed of ["", "   ", "rest", "restores", "restore now", "yes"]) {
      expect(isRestoreConfirmed(typed)).toBe(false);
    }
  });
});
