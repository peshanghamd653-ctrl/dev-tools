/**
 * `snippet.test.ts` covers the rules. What is untested there is the wiring —
 * and for this page the wiring *is* the feature: a copy that silently fails
 * looks exactly like one that worked, so the confirmation is pinned here
 * alongside the insert-vs-update switch that decides whether Save creates a
 * second snippet or edits the one on screen.
 */
import { cleanup, fireEvent, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/shared/ipc/client", async () => {
  const { createClientMock } = await import("@/shared/test/ipc");
  return createClientMock();
});

import { ipc, type Snippet } from "@/shared/ipc/client";
import { resetClientMock, setDesktopShell } from "@/shared/test/ipc";
import { renderWithClient } from "@/shared/test/render";
import { SnippetsPage } from "./SnippetsPage";

function snippet(over: Partial<Snippet> = {}): Snippet {
  return {
    id: "s1",
    title: "Rebase onto main",
    language: "shell",
    body: "git rebase origin/main",
    tags: ["git"],
    createdAt: 1_700_000_000_000,
    updatedAt: 1_700_000_000_000,
    ...over,
  };
}

// jsdom ships no clipboard at all, so this is a stub rather than a spy.
const writeText = vi.fn<(text: string) => Promise<void>>();

beforeEach(() => {
  resetClientMock();
  writeText.mockReset();
  writeText.mockResolvedValue(undefined);
  Object.defineProperty(navigator, "clipboard", {
    value: { writeText },
    writable: true,
    configurable: true,
  });
});
afterEach(cleanup);

describe("SnippetsPage degraded state", () => {
  it("says the library needs the shell, and asks the backend for nothing", () => {
    setDesktopShell(false);
    renderWithClient(<SnippetsPage />);

    expect(
      screen.getByText("Snippets need the desktop shell"),
    ).toBeInTheDocument();
    expect(ipc.snippetsSearch).not.toHaveBeenCalled();
  });

  it("invites a first snippet instead of showing an empty list", async () => {
    vi.mocked(ipc.snippetsSearch).mockResolvedValue([]);
    renderWithClient(<SnippetsPage />);

    expect(
      await screen.findByText(/No snippets yet/),
    ).toBeInTheDocument();
  });
});

describe("SnippetsPage copy", () => {
  it("puts the body on the clipboard and confirms it did", async () => {
    vi.mocked(ipc.snippetsSearch).mockResolvedValue([snippet()]);
    renderWithClient(<SnippetsPage />);

    fireEvent.click(
      await screen.findByRole("button", { name: "Copy Rebase onto main" }),
    );

    // The body, not the title and not a formatted blob around it.
    await waitFor(() =>
      expect(writeText).toHaveBeenCalledWith("git rebase origin/main"),
    );
  });

  it("copies the editor's body, including edits that were never saved", async () => {
    vi.mocked(ipc.snippetsSearch).mockResolvedValue([snippet()]);
    renderWithClient(<SnippetsPage />);

    fireEvent.click(await screen.findByText("Rebase onto main"));
    fireEvent.change(screen.getByLabelText("Body"), {
      target: { value: "git reset --hard" },
    });
    fireEvent.click(screen.getByRole("button", { name: /^Copy$/ }));

    await waitFor(() =>
      expect(writeText).toHaveBeenCalledWith("git reset --hard"),
    );
    expect(await screen.findByText("Copied")).toBeInTheDocument();
  });

  it("says so rather than pretending, when the clipboard refuses", async () => {
    writeText.mockRejectedValue(new Error("denied"));
    vi.mocked(ipc.snippetsSearch).mockResolvedValue([snippet()]);
    renderWithClient(<SnippetsPage />);

    fireEvent.click(
      await screen.findByRole("button", { name: "Copy Rebase onto main" }),
    );

    await waitFor(() => expect(writeText).toHaveBeenCalled());
    expect(screen.queryByText("Copied")).not.toBeInTheDocument();
  });
});

describe("SnippetsPage saving", () => {
  it("sends no id for a new snippet, so the backend inserts", async () => {
    vi.mocked(ipc.snippetsSearch).mockResolvedValue([]);
    vi.mocked(ipc.snippetSave).mockResolvedValue(snippet({ title: "New one" }));
    renderWithClient(<SnippetsPage />);

    fireEvent.change(await screen.findByLabelText("Title"), {
      target: { value: "New one" },
    });
    fireEvent.change(screen.getByLabelText("Tags"), {
      target: { value: "Git, GIT , danger" },
    });
    fireEvent.click(screen.getByRole("button", { name: /Create/ }));

    await waitFor(() =>
      expect(ipc.snippetSave).toHaveBeenCalledWith({
        id: null,
        title: "New one",
        language: "",
        tags: ["git", "danger"],
        body: "",
      }),
    );
  });

  it("sends the id of the loaded snippet, so an edit updates in place", async () => {
    vi.mocked(ipc.snippetsSearch).mockResolvedValue([snippet()]);
    vi.mocked(ipc.snippetSave).mockResolvedValue(snippet({ title: "Edited" }));
    renderWithClient(<SnippetsPage />);

    fireEvent.click(await screen.findByText("Rebase onto main"));
    fireEvent.change(screen.getByLabelText("Title"), {
      target: { value: "Edited" },
    });
    fireEvent.click(screen.getByRole("button", { name: /Save/ }));

    await waitFor(() =>
      expect(ipc.snippetSave).toHaveBeenCalledWith(
        expect.objectContaining({ id: "s1", title: "Edited" }),
      ),
    );
  });

  it("keeps Save inert until there is something to save", async () => {
    vi.mocked(ipc.snippetsSearch).mockResolvedValue([snippet()]);
    renderWithClient(<SnippetsPage />);

    const row = await screen.findByText("Rebase onto main");
    expect(screen.getByRole("button", { name: /Create/ })).toBeDisabled();

    // Loading a snippet is not an edit of it.
    fireEvent.click(row);
    expect(screen.getByRole("button", { name: /Save/ })).toBeDisabled();

    // Nor is retyping a value that normalizes back to what is stored.
    fireEvent.change(screen.getByLabelText("Language"), {
      target: { value: "SHELL" },
    });
    expect(screen.getByRole("button", { name: /Save/ })).toBeDisabled();

    fireEvent.change(screen.getByLabelText("Language"), {
      target: { value: "powershell" },
    });
    expect(screen.getByRole("button", { name: /Save/ })).toBeEnabled();
    expect(ipc.snippetSave).not.toHaveBeenCalled();
  });
});

describe("SnippetsPage search", () => {
  it("asks the backend rather than filtering what it already has", async () => {
    vi.mocked(ipc.snippetsSearch).mockResolvedValue([snippet()]);
    renderWithClient(<SnippetsPage />);

    await screen.findByText("Rebase onto main");
    fireEvent.change(screen.getByLabelText("Search snippets"), {
      target: { value: "origin" },
    });

    await waitFor(() =>
      expect(ipc.snippetsSearch).toHaveBeenCalledWith("origin"),
    );
  });

  it("explains a match the row's own text does not", async () => {
    vi.mocked(ipc.snippetsSearch).mockResolvedValue([snippet()]);
    renderWithClient(<SnippetsPage />);

    await screen.findByText("Rebase onto main");
    fireEvent.change(screen.getByLabelText("Search snippets"), {
      target: { value: "origin" },
    });

    // Nothing visible on the row contains "origin" — without this the result
    // reads as a bug in the search.
    expect(
      await screen.findByText(/in body:\s*git rebase origin\/main/),
    ).toBeInTheDocument();
  });
});
