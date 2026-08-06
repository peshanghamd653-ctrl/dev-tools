/**
 * The explorer has three modes competing for the same pane — tree preview,
 * search results, and the "pick a file" prompt — plus a lazily-loaded tree
 * where each expanded directory is its own request. The interesting failures
 * are all about which mode wins and what path a click actually asks for.
 */
import { cleanup, fireEvent, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/shared/ipc/client", async () => {
  const { createClientMock } = await import("@/shared/test/ipc");
  return createClientMock();
});
// Only reachable from the "Reveal in Explorer" button, which needs a real shell.
vi.mock("@tauri-apps/plugin-opener", () => ({ revealItemInDir: vi.fn() }));

import {
  ipc,
  type FilePreview,
  type FsEntry,
  type Project,
} from "@/shared/ipc/client";
import { resetClientMock, setDesktopShell } from "@/shared/test/ipc";
import { renderWithClient } from "@/shared/test/render";
import { FileExplorerPage } from "./FileExplorerPage";

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

function dir(name: string): FsEntry {
  return { name, isDir: true, size: 0 };
}

function file(name: string, size = 120): FsEntry {
  return { name, isDir: false, size };
}

function preview(over: Partial<FilePreview> = {}): FilePreview {
  return {
    content: "const a = 1;\nconst b = 2;",
    truncated: false,
    binary: false,
    size: 2048,
    ...over,
  };
}

/** Root listing plus whatever subdirectories the test needs, keyed by path. */
function stubTree(tree: Record<string, FsEntry[]>) {
  vi.mocked(ipc.workspacesList).mockResolvedValue([WORKSPACE]);
  vi.mocked(ipc.projectsList).mockResolvedValue([PROJECT]);
  vi.mocked(ipc.fsListDir).mockImplementation((_path, relative) =>
    Promise.resolve(tree[relative] ?? []),
  );
}

function searchBox() {
  return screen.getByPlaceholderText(/Search file names and content/);
}

beforeEach(resetClientMock);
afterEach(cleanup);

describe("FileExplorerPage preconditions", () => {
  it("explains itself outside the desktop shell", () => {
    setDesktopShell(false);
    renderWithClient(<FileExplorerPage />);

    expect(
      screen.getByText("The file explorer needs the desktop shell"),
    ).toBeInTheDocument();
    expect(ipc.fsListDir).not.toHaveBeenCalled();
  });

  it("asks for a project instead of showing an empty tree", async () => {
    vi.mocked(ipc.workspacesList).mockResolvedValue([WORKSPACE]);
    vi.mocked(ipc.projectsList).mockResolvedValue([]);
    renderWithClient(<FileExplorerPage />);

    expect(
      await screen.findByText("No project selected — add one first"),
    ).toBeInTheDocument();
    expect(ipc.fsListDir).not.toHaveBeenCalled();
  });
});

describe("FileExplorerPage tree", () => {
  it("lists the project root and waits to be told what to preview", async () => {
    stubTree({ "": [dir("src"), file("README.md")] });
    renderWithClient(<FileExplorerPage />);

    expect(await screen.findByRole("button", { name: "src" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "README.md" })).toBeInTheDocument();
    expect(screen.getByText("Select a file to preview it.")).toBeInTheDocument();
    expect(ipc.fsReadFile).not.toHaveBeenCalled();
  });

  it("only fetches a directory's contents once it is expanded", async () => {
    stubTree({ "": [dir("src")], src: [file("main.tsx")] });
    renderWithClient(<FileExplorerPage />);

    fireEvent.click(await screen.findByRole("button", { name: "src" }));

    expect(await screen.findByRole("button", { name: "main.tsx" })).toBeInTheDocument();
    expect(ipc.fsListDir).toHaveBeenCalledWith(PROJECT.path, "src");
  });

  it("asks for the nested path, not the bare file name", async () => {
    stubTree({ "": [dir("src")], src: [file("main.tsx")] });
    vi.mocked(ipc.fsReadFile).mockResolvedValue(preview());
    renderWithClient(<FileExplorerPage />);

    fireEvent.click(await screen.findByRole("button", { name: "src" }));
    fireEvent.click(await screen.findByRole("button", { name: "main.tsx" }));

    await waitFor(() =>
      expect(ipc.fsReadFile).toHaveBeenCalledWith(PROJECT.path, "src/main.tsx"),
    );
  });

  it("collapses a directory again, dropping its children from the tree", async () => {
    stubTree({ "": [dir("src")], src: [file("main.tsx")] });
    renderWithClient(<FileExplorerPage />);

    const folder = await screen.findByRole("button", { name: "src" });
    fireEvent.click(folder);
    await screen.findByRole("button", { name: "main.tsx" });

    fireEvent.click(folder);
    expect(screen.queryByRole("button", { name: "main.tsx" })).not.toBeInTheDocument();
  });
});

