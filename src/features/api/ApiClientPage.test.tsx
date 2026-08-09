/**
 * The API client's real logic is the translation between the form and the
 * `ApiRequestSpec` the kernel receives, plus how a response is presented. Both
 * are easy to break silently — a half-typed header row leaking into the
 * request, or a JSON body arriving as one unreadable line.
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

import {
  ipc,
  type ApiEnvironment,
  type ApiResponse,
  type SavedRequest,
} from "@/shared/ipc/client";
import { resetClientMock, setDesktopShell } from "@/shared/test/ipc";
import { renderWithClient } from "@/shared/test/render";
import { ApiClientPage } from "./ApiClientPage";

function response(over: Partial<ApiResponse> = {}): ApiResponse {
  return {
    status: 200,
    headers: [
      { name: "content-type", value: "application/json" },
      { name: "x-request-id", value: "abc" },
    ],
    body: '{"ok":true}',
    truncated: false,
    durationMs: 12,
    sizeBytes: 34,
    ...over,
  };
}

const SAVED: SavedRequest = {
  id: "s1",
  name: "List users",
  collection: "Users",
  method: "POST",
  url: "https://api.example.com/v1/users",
  headers: [{ name: "authorization", value: "Bearer t" }],
  body: '{"page":1}',
  updatedAt: 1_700_000_000_000,
};

/** Both sidebar queries run on mount; every test needs them settled. */
function stubSidebar(saved: SavedRequest[] = [], history: never[] = []) {
  vi.mocked(ipc.apiRequests).mockResolvedValue(saved);
  vi.mocked(ipc.apiHistory).mockResolvedValue(history);
}

function environment(over: Partial<ApiEnvironment> = {}): ApiEnvironment {
  return {
    id: "e1",
    name: "Production",
    vars: [{ key: "API_URL", value: "https://api.example.com", secret: false }],
    active: false,
    updatedAt: 1_700_000_000_000,
    ...over,
  };
}

/** The environments query also runs on every mount alongside the sidebar. */
function stubEnvironments(environments: ApiEnvironment[] = []) {
  vi.mocked(ipc.apiEnvironments).mockResolvedValue(environments);
}

function openEnvironmentMenu() {
  fireEvent.pointerDown(screen.getByRole("button", { name: "Environment" }), {
    pointerId: 1,
    button: 0,
  });
}

const urlField = () =>
  screen.getByPlaceholderText("https://api.example.com/v1/users");
const methodField = () => screen.getByRole("combobox");
const sendButton = () => screen.getByRole("button", { name: "Send" });

beforeEach(resetClientMock);
afterEach(cleanup);

describe("ApiClientPage outside the desktop shell", () => {
  it("explains itself and asks for nothing", () => {
    setDesktopShell(false);
    renderWithClient(<ApiClientPage />);

    expect(
      screen.getByText("The API client needs the desktop shell"),
    ).toBeInTheDocument();
    expect(ipc.apiRequests).not.toHaveBeenCalled();
    expect(ipc.apiHistory).not.toHaveBeenCalled();
  });
});

