/**
 * `backups.test.ts` covers the rules. This covers the thing the rules exist
 * for: that a destructive, unrecoverable action cannot be reached by clicking
 * through, and that when it *is* reached it stages exactly the file that was
 * chosen and nothing about the live database changes yet.
 */
import { cleanup, fireEvent, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/shared/ipc/client", async () => {
  const { createClientMock } = await import("@/shared/test/ipc");
  return createClientMock();
});
vi.mock("@tauri-apps/plugin-opener", () => ({ revealItemInDir: vi.fn() }));

import { revealItemInDir } from "@tauri-apps/plugin-opener";

import {
  ipc,
  type BackupEntry,
  type BackupListing,
  type PendingRestore,
} from "@/shared/ipc/client";
import { resetClientMock, setDesktopShell } from "@/shared/test/ipc";
import { renderWithClient } from "@/shared/test/render";
import { BackupsSection } from "./BackupsSection";

function entry(over: Partial<BackupEntry> = {}): BackupEntry {
  return {
    name: "devos-daily-2026-08-01.db",
    path: "C:/data/backups/devos-daily-2026-08-01.db",
    kind: "daily",
    modifiedAt: new Date(2026, 7, 1, 9, 30).getTime(),
    size: 2_097_152,
    ...over,
  };
}

function listing(over: Partial<BackupListing> = {}): BackupListing {
  return {
    dir: "C:/data/backups",
    entries: [entry()],
    pending: null,
    ...over,
  };
}

function pending(over: Partial<PendingRestore> = {}): PendingRestore {
  return {
    source: "devos-daily-2026-08-01.db",
    requestedAt: Date.now(),
    size: 2_097_152,
    ...over,
  };
}

beforeEach(() => {
  resetClientMock();
  vi.mocked(revealItemInDir).mockReset();
  vi.mocked(revealItemInDir).mockResolvedValue(undefined);
});
afterEach(cleanup);

