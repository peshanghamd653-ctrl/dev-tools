/**
 * The database page is the one screen in DevOS that can destroy user data, and
 * the only thing standing between a typo and a dropped table is the read-only
 * toggle. These tests are written so that they fail if that affordance is
 * weakened: if the toggle ever defaults to "on", if the warning banner stops
 * appearing, or if a refused write degrades into a raw error dump.
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

import type { DbErrorDto } from "@/shared/ipc/bindings/DbErrorDto";
import {
  ipc,
  type DbConnection,
  type DbSchema,
  type QueryResult,
} from "@/shared/ipc/client";
import { resetClientMock, setDesktopShell } from "@/shared/test/ipc";
import { renderWithClient } from "@/shared/test/render";
import { DatabasePage } from "./DatabasePage";

const CONNECTION: DbConnection = {
  id: "c1",
  name: "App database",
  driver: "sqlite",
  path: "C:/projects/app/data.db",
  createdAt: 1_700_000_000_000,
};

const SCHEMA: DbSchema = { tables: [], sizeBytes: 4096 };

function queryResult(over: Partial<QueryResult> = {}): QueryResult {
  return {
    columns: ["id", "email"],
    rows: [["1", "ada@example.com"]],
    rowCount: 1,
    rowsAffected: 0,
    durationMs: 3,
    truncated: false,
    readOnly: true,
    ...over,
  };
}

/**
 * The real backend refuses a write with this exact payload: `DbError::WriteBlocked`
 * mapped to `DbErrorDto`, which is what `db_query` rejects with. The page reads
 * `kind`; `message` is only ever displayed.
 */
const BLOCKED: DbErrorDto = {
  kind: "writeBlocked",
  message: "write blocked: INSERT modifies the database; enable writes to run it",
};

/** An ordinary SQL mistake — same envelope, different discriminant. */
const SQL_ERROR: DbErrorDto = {
  kind: "database",
  message: "database error: no such table: usrs",
};

/** Mount the page and get to the state where the Run button is usable. */
async function renderConnected() {
  vi.mocked(ipc.dbConnections).mockResolvedValue([CONNECTION]);
  vi.mocked(ipc.dbSchema).mockResolvedValue(SCHEMA);

  renderWithClient(<DatabasePage />);

  fireEvent.click(await screen.findByRole("button", { name: /^App database/ }));
  await screen.findByText(/0 tables/);
}

function typeSql(sql: string) {
  fireEvent.change(screen.getByPlaceholderText("SELECT * FROM sqlite_master;"), {
    target: { value: sql },
  });
}

/**
 * Queried by its fixed accessible name, not by its label: the label *is* the
 * state, so matching on it means a test can only find the toggle if it already
 * knows which way it is set. `aria-label` + `aria-pressed` is what makes the
 * control nameable — asserting the visible text stays a separate concern.
 */
function writeToggle() {
  return screen.getByRole("button", { name: "Allow SQL writes" });
}

beforeEach(resetClientMock);
afterEach(cleanup);

describe("DatabasePage outside the desktop shell", () => {
  it("explains itself instead of firing IPC that cannot work", async () => {
    setDesktopShell(false);
    renderWithClient(<DatabasePage />);

    expect(
      screen.getByText("The database manager needs the desktop shell"),
    ).toBeInTheDocument();
    expect(ipc.dbConnections).not.toHaveBeenCalled();
  });
});

describe("DatabasePage with nothing connected", () => {
  /**
   * The first-run state used to say "Select a connection to start querying"
   * beside an empty list, which is an instruction the user cannot follow. It
   * has to name what the module wants and offer the way to give it.
   */
  it("asks for a SQLite file rather than for a selection", async () => {
    vi.mocked(ipc.dbConnections).mockResolvedValue([]);
    renderWithClient(<DatabasePage />);

    expect(await screen.findByText("No databases yet")).toBeInTheDocument();
    expect(
      screen.queryByText("Select a connection to start querying."),
    ).not.toBeInTheDocument();
  });

  it("opens the add-connection dialog from the empty pane", async () => {
    vi.mocked(ipc.dbConnections).mockResolvedValue([]);
    renderWithClient(<DatabasePage />);

    // Two controls carry that name: the icon in the sidebar header, which is
    // always there, and the empty-state call to action, which is the one a
    // first-run user will actually see.
    const buttons = await screen.findAllByRole("button", {
      name: "Add connection",
    });
    fireEvent.click(buttons.at(-1) as HTMLElement);

    const dialog = within(await screen.findByRole("dialog"));
    expect(dialog.getByLabelText("File path")).toBeInTheDocument();
  });
});

