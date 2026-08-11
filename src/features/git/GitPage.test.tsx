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

import {
  ipc,
  type ConflictSides,
  type GitStatus,
  type Project,
  type RebaseStatus,
} from "@/shared/ipc/client";
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

const NOT_REBASING: RebaseStatus = {
  inProgress: false,
  step: null,
  total: null,
  branch: null,
};

const REPO: GitStatus = {
  info: { isRepo: true, branch: "main", upstream: null, ahead: 0n, behind: 0n },
  entries: [
    {
      path: "f.txt",
      origPath: null,
      staged: ".",
      unstaged: ".",
      untracked: false,
      conflicted: true,
    },
  ],
};

function stubRepoBasics(status: GitStatus) {
  vi.mocked(ipc.workspacesList).mockResolvedValue([WORKSPACE]);
  vi.mocked(ipc.projectsList).mockResolvedValue([PROJECT]);
  vi.mocked(ipc.gitStatus).mockResolvedValue(status);
  vi.mocked(ipc.gitBranches).mockResolvedValue([]);
  vi.mocked(ipc.gitLog).mockResolvedValue([]);
  vi.mocked(ipc.gitRebaseStatus).mockResolvedValue(NOT_REBASING);
}

describe("GitPage conflict resolution", () => {
  it("lists a conflicted file under its own Conflicts section", async () => {
    stubRepoBasics(REPO);
    renderWithClient(<GitPage />);

    expect(await screen.findByText("Conflicts (1)")).toBeInTheDocument();
    expect(screen.getByText("f.txt")).toBeInTheDocument();
  });

  it("shows base/ours/theirs content and resolves with a quick action", async () => {
    stubRepoBasics(REPO);
    const sides: ConflictSides = {
      base: "base\n",
      ours: "ours content\n",
      theirs: "theirs content\n",
    };
    vi.mocked(ipc.gitConflictSides).mockResolvedValue(sides);
    vi.mocked(ipc.gitResolveTheirs).mockResolvedValue(undefined);
    renderWithClient(<GitPage />);

    fireEvent.click(await screen.findByText("f.txt"));

    expect(await screen.findByText("ours content")).toBeInTheDocument();
    expect(screen.getByText("theirs content")).toBeInTheDocument();

    // Two "Keep theirs" controls exist once a conflicted file is selected —
    // the row's quick action and the panel's — so pick the panel's, the
    // last one in DOM order.
    const keepTheirsButtons = screen.getAllByRole("button", {
      name: "Keep theirs",
    });
    fireEvent.click(keepTheirsButtons[keepTheirsButtons.length - 1]!);

    await vi.waitFor(() =>
      expect(ipc.gitResolveTheirs).toHaveBeenCalledWith(
        "C:/projects/devos",
        "f.txt",
      ),
    );
  });

  it("resolves ours directly from the file row without selecting it", async () => {
    stubRepoBasics(REPO);
    vi.mocked(ipc.gitResolveOurs).mockResolvedValue(undefined);
    renderWithClient(<GitPage />);

    await screen.findByText("f.txt");
    fireEvent.click(screen.getByRole("button", { name: "Keep ours" }));

    await vi.waitFor(() =>
      expect(ipc.gitResolveOurs).toHaveBeenCalledWith(
        "C:/projects/devos",
        "f.txt",
      ),
    );
  });
});

describe("GitPage rebase in progress", () => {
  it("shows the paused step and lets it be continued", async () => {
    stubRepoBasics({
      info: { isRepo: true, branch: "main", upstream: null, ahead: 0n, behind: 0n },
      entries: [],
    });
    vi.mocked(ipc.gitRebaseStatus).mockResolvedValue({
      inProgress: true,
      step: 1,
      total: 2,
      branch: "feature",
    });
    vi.mocked(ipc.gitRebaseContinue).mockResolvedValue(undefined);
    renderWithClient(<GitPage />);

    expect(
      await screen.findByText("Rebasing feature — step 1 of 2"),
    ).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Continue" }));

    await vi.waitFor(() =>
      expect(ipc.gitRebaseContinue).toHaveBeenCalledWith("C:/projects/devos"),
    );
  });
});
