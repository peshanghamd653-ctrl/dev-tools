/**
 * This module ships discovery only — connect, list what a server offers,
 * disconnect — never invocation (see `devos_mcp`'s module doc comment for
 * why). These tests cover the CRUD wiring and that a discovery result
 * actually reaches the row it was requested from, not the protocol itself
 * (that's `client.rs`'s job).
 */
import {
  cleanup,
  fireEvent,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/shared/ipc/client", async () => {
  const { createClientMock } = await import("@/shared/test/ipc");
  return createClientMock();
});

import { ipc, type McpServer } from "@/shared/ipc/client";
import { resetClientMock, setDesktopShell } from "@/shared/test/ipc";
import { renderWithClient } from "@/shared/test/render";
import { McpPage } from "./McpPage";

function server(over: Partial<McpServer> = {}): McpServer {
  return {
    id: "s1",
    name: "Filesystem",
    command: "npx",
    args: ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"],
    createdAt: 1_700_000_000_000,
    ...over,
  };
}

beforeEach(resetClientMock);
afterEach(cleanup);

describe("McpPage preconditions", () => {
  it("explains itself outside the desktop shell", () => {
    setDesktopShell(false);
    renderWithClient(<McpPage />);

    expect(
      screen.getByText("MCP servers need the desktop shell"),
    ).toBeInTheDocument();
    expect(ipc.mcpServers).not.toHaveBeenCalled();
  });

  it("says so when nothing is configured", async () => {
    vi.mocked(ipc.mcpServers).mockResolvedValue([]);
    renderWithClient(<McpPage />);

    expect(
      await screen.findByText("No servers configured"),
    ).toBeInTheDocument();
  });
});

describe("McpPage server list", () => {
  it("shows the launch command beside each server's name", async () => {
    vi.mocked(ipc.mcpServers).mockResolvedValue([server()]);
    renderWithClient(<McpPage />);

    expect(await screen.findByText("Filesystem")).toBeInTheDocument();
    expect(
      screen.getByText("npx -y @modelcontextprotocol/server-filesystem /tmp"),
    ).toBeInTheDocument();
  });

  it("deletes a server by id", async () => {
    vi.mocked(ipc.mcpServers).mockResolvedValue([server()]);
    vi.mocked(ipc.mcpServerDelete).mockResolvedValue(undefined);
    renderWithClient(<McpPage />);

    fireEvent.click(
      await screen.findByRole("button", { name: "Delete Filesystem" }),
    );

    await waitFor(() => expect(ipc.mcpServerDelete).toHaveBeenCalledWith("s1"));
  });
});

describe("McpPage add server dialog", () => {
  it("splits arguments on whitespace before sending them", async () => {
    vi.mocked(ipc.mcpServers).mockResolvedValue([]);
    vi.mocked(ipc.mcpServerCreate).mockResolvedValue(
      server({ name: "Weather" }),
    );
    renderWithClient(<McpPage />);

    await screen.findByText("No servers configured");
    fireEvent.click(screen.getByRole("button", { name: "Add server" }));
    const dialog = within(screen.getByRole("dialog"));

    fireEvent.change(dialog.getByLabelText("Name"), {
      target: { value: "Weather" },
    });
    fireEvent.change(dialog.getByLabelText("Command"), {
      target: { value: "uvx" },
    });
    fireEvent.change(dialog.getByLabelText("Arguments"), {
      target: { value: "  mcp-server-weather  --verbose " },
    });
    fireEvent.click(dialog.getByRole("button", { name: "Add server" }));

    await waitFor(() =>
      expect(ipc.mcpServerCreate).toHaveBeenCalledWith("Weather", "uvx", [
        "mcp-server-weather",
        "--verbose",
      ]),
    );
  });

  it("refuses to submit without a name or a command", async () => {
    vi.mocked(ipc.mcpServers).mockResolvedValue([]);
    renderWithClient(<McpPage />);

    await screen.findByText("No servers configured");
    fireEvent.click(screen.getByRole("button", { name: "Add server" }));
    const dialog = within(screen.getByRole("dialog"));

    const submit = dialog.getByRole("button", { name: "Add server" });
    expect(submit).toBeDisabled();

    fireEvent.change(dialog.getByLabelText("Name"), {
      target: { value: "Weather" },
    });
    expect(submit).toBeDisabled();

    fireEvent.change(dialog.getByLabelText("Command"), {
      target: { value: "uvx" },
    });
    expect(submit).toBeEnabled();
    expect(ipc.mcpServerCreate).not.toHaveBeenCalled();
  });
});

describe("McpPage discovery", () => {
  it("shows the tools a server reports, on the row that asked", async () => {
    vi.mocked(ipc.mcpServers).mockResolvedValue([server()]);
    vi.mocked(ipc.mcpDiscoverTools).mockResolvedValue([
      "fs-server",
      [
        {
          name: "read_file",
          description: "Read a file's contents",
          inputSchema: {},
        },
        { name: "list_dir", description: null, inputSchema: {} },
      ],
    ]);
    renderWithClient(<McpPage />);

    fireEvent.click(
      await screen.findByRole("button", { name: "Discover tools" }),
    );

    expect(await screen.findByText("fs-server")).toBeInTheDocument();
    expect(screen.getByText("read_file")).toBeInTheDocument();
    expect(screen.getByText(/Read a file's contents/)).toBeInTheDocument();
    expect(screen.getByText("list_dir")).toBeInTheDocument();
    expect(ipc.mcpDiscoverTools).toHaveBeenCalledWith("npx", [
      "-y",
      "@modelcontextprotocol/server-filesystem",
      "/tmp",
    ]);
  });

  it("says so when a server advertises no tools", async () => {
    vi.mocked(ipc.mcpServers).mockResolvedValue([server()]);
    vi.mocked(ipc.mcpDiscoverTools).mockResolvedValue(["empty-server", []]);
    renderWithClient(<McpPage />);

    fireEvent.click(
      await screen.findByRole("button", { name: "Discover tools" }),
    );

    expect(await screen.findByText("No tools advertised.")).toBeInTheDocument();
  });

  it("does not crash the row when discovery fails", async () => {
    vi.mocked(ipc.mcpServers).mockResolvedValue([server()]);
    vi.mocked(ipc.mcpDiscoverTools).mockRejectedValue(
      "could not start the server process: program not found",
    );
    renderWithClient(<McpPage />);

    fireEvent.click(
      await screen.findByRole("button", { name: "Discover tools" }),
    );

    await waitFor(() => expect(ipc.mcpDiscoverTools).toHaveBeenCalled());
    expect(screen.queryByText("No tools advertised.")).not.toBeInTheDocument();
  });
});