describe("FileExplorerPage preview", () => {
  it("numbers the lines of a text file", async () => {
    stubTree({ "": [file("notes.txt")] });
    vi.mocked(ipc.fsReadFile).mockResolvedValue(
      preview({ content: "alpha\nbeta\ngamma" }),
    );
    renderWithClient(<FileExplorerPage />);

    fireEvent.click(await screen.findByRole("button", { name: "notes.txt" }));

    const table = await screen.findByRole("table");
    const rows = within(table).getAllByRole("row");
    expect(rows).toHaveLength(3);
    expect(rows[2]).toHaveTextContent("3gamma");
  });

  it("shows the file size beside the path", async () => {
    stubTree({ "": [file("notes.txt")] });
    vi.mocked(ipc.fsReadFile).mockResolvedValue(preview({ size: 2048 }));
    renderWithClient(<FileExplorerPage />);

    fireEvent.click(await screen.findByRole("button", { name: "notes.txt" }));

    expect(await screen.findByText("2.0 KB")).toBeInTheDocument();
  });

  it("refuses to render a binary file as text", async () => {
    stubTree({ "": [file("logo.png")] });
    vi.mocked(ipc.fsReadFile).mockResolvedValue(
      preview({ binary: true, content: "", size: 5120 }),
    );
    renderWithClient(<FileExplorerPage />);

    fireEvent.click(await screen.findByRole("button", { name: "logo.png" }));

    expect(
      await screen.findByText("Binary file (5.0 KB) — no preview."),
    ).toBeInTheDocument();
    expect(screen.queryByRole("table")).not.toBeInTheDocument();
  });

  it("says when a preview was cut short", async () => {
    stubTree({ "": [file("huge.log")] });
    vi.mocked(ipc.fsReadFile).mockResolvedValue(
      preview({ truncated: true, content: "line" }),
    );
    renderWithClient(<FileExplorerPage />);

    fireEvent.click(await screen.findByRole("button", { name: "huge.log" }));

    expect(await screen.findByText(/preview truncated/)).toBeInTheDocument();
  });

  it("surfaces a read failure in the pane rather than blanking it", async () => {
    stubTree({ "": [file("locked.txt")] });
    vi.mocked(ipc.fsReadFile).mockRejectedValue("permission denied");
    renderWithClient(<FileExplorerPage />);

    fireEvent.click(await screen.findByRole("button", { name: "locked.txt" }));

    expect(await screen.findByText("permission denied")).toBeInTheDocument();
  });
});

describe("FileExplorerPage search", () => {
  it("waits for Enter before searching", async () => {
    stubTree({ "": [file("README.md")] });
    renderWithClient(<FileExplorerPage />);

    await screen.findByRole("button", { name: "README.md" });
    fireEvent.change(searchBox(), { target: { value: "config" } });

    expect(ipc.fsFind).not.toHaveBeenCalled();
    expect(screen.getByText("Select a file to preview it.")).toBeInTheDocument();
  });

  it("runs both the name and the content search, and counts each", async () => {
    stubTree({ "": [file("README.md")] });
    vi.mocked(ipc.fsFind).mockResolvedValue([
      "src/config.ts",
      "src/config.test.ts",
    ]);
    vi.mocked(ipc.indexSearch).mockResolvedValue([
      { file: "src/main.tsx", startLine: 12, snippet: "import config\nfrom" },
    ]);
    renderWithClient(<FileExplorerPage />);

    await screen.findByRole("button", { name: "README.md" });
    fireEvent.change(searchBox(), { target: { value: "config" } });
    fireEvent.keyDown(searchBox(), { key: "Enter" });

    expect(await screen.findByText("Files (2)")).toBeInTheDocument();
    expect(screen.getByText("Content (1)")).toBeInTheDocument();
    expect(screen.getByText("src/main.tsx:12")).toBeInTheDocument();
    expect(ipc.fsFind).toHaveBeenCalledWith(PROJECT.path, "config");
    expect(ipc.indexSearch).toHaveBeenCalledWith(PROJECT.path, "config");
  });

  it("points at the indexer when content search comes back empty", async () => {
    stubTree({ "": [file("README.md")] });
    vi.mocked(ipc.fsFind).mockResolvedValue([]);
    vi.mocked(ipc.indexSearch).mockResolvedValue([]);
    renderWithClient(<FileExplorerPage />);

    await screen.findByRole("button", { name: "README.md" });
    fireEvent.change(searchBox(), { target: { value: "nothing" } });
    fireEvent.keyDown(searchBox(), { key: "Enter" });

    expect(
      await screen.findByText(/No file names match/),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/if this project isn’t indexed yet/),
    ).toBeInTheDocument();
  });

  it("opens a hit from the results, leaving search behind", async () => {
    stubTree({ "": [file("README.md")] });
    vi.mocked(ipc.fsFind).mockResolvedValue(["src/config.ts"]);
    vi.mocked(ipc.indexSearch).mockResolvedValue([]);
    vi.mocked(ipc.fsReadFile).mockResolvedValue(preview());
    renderWithClient(<FileExplorerPage />);

    await screen.findByRole("button", { name: "README.md" });
    fireEvent.change(searchBox(), { target: { value: "config" } });
    fireEvent.keyDown(searchBox(), { key: "Enter" });

    fireEvent.click(await screen.findByRole("button", { name: "src/config.ts" }));

    await waitFor(() =>
      expect(ipc.fsReadFile).toHaveBeenCalledWith(PROJECT.path, "src/config.ts"),
    );
    expect(screen.queryByText("Files (1)")).not.toBeInTheDocument();
  });

  it("returns to the tree when the search is cleared", async () => {
    stubTree({ "": [file("README.md")] });
    vi.mocked(ipc.fsFind).mockResolvedValue([]);
    vi.mocked(ipc.indexSearch).mockResolvedValue([]);
    renderWithClient(<FileExplorerPage />);

    await screen.findByRole("button", { name: "README.md" });
    fireEvent.change(searchBox(), { target: { value: "config" } });
    fireEvent.keyDown(searchBox(), { key: "Enter" });
    await screen.findByText("Files (0)");

    fireEvent.click(screen.getByRole("button", { name: "Clear search" }));

    expect(screen.getByText("Select a file to preview it.")).toBeInTheDocument();
    expect(screen.queryByText("Files (0)")).not.toBeInTheDocument();
  });
});
