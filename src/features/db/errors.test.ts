/**
 * `isWriteBlocked` decides whether the database page shows an explanatory card
 * with a way out, or a raw error dump. It gets that decision from the wording
 * of a Rust error, so the cases worth pinning down are (a) the exact strings
 * the backend produces today and (b) the ordinary failures that must *not* be
 * dressed up as a permissions problem.
 */
import { describe, expect, it } from "vitest";

import { isWriteBlocked } from "./errors";

/**
 * Verbatim from `DbError::WriteBlocked` — `#[error("write blocked: {0}")]` in
 * `crates/modules/devos-db/src/lib.rs`, filled in by `run_query` in
 * `query.rs`, stringified by Tauri. If this test starts failing, the Rust
 * variant was reworded and `errors.ts` needs the same edit.
 */
const BLOCKED =
  "write blocked: INSERT modifies the database; enable writes to run it";

describe("isWriteBlocked", () => {
  it("recognises the message the backend actually sends", () => {
    expect(isWriteBlocked(BLOCKED)).toBe(true);
  });

  it("recognises it for every keyword the classifier interpolates", () => {
    for (const keyword of ["INSERT", "UPDATE", "DELETE", "DROP", "CREATE"]) {
      expect(
        isWriteBlocked(
          `write blocked: ${keyword} modifies the database; enable writes to run it`,
        ),
      ).toBe(true);
    }
  });

  it("survives the likely rewordings of the Rust variant", () => {
    expect(isWriteBlocked("writes blocked: DELETE modifies the database")).toBe(
      true,
    );
    expect(isWriteBlocked("this write was blocked")).toBe(true);
    expect(isWriteBlocked("WRITE BLOCKED: UPDATE modifies the database")).toBe(
      true,
    );
  });

  it("also catches SQLite refusing on a read-only handle", () => {
    expect(
      isWriteBlocked(
        "database error: error returned from database: attempt to write a readonly database",
      ),
    ).toBe(true);
    expect(isWriteBlocked("database error: connection is read-only")).toBe(true);
  });

  it("leaves an ordinary SQL failure alone", () => {
    expect(isWriteBlocked("database error: no such table: usrs")).toBe(false);
    expect(isWriteBlocked("database error: syntax error near \"SELCT\"")).toBe(
      false,
    );
    expect(isWriteBlocked("not found: connection c1")).toBe(false);
  });

  it("does not fire on a half-match", () => {
    // "write" without "block" is every successful write path's vocabulary.
    expect(isWriteBlocked("database error: disk I/O error during write")).toBe(
      false,
    );
    // "block" without "write" — SQLite's busy/locking vocabulary.
    expect(isWriteBlocked("database error: database is locked, blocking")).toBe(
      false,
    );
  });

  it("handles non-string errors without throwing", () => {
    expect(isWriteBlocked(new Error(BLOCKED))).toBe(true);
    expect(isWriteBlocked(new Error("no such table"))).toBe(false);
    expect(isWriteBlocked(null)).toBe(false);
    expect(isWriteBlocked(undefined)).toBe(false);
    expect(isWriteBlocked({})).toBe(false);
  });
});
