import { describe, expect, it } from "vitest";

import type { AuditEntry, AuditLog } from "@/shared/ipc/client";
import {
  describeAction,
  describeActor,
  describeCoverage,
  formatAuditDate,
  formatAuditTime,
  isRefusal,
} from "./audit";

function entry(over: Partial<AuditEntry> = {}): AuditEntry {
  return {
    id: 1,
    actor: "ai",
    action: "ai.tool.approved",
    detail: "write_file: src/new.rs",
    createdAt: new Date(2026, 7, 6, 14, 3).getTime(),
    ...over,
  };
}

function log(over: Partial<AuditLog> = {}): AuditLog {
  return {
    entries: [entry()],
    total: 1,
    oldest: new Date(2026, 7, 6, 14, 3).getTime(),
    retentionDays: 90,
    ...over,
  };
}

describe("describeAction", () => {
  it("labels every action the kernel emits", () => {
    expect(describeAction("ai.tool.approved")).toBe("AI tool approved");
    expect(describeAction("ai.tool.denied")).toBe("AI tool denied");
    expect(describeAction("secret.set")).toBe("Secret stored");
    expect(describeAction("secret.deleted")).toBe("Secret deleted");
    expect(describeAction("db.write")).toBe("Database write");
    expect(describeAction("backup.restored")).toBe("Database restored");
    expect(describeAction("backup.restore_refused")).toBe("Restore refused");
    expect(describeAction("issue.created")).toBe("Issue filed");
  });

  /**
   * `action` crosses IPC as a string, so a frontend older than the backend it
   * boots against will meet identifiers it has never heard of. A security
   * record must still render those as something a person can act on — the one
   * unreadable outcome is the one that must not happen.
   */
  it("derives a readable label for an action it has never seen", () => {
    expect(describeAction("plugin.installed")).toBe("Plugin installed");
    expect(describeAction("workspace.member_removed")).toBe(
      "Workspace member removed",
    );
    expect(describeAction("")).toBe("Unknown event");
  });
});

describe("describeActor", () => {
  it("names the actor in the person the reader thinks in", () => {
    expect(describeActor("user")).toBe("You");
    expect(describeActor("ai")).toBe("AI assistant");
    expect(describeActor("system")).toBe("DevOS");
  });

  it("passes an unrecognised actor through rather than dropping it", () => {
    expect(describeActor("plugin:formatter")).toBe("plugin:formatter");
  });
});

describe("isRefusal", () => {
  it("reads the outcome off the action, not out of the detail text", () => {
    expect(isRefusal(entry({ action: "ai.tool.denied" }))).toBe(true);
    expect(isRefusal(entry({ action: "backup.restore_refused" }))).toBe(true);
    expect(isRefusal(entry({ action: "ai.tool.approved" }))).toBe(false);
    expect(isRefusal(entry({ action: "db.write" }))).toBe(false);
  });

  /**
   * The detail of an approved call can quote a refusal — the model was asked
   * to run `git push --force || echo denied`, say. Classifying on prose would
   * flag it; classifying on the identifier cannot.
   */
  it("is not fooled by an approved call whose detail mentions a denial", () => {
    expect(
      isRefusal(
        entry({
          action: "ai.tool.approved",
          detail: "run_command: echo 'denied by the user'",
        }),
      ),
    ).toBe(false);
  });
});

describe("formatAuditTime", () => {
  it("renders a sortable local timestamp", () => {
    expect(formatAuditTime(new Date(2026, 7, 6, 14, 3).getTime())).toBe(
      "2026-08-06 14:03",
    );
    expect(formatAuditDate(new Date(2026, 7, 6, 14, 3).getTime())).toBe(
      "2026-08-06",
    );
  });

  it("does not render a missing timestamp as 1970", () => {
    expect(formatAuditTime(0)).toBe("Unknown date");
    expect(formatAuditTime(Number.NaN)).toBe("Unknown date");
    expect(formatAuditTime(-1)).toBe("Unknown date");
  });
});

/**
 * The retention policy is something the user did not choose and cannot see
 * from the rows, and there are *two* invisible truncations — age and page
 * size. These assert both are said out loud rather than inferred.
 */
describe("describeCoverage", () => {
  it("states the window even when there is nothing to show", () => {
    const sentence = describeCoverage(
      log({ entries: [], total: 0, oldest: null }),
    );
    expect(sentence).toContain("kept for 90 days");
    expect(sentence).not.toContain("0 entries");
  });

  it("says how far back the record actually reaches", () => {
    const sentence = describeCoverage(
      log({
        entries: [entry(), entry({ id: 2 })],
        total: 2,
        oldest: new Date(2026, 4, 10, 9, 0).getTime(),
      }),
    );
    expect(sentence).toContain("2 entries.");
    expect(sentence).toContain("reaches back to 2026-05-10");
    expect(sentence).toContain("kept for 90 days");
  });

  it("admits when the list on screen is a slice, not the whole table", () => {
    const sentence = describeCoverage(
      log({ entries: [entry(), entry({ id: 2 })], total: 1412 }),
    );
    expect(sentence).toContain("Showing the newest 2 of 1412 entries.");
  });

  it("does not pluralise a single entry", () => {
    expect(describeCoverage(log({ total: 1 }))).toContain("1 entry.");
  });

  /** The window comes from the backend, so the sentence follows it. */
  it("reports the retention the backend actually keeps, not a hardcoded one", () => {
    expect(describeCoverage(log({ retentionDays: 30 }))).toContain(
      "kept for 30 days",
    );
  });
});