describe("ApiClientPage request building", () => {
  it("will not send without a URL", async () => {
    stubSidebar();
    renderWithClient(<ApiClientPage />);

    await screen.findByText("Sent requests appear here.");
    expect(sendButton()).toBeDisabled();

    fireEvent.change(urlField(), { target: { value: "https://example.com" } });
    expect(sendButton()).toBeEnabled();
  });

  it("treats a whitespace-only URL as no URL", async () => {
    stubSidebar();
    renderWithClient(<ApiClientPage />);

    await screen.findByText("Sent requests appear here.");
    fireEvent.change(urlField(), { target: { value: "   " } });

    expect(sendButton()).toBeDisabled();
  });

  it("drops half-typed header rows and sends an empty body as null", async () => {
    stubSidebar();
    vi.mocked(ipc.apiSend).mockResolvedValue(response());
    renderWithClient(<ApiClientPage />);

    await screen.findByText("Sent requests appear here.");
    fireEvent.change(methodField(), { target: { value: "POST" } });
    fireEvent.change(urlField(), {
      target: { value: "  https://api.example.com/v1/users  " },
    });

    fireEvent.click(screen.getByRole("button", { name: "Add header" }));
    fireEvent.click(screen.getByRole("button", { name: "Add header" }));
    const names = screen.getAllByPlaceholderText("Header");
    const values = screen.getAllByPlaceholderText("Value");
    fireEvent.change(names[0]!, { target: { value: "authorization" } });
    fireEvent.change(values[0]!, { target: { value: "Bearer t" } });
    // Second row left blank — an artifact of clicking "Add header", not a header.
    fireEvent.change(values[1]!, { target: { value: "orphan" } });

    fireEvent.click(sendButton());

    await waitFor(() =>
      expect(ipc.apiSend).toHaveBeenCalledWith({
        method: "POST",
        url: "https://api.example.com/v1/users",
        headers: [{ name: "authorization", value: "Bearer t" }],
        body: null,
      }),
    );
  });

  it("sends the body verbatim when there is one", async () => {
    stubSidebar();
    vi.mocked(ipc.apiSend).mockResolvedValue(response());
    renderWithClient(<ApiClientPage />);

    await screen.findByText("Sent requests appear here.");
    fireEvent.change(urlField(), { target: { value: "https://example.com" } });
    fireEvent.click(screen.getByRole("button", { name: "Body" }));
    fireEvent.change(screen.getByPlaceholderText('{"raw": "request body"}'), {
      target: { value: '{"page": 1}' },
    });

    fireEvent.click(sendButton());

    await waitFor(() =>
      expect(ipc.apiSend).toHaveBeenCalledWith(
        expect.objectContaining({ body: '{"page": 1}' }),
      ),
    );
  });

  it("sends on Enter in the URL field", async () => {
    stubSidebar();
    vi.mocked(ipc.apiSend).mockResolvedValue(response());
    renderWithClient(<ApiClientPage />);

    await screen.findByText("Sent requests appear here.");
    fireEvent.change(urlField(), { target: { value: "https://example.com" } });
    fireEvent.keyDown(urlField(), { key: "Enter" });

    await waitFor(() => expect(ipc.apiSend).toHaveBeenCalledTimes(1));
  });
});

describe("ApiClientPage response rendering", () => {
  async function send(result: ApiResponse) {
    stubSidebar();
    vi.mocked(ipc.apiSend).mockResolvedValue(result);
    const view = renderWithClient(<ApiClientPage />);

    await screen.findByText("Sent requests appear here.");
    fireEvent.change(urlField(), { target: { value: "https://example.com" } });
    fireEvent.click(sendButton());
    await screen.findByText(String(result.status));
    return view;
  }

  it("shows nothing but a prompt before the first send", async () => {
    stubSidebar();
    renderWithClient(<ApiClientPage />);

    expect(
      await screen.findByText("Send a request to see the response."),
    ).toBeInTheDocument();
  });

  it("pretty-prints a JSON body instead of showing the wire format", async () => {
    const { container } = await send(
      response({ body: '{"ok":true,"items":[1,2]}' }),
    );

    expect(container.querySelector("pre")).toHaveTextContent(
      '{ "ok": true, "items": [ 1, 2 ] }',
    );
    expect(container.querySelector("pre")?.textContent).toBe(
      '{\n  "ok": true,\n  "items": [\n    1,\n    2\n  ]\n}',
    );
  });

  it("leaves a non-JSON body exactly as it arrived", async () => {
    const { container } = await send(
      response({ body: "plain text, {not json", status: 500 }),
    );

    expect(container.querySelector("pre")?.textContent).toBe(
      "plain text, {not json",
    );
  });

  it("reports timing and size next to the status", async () => {
    await send(response());

    expect(screen.getByText("12 ms · 34 B")).toBeInTheDocument();
  });

  it("flags a truncated body rather than silently shortening it", async () => {
    await send(response({ truncated: true }));

    expect(screen.getByText(/· truncated/)).toBeInTheDocument();
  });

  it("switches to the response headers on demand", async () => {
    await send(response());

    fireEvent.click(screen.getByRole("button", { name: "Headers (2)" }));

    expect(screen.getByText("content-type")).toBeInTheDocument();
    expect(screen.getByText("application/json")).toBeInTheDocument();
  });
});

describe("ApiClientPage saved requests", () => {
  it("loads a saved request back into the form", async () => {
    stubSidebar([SAVED]);
    renderWithClient(<ApiClientPage />);

    fireEvent.click(await screen.findByText("List users"));

    expect(urlField()).toHaveValue("https://api.example.com/v1/users");
    expect(methodField()).toHaveValue("POST");
    expect(
      screen.getByRole("button", { name: "Headers (1)" }),
    ).toBeInTheDocument();
  });

  it("groups saved requests under their collection", async () => {
    stubSidebar([
      SAVED,
      { ...SAVED, id: "s2", name: "Get user", collection: "Users" },
    ]);
    const { container } = renderWithClient(<ApiClientPage />);

    await screen.findByText(/List users/);
    const sidebar = within(container.querySelector("aside")!);
    expect(sidebar.getByText("Users")).toBeInTheDocument();
    expect(sidebar.getByText("Get user")).toBeInTheDocument();
  });
});

