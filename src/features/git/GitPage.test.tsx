/**
 * Git is the second thing a new user opens and the first that can be reached
 * with nothing registered. Both of its "you cannot do anything here yet"
 * states have to explain themselves and, where one exists, offer the action
 * that ends them — an empty card is where someone closes the app.
 */
import { cleanup, fireEvent, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/shared/ipc/client", async () => {
  const { createClientMock } = await import("@/shared/test/ipc");
  return createClientMock();
});

import { ipc, type GitStatus, type Project } from "@/shared/ipc/client";
import { useDialogStore } from "@/shared/stores/dialogs";
import { resetClientMock, setDesktopShell } from "@/shared/test/ipc";
import { renderWithClient } from "@/shared/test/render";
import { GitPage } from "./GitPage";

const WORKSPACE = {
  id: "w1",
  name: "Default",
  createdAt: 1_700_000_000_000,
  updatedAt: 1_700_000_000_000,
};

const PROJECT: Project = {
  id: "p1",
  workspaceId: "w1",
  name: "devos",
  path: "C:/projects/devos",
  createdAt: 1_700_000_000_000,
  updatedAt: 1_700_000_000_000,
};

/** What `git_status` returns for a folder with no `.git` directory. */
const NOT_A_REPO: GitStatus = {
  info: { isRepo: false, branch: null, upstream: null, ahead: 0n, behind: 0n },
  entries: [],
};

beforeEach(() => {
  resetClientMock();
  useDialogStore.setState({ addProjectOpen: false });
});
afterEach(cleanup);

describe("GitPage with no project", () => {
  it("says what the module is for rather than only that nothing is selected", async () => {
    vi.mocked(ipc.workspacesList).mockResolvedValue([WORKSPACE]);
    vi.mocked(ipc.projectsList).mockResolvedValue([]);
    renderWithClient(<GitPage />);

    expect(await screen.findByText("No project yet")).toBeInTheDocument();
    expect(screen.getByText(/stage, commit, switch branches/)).toBeInTheDocument();
    expect(ipc.gitStatus).not.toHaveBeenCalled();
  });

  it("offers the add-project dialog instead of dead-ending", async () => {
    vi.mocked(ipc.workspacesList).mockResolvedValue([WORKSPACE]);
    vi.mocked(ipc.projectsList).mockResolvedValue([]);
    renderWithClient(<GitPage />);

    fireEvent.click(await screen.findByRole("button", { name: "Add project" }));

    expect(useDialogStore.getState().addProjectOpen).toBe(true);
  });

  it("explains itself outside the desktop shell without firing IPC", () => {
    setDesktopShell(false);
    renderWithClient(<GitPage />);

    expect(screen.getByText("Git needs the desktop shell")).toBeInTheDocument();
    expect(ipc.gitStatus).not.toHaveBeenCalled();
  });
});

describe("GitPage on a folder that is not a repository", () => {
  it("names the folder and the command that fixes it", async () => {
    vi.mocked(ipc.workspacesList).mockResolvedValue([WORKSPACE]);
    vi.mocked(ipc.projectsList).mockResolvedValue([PROJECT]);
    vi.mocked(ipc.gitStatus).mockResolvedValue(NOT_A_REPO);
    vi.mocked(ipc.gitBranches).mockResolvedValue([]);
    vi.mocked(ipc.gitLog).mockResolvedValue([]);
    renderWithClient(<GitPage />);

    expect(
      await screen.findByText("Not a git repository"),
    ).toBeInTheDocument();
    expect(screen.getByText(/C:\/projects\/devos/)).toBeInTheDocument();
    expect(screen.getByText("git init")).toBeInTheDocument();
  });
});
