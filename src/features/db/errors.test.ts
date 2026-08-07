/**
 * `isWriteBlocked` decides whether the database page shows an explanatory card
 * with a way out, or a raw error dump. It takes that decision from the `kind`
 * discriminant on `DbErrorDto`, so the cases worth pinning down are (a) the
 * exact payloads the backend produces, (b) the ordinary failures that must
 * *not* be dressed up as a permissions problem, and (c) rejections that are
 * not the DTO at all, which is where the deliberate choice lives: an
 * unrecognized shape is a generic failure, never a blocked write.
 */
import { describe, expect, it } from "vitest";

import type { DbErrorDto } from "@/shared/ipc/bindings/DbErrorDto";
import { dbErrorMessage, isWriteBlocked } from "./errors";

/**
 * Verbatim from `DbErrorDto::from(DbError::WriteBlocked(..))` —
 * `crates/modules/devos-db/src/error.rs`, whose own test pins this JSON. The
 * message text is deliberately *not* what the classification reads.
 */
const BLOCKED: DbErrorDto = {
  kind: "writeBlocked",
  message: "write blocked: INSERT modifies the database; enable writes to run it",
};

describe("isWriteBlocked", () => {
  it("recognises the payload the backend actually sends", () => {
    expect(isWriteBlocked(BLOCKED)).toBe(true);
  });

  it("reads the discriminant, not the prose", () => {
    // The wording is free to change — including losing the words the old
    // string matcher depended on — without touching the classification.
    expect(
      isWriteBlocked({ kind: "writeBlocked", message: "nope, read-only" }),
    ).toBe(true);
    expect(isWriteBlocked({ kind: "writeBlocked", message: "" })).toBe(true);
    // …and prose alone can no longer promote an error it does not belong to.
    expect(
      isWriteBlocked({
        kind: "database",
        message: "database error: write blocked by another process",
      }),
    ).toBe(false);
  });

  it("covers SQLite's own refusal, which the backend maps to the same kind", () => {
    // `WITH x AS (…) INSERT …` gets past classification and is stopped by the
    // read-only connection instead; `DbErrorDto::from` reports it as
    // writeBlocked so the UI does not depend on which guard fired.
    expect(
      isWriteBlocked({
        kind: "writeBlocked",
        message:
          "database error: error returned from database: (code: 8) attempt to write a readonly database",
      }),
    ).toBe(true);
  });

  it("leaves the other kinds alone", () => {
    for (const kind of ["database", "invalid", "notFound"] as const) {
      expect(isWriteBlocked({ kind, message: "something went wrong" })).toBe(
        false,
      );
    }
    expect(
      isWriteBlocked({
        kind: "database",
        message: "database error: no such table: usrs",
      }),
    ).toBe(false);
  });

  it("treats a rejection that is not a DbErrorDto as an ordinary failure", () => {
    // The decision, tested: no string fallback. A panic, a serialization
    // failure or a stale frontend cannot reach the write-blocked card, and
    // that is preferable to a card that tells the user to enable writes for
    // something unrelated.
    expect(
      isWriteBlocked(
        "write blocked: INSERT modifies the database; enable writes to run it",
      ),
    ).toBe(false);
    expect(isWriteBlocked(new Error("write blocked: DELETE"))).toBe(false);
    expect(isWriteBlocked("attempt to write a readonly database")).toBe(false);
    expect(isWriteBlocked(null)).toBe(false);
    expect(isWriteBlocked(undefined)).toBe(false);
    expect(isWriteBlocked({})).toBe(false);
  });

  it("rejects a near-miss shape rather than trusting a stray field", () => {
    expect(isWriteBlocked({ kind: "writeBlocked" })).toBe(false);
    expect(isWriteBlocked({ kind: "WriteBlocked", message: "x" })).toBe(false);
    expect(isWriteBlocked({ kind: "write_blocked", message: "x" })).toBe(false);
    expect(isWriteBlocked({ kind: "writeblocked", message: "x" })).toBe(false);
    expect(isWriteBlocked({ kind: 7, message: "x" })).toBe(false);
    expect(isWriteBlocked({ kind: "writeBlocked", message: 7 })).toBe(false);
    // `in` would find this on Object.prototype; a value lookup does not.
    expect(isWriteBlocked({ kind: "toString", message: "x" })).toBe(false);
  });
});

describe("dbErrorMessage", () => {
  it("shows the backend's own message when there is one", () => {
    expect(dbErrorMessage(BLOCKED)).toBe(BLOCKED.message);
    expect(
      dbErrorMessage({
        kind: "database",
        message: "database error: no such table: usrs",
      }),
    ).toBe("database error: no such table: usrs");
  });

  it("still says something readable for a rejection it does not recognise", () => {
    // Losing the classification must not also lose the text — this is what
    // makes the no-fallback choice cheap.
    expect(dbErrorMessage("kernel panicked")).toBe("kernel panicked");
    expect(dbErrorMessage(new Error("boom"))).toBe("Error: boom");
    expect(dbErrorMessage({ kind: "writeBlocked" })).toBe("[object Object]");
  });
});