describe("DatabasePage write safety", () => {
  it("starts read-only, with no warning on screen", async () => {
    await renderConnected();

    const toggle = writeToggle();
    expect(toggle).toHaveTextContent("Read-only");
    expect(toggle).toHaveAttribute("aria-pressed", "false");
    expect(screen.queryByText(/There is no undo/)).not.toBeInTheDocument();
  });

  it("keeps one accessible name in both states, so it can always be found", async () => {
    await renderConnected();

    // The visible label flips; the name a screen reader (or a test) uses to
    // address the control does not. Without this, "the write toggle" is only
    // reachable by guessing its current state.
    const before = writeToggle();
    fireEvent.click(before);
    expect(writeToggle()).toBe(before);
    expect(before).toHaveTextContent("Writes enabled");
    expect(before).toHaveAttribute("aria-pressed", "true");
  });

  it("sends allowWrite=false unless the user opts in", async () => {
    await renderConnected();
    vi.mocked(ipc.dbQuery).mockResolvedValue(queryResult());

    typeSql("SELECT * FROM users");
    fireEvent.click(screen.getByRole("button", { name: "Run" }));

    await waitFor(() =>
      expect(ipc.dbQuery).toHaveBeenCalledWith("c1", "SELECT * FROM users", false),
    );
  });

  it("warns on screen once writes are enabled, and then sends allowWrite=true", async () => {
    await renderConnected();
    vi.mocked(ipc.dbQuery).mockResolvedValue(queryResult({ readOnly: false }));

    fireEvent.click(writeToggle());

    const toggle = writeToggle();
    expect(toggle).toHaveTextContent("Writes enabled");
    expect(toggle).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByText(/There is no undo/)).toBeInTheDocument();

    typeSql("DELETE FROM users");
    fireEvent.click(screen.getByRole("button", { name: "Run" }));

    await waitFor(() =>
      expect(ipc.dbQuery).toHaveBeenCalledWith("c1", "DELETE FROM users", true),
    );
  });

  it("turns the toggle back off, taking the warning with it", async () => {
    await renderConnected();

    fireEvent.click(writeToggle());
    expect(screen.getByText(/There is no undo/)).toBeInTheDocument();

    fireEvent.click(writeToggle());
    expect(writeToggle()).toHaveTextContent("Read-only");
    expect(screen.queryByText(/There is no undo/)).not.toBeInTheDocument();
  });
});

describe("DatabasePage error rendering", () => {
  it("explains a refused write and points at the toggle, keeping the raw message", async () => {
    await renderConnected();
    vi.mocked(ipc.dbQuery).mockRejectedValue(BLOCKED);

    typeSql("INSERT INTO users VALUES (1)");
    fireEvent.click(screen.getByRole("button", { name: "Run" }));

    expect(
      await screen.findByText(/this statement writes to the database/),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: /Enable writes/ }),
    ).toBeInTheDocument();
    // The explanation replaces the raw dump as the headline, but does not hide it.
    expect(screen.getByText(BLOCKED.message)).toBeInTheDocument();
  });

  it("offers a one-click way out of the blocked state", async () => {
    await renderConnected();
    vi.mocked(ipc.dbQuery).mockRejectedValue(BLOCKED);

    typeSql("INSERT INTO users VALUES (1)");
    fireEvent.click(screen.getByRole("button", { name: "Run" }));
    fireEvent.click(await screen.findByRole("button", { name: /Enable writes/ }));

    expect(writeToggle()).toHaveTextContent("Writes enabled");
    expect(screen.getByText(/There is no undo/)).toBeInTheDocument();
  });

  it("does not dress an ordinary SQL error up as a permissions problem", async () => {
    await renderConnected();
    vi.mocked(ipc.dbQuery).mockRejectedValue(SQL_ERROR);

    typeSql("SELECT * FROM usrs");
    fireEvent.click(screen.getByRole("button", { name: "Run" }));

    expect(await screen.findByText("Query failed")).toBeInTheDocument();
    expect(screen.getByText(SQL_ERROR.message)).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /Enable writes/ }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByText(/this statement writes to the database/),
    ).not.toBeInTheDocument();
  });

  /**
   * Not every rejection is a `DbErrorDto` — a panic in the command layer, or a
   * frontend older than the backend it is talking to, arrives as a bare
   * string. It renders as a plain failure with its text intact, and pointedly
   * does *not* reach the write-blocked card even though the words are there:
   * a wrong explanation offering to switch writes on is worse than no
   * explanation.
   */
  it("keeps an unrecognised rejection readable without guessing at it", async () => {
    await renderConnected();
    vi.mocked(ipc.dbQuery).mockRejectedValue(
      "write blocked: INSERT modifies the database; enable writes to run it",
    );

    typeSql("INSERT INTO users VALUES (1)");
    fireEvent.click(screen.getByRole("button", { name: "Run" }));

    expect(await screen.findByText("Query failed")).toBeInTheDocument();
    expect(screen.getByText(BLOCKED.message)).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /Enable writes/ }),
    ).not.toBeInTheDocument();
  });
});

describe("DatabasePage results", () => {
  it("spells a NULL out rather than leaving the cell blank", async () => {
    await renderConnected();
    vi.mocked(ipc.dbQuery).mockResolvedValue(
      queryResult({ rows: [["1", null]], rowCount: 1 }),
    );

    typeSql("SELECT * FROM users");
    fireEvent.click(screen.getByRole("button", { name: "Run" }));

    // Singular "row", and a NULL that reads as NULL instead of an empty cell.
    expect(await screen.findByText("1 row · 3 ms")).toBeInTheDocument();
    const cells = within(screen.getByRole("table")).getAllByRole("cell");
    expect(cells[0]).toHaveTextContent("1");
    expect(cells[1]).toHaveTextContent("NULL");
  });

  it("reports a write as rows affected, badged as a write", async () => {
    await renderConnected();
    vi.mocked(ipc.dbQuery).mockResolvedValue(
      queryResult({
        columns: [],
        rows: [],
        rowCount: 0,
        rowsAffected: 4,
        readOnly: false,
      }),
    );

    fireEvent.click(writeToggle());
    typeSql("DELETE FROM users WHERE id > 10");
    fireEvent.click(screen.getByRole("button", { name: "Run" }));

    expect(await screen.findByText("write")).toBeInTheDocument();
    expect(screen.getByText("4 rows affected")).toBeInTheDocument();
  });

  it("says when a result was capped rather than showing a silent partial", async () => {
    await renderConnected();
    vi.mocked(ipc.dbQuery).mockResolvedValue(
      queryResult({
        rows: Array.from({ length: 500 }, (_, i) => [String(i), null]),
        rowCount: 500,
        truncated: true,
      }),
    );

    typeSql("SELECT * FROM events");
    fireEvent.click(screen.getByRole("button", { name: "Run" }));

    expect(await screen.findByText(/showing first 500 rows/)).toBeInTheDocument();
  });
});