describe("ApiClientPage environment picker", () => {
  it("shows no environment when none is active", async () => {
    stubSidebar();
    stubEnvironments([environment({ active: false })]);
    renderWithClient(<ApiClientPage />);

    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: "Environment" }),
      ).toHaveTextContent("No environment"),
    );
  });

  it("shows the active environment's name", async () => {
    stubSidebar();
    stubEnvironments([environment({ active: true })]);
    renderWithClient(<ApiClientPage />);

    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: "Environment" }),
      ).toHaveTextContent("Production"),
    );
  });

  it("activates a different environment from the list", async () => {
    stubSidebar();
    stubEnvironments([
      environment({ id: "e1", name: "Production", active: true }),
      environment({ id: "e2", name: "Staging", active: false }),
    ]);
    vi.mocked(ipc.apiEnvironmentSetActive).mockResolvedValue(undefined);
    renderWithClient(<ApiClientPage />);

    await screen.findByRole("button", { name: "Environment" });
    openEnvironmentMenu();
    fireEvent.click(await screen.findByRole("menuitem", { name: "Staging" }));

    await waitFor(() =>
      expect(ipc.apiEnvironmentSetActive).toHaveBeenCalledWith("e2"),
    );
  });

  it("clears the active environment via 'No environment'", async () => {
    stubSidebar();
    stubEnvironments([environment({ active: true })]);
    vi.mocked(ipc.apiEnvironmentSetActive).mockResolvedValue(undefined);
    renderWithClient(<ApiClientPage />);

    await screen.findByRole("button", { name: "Environment" });
    openEnvironmentMenu();
    fireEvent.click(
      await screen.findByRole("menuitem", { name: /No environment/ }),
    );

    await waitFor(() =>
      expect(ipc.apiEnvironmentSetActive).toHaveBeenCalledWith(null),
    );
  });
});

describe("ApiClientPage environments dialog", () => {
  async function openManageDialog() {
    await screen.findByRole("button", { name: "Environment" });
    openEnvironmentMenu();
    fireEvent.click(
      await screen.findByRole("menuitem", { name: "Manage environments…" }),
    );
    return within(screen.getByRole("dialog"));
  }

  it("creates a new environment", async () => {
    stubSidebar();
    stubEnvironments([]);
    vi.mocked(ipc.apiEnvironmentCreate).mockResolvedValue(
      environment({ id: "new", name: "Staging", vars: [] }),
    );
    renderWithClient(<ApiClientPage />);

    const dialog = await openManageDialog();
    fireEvent.change(dialog.getByLabelText("Name"), {
      target: { value: "Staging" },
    });
    fireEvent.click(dialog.getByRole("button", { name: "Create" }));

    await waitFor(() =>
      expect(ipc.apiEnvironmentCreate).toHaveBeenCalledWith("Staging"),
    );
  });

  it("adds a variable to an existing environment and saves", async () => {
    stubSidebar();
    const env = environment({ vars: [] });
    stubEnvironments([env]);
    vi.mocked(ipc.apiEnvironmentUpdate).mockResolvedValue({
      ...env,
      vars: [
        { key: "API_URL", value: "https://api.example.com", secret: false },
      ],
    });
    renderWithClient(<ApiClientPage />);

    const dialog = await openManageDialog();
    fireEvent.click(dialog.getByText("Production"));
    fireEvent.click(dialog.getByRole("button", { name: "Add variable" }));
    fireEvent.change(dialog.getByPlaceholderText("API_URL"), {
      target: { value: "API_URL" },
    });
    fireEvent.change(dialog.getByPlaceholderText("Value"), {
      target: { value: "https://api.example.com" },
    });
    fireEvent.click(dialog.getByRole("button", { name: "Save" }));

    await waitFor(() =>
      expect(ipc.apiEnvironmentUpdate).toHaveBeenCalledWith(
        "e1",
        "Production",
        [{ key: "API_URL", value: "https://api.example.com", secret: false }],
      ),
    );
  });

  it("deletes an environment", async () => {
    stubSidebar();
    stubEnvironments([environment()]);
    vi.mocked(ipc.apiEnvironmentDelete).mockResolvedValue(undefined);
    renderWithClient(<ApiClientPage />);

    const dialog = await openManageDialog();
    fireEvent.click(dialog.getByRole("button", { name: "Delete Production" }));

    await waitFor(() =>
      expect(ipc.apiEnvironmentDelete).toHaveBeenCalledWith("e1"),
    );
  });
});