describe("BackupsSection listing", () => {
  it("needs the desktop shell, and asks the backend for nothing without it", () => {
    setDesktopShell(false);
    renderWithClient(<BackupsSection />);

    expect(screen.getByText(/can't reach them/)).toBeInTheDocument();
    expect(ipc.backupsList).not.toHaveBeenCalled();
  });

  it("shows each backup's kind, timestamp and size", async () => {
    vi.mocked(ipc.backupsList).mockResolvedValue(
      listing({
        entries: [
          entry(),
          entry({
            name: "devos-premigration-20260802-101500-v0003.db",
            kind: "preMigration",
            modifiedAt: new Date(2026, 7, 2, 10, 15).getTime(),
            size: 1024,
          }),
        ],
      }),
    );
    renderWithClient(<BackupsSection />);

    expect(await screen.findByText("Daily")).toBeInTheDocument();
    expect(screen.getByText("Before migration")).toBeInTheDocument();
    expect(screen.getByText(/2026-08-01 09:30/)).toBeInTheDocument();
    expect(screen.getByText(/2\.0 MB/)).toBeInTheDocument();
  });

  it("invites the first backup instead of showing an empty list", async () => {
    vi.mocked(ipc.backupsList).mockResolvedValue(listing({ entries: [] }));
    renderWithClient(<BackupsSection />);

    expect(await screen.findByText(/No backups yet/)).toBeInTheDocument();
  });

  it("reveals a backup at its own path, not the folder's", async () => {
    vi.mocked(ipc.backupsList).mockResolvedValue(listing());
    renderWithClient(<BackupsSection />);

    fireEvent.click(
      await screen.findByRole("button", {
        name: "Reveal devos-daily-2026-08-01.db in Explorer",
      }),
    );
    expect(revealItemInDir).toHaveBeenCalledWith(
      "C:/data/backups/devos-daily-2026-08-01.db",
    );
  });
});

describe("BackupsSection restore confirmation", () => {
  async function openDialog() {
    vi.mocked(ipc.backupsList).mockResolvedValue(listing());
    renderWithClient(<BackupsSection />);
    fireEvent.click(await screen.findByRole("button", { name: "Restore…" }));
    return screen.getByRole("button", { name: "Stage restore" });
  }

  it("opens disarmed, and stays disarmed until the word is typed", async () => {
    const confirm = await openDialog();
    expect(confirm).toBeDisabled();

    fireEvent.change(screen.getByLabelText(/Type .* to confirm/), {
      target: { value: "rest" },
    });
    expect(confirm).toBeDisabled();
    expect(ipc.backupRestoreStage).not.toHaveBeenCalled();

    fireEvent.change(screen.getByLabelText(/Type .* to confirm/), {
      target: { value: "restore" },
    });
    expect(confirm).toBeEnabled();
  });

  it("says what is replaced and where the current database goes", async () => {
    await openDialog();

    // Both halves matter: the threat, and the fact that it is survivable.
    expect(
      screen.getByText(/Replace your database with this backup\?/),
    ).toBeInTheDocument();
    expect(screen.getByText(/devos-replaced-…/)).toBeInTheDocument();
    expect(
      screen.getByText(/Nothing happens until DevOS restarts/),
    ).toBeInTheDocument();
  });

  it("stages the chosen file, and never restarts on its own", async () => {
    const confirm = await openDialog();
    vi.mocked(ipc.backupRestoreStage).mockResolvedValue(pending());

    fireEvent.change(screen.getByLabelText(/Type .* to confirm/), {
      target: { value: "restore" },
    });
    fireEvent.click(confirm);

    await waitFor(() =>
      expect(ipc.backupRestoreStage).toHaveBeenCalledWith(
        "devos-daily-2026-08-01.db",
      ),
    );
    // Staging is reversible; restarting is what makes it happen. The user
    // asks for that separately.
    expect(ipc.backupRestart).not.toHaveBeenCalled();
  });

  it("forgets what was typed when the dialog is dismissed", async () => {
    const confirm = await openDialog();
    fireEvent.change(screen.getByLabelText(/Type .* to confirm/), {
      target: { value: "restore" },
    });
    expect(confirm).toBeEnabled();

    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    fireEvent.click(await screen.findByRole("button", { name: "Restore…" }));

    expect(
      screen.getByRole("button", { name: "Stage restore" }),
    ).toBeDisabled();
  });
});

describe("BackupsSection staged restore", () => {
  it("warns that a restore is armed and that nothing has changed yet", async () => {
    vi.mocked(ipc.backupsList).mockResolvedValue(
      listing({ pending: pending() }),
    );
    renderWithClient(<BackupsSection />);

    expect(await screen.findByText("Restore staged")).toBeInTheDocument();
    expect(screen.getByText(/Nothing has changed yet/)).toBeInTheDocument();
    // A second, contradictory request cannot be made on top of the first.
    expect(screen.getByRole("button", { name: "Restore…" })).toBeDisabled();
  });

  it("cancels the staged restore without restarting", async () => {
    vi.mocked(ipc.backupsList).mockResolvedValue(
      listing({ pending: pending() }),
    );
    vi.mocked(ipc.backupRestoreCancel).mockResolvedValue(undefined);
    renderWithClient(<BackupsSection />);

    fireEvent.click(
      await screen.findByRole("button", { name: "Cancel restore" }),
    );

    await waitFor(() => expect(ipc.backupRestoreCancel).toHaveBeenCalled());
    expect(ipc.backupRestart).not.toHaveBeenCalled();
  });

  it("restarts only when asked", async () => {
    vi.mocked(ipc.backupsList).mockResolvedValue(
      listing({ pending: pending() }),
    );
    vi.mocked(ipc.backupRestart).mockResolvedValue(undefined);
    renderWithClient(<BackupsSection />);

    fireEvent.click(await screen.findByRole("button", { name: "Restart now" }));
    await waitFor(() => expect(ipc.backupRestart).toHaveBeenCalled());
  });
});
