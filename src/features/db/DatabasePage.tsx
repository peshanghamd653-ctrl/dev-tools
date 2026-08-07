import { useState } from "react";
import {
  ChevronDown,
  ChevronRight,
  Database,
  Eye,
  Play,
  Plus,
  ShieldAlert,
  ShieldCheck,
  Table2,
  Trash2,
  TriangleAlert,
} from "lucide-react";
import { toast } from "sonner";

import {
  isDesktopShell,
  type DbConnection,
  type QueryResult,
} from "@/shared/ipc/client";
import { formatBytes } from "@/shared/lib/format";
import { cn } from "@/shared/lib/utils";
import { Badge } from "@/shared/ui/badge";
import { Button } from "@/shared/ui/button";
import { Card, CardContent } from "@/shared/ui/card";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/shared/ui/dialog";
import { Input } from "@/shared/ui/input";
import { Label } from "@/shared/ui/label";
import { Skeleton } from "@/shared/ui/skeleton";
import { dbErrorMessage, isWriteBlocked } from "./errors";
import {
  useDbConnect,
  useDbConnectionDelete,
  useDbConnections,
  useDbQuery,
  useDbSchema,
  useDbTableRows,
} from "./hooks";

/** How many rows a table click pulls in — mirrored in the results notice. */
const TABLE_ROW_LIMIT = 500;

/**
 * The write toggle's visible label is its *state* ("Read-only" / "Writes
 * enabled"), which makes it unnameable: nothing can ask for "the write toggle"
 * without already knowing which way it is set. This is the app's most
 * safety-critical control, so it carries a fixed accessible name and reports
 * its state through `aria-pressed` — the way a toggle button is supposed to.
 */
const WRITE_TOGGLE_LABEL = "Allow SQL writes";

export function DatabasePage() {
  const { data: connections } = useDbConnections();
  const connect = useDbConnect();
  const remove = useDbConnectionDelete();
  const runQuery = useDbQuery();
  const runTableRows = useDbTableRows();

  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [selectedTable, setSelectedTable] = useState<string | null>(null);
  const [sql, setSql] = useState("");
  const [allowWrite, setAllowWrite] = useState(false);
  const [addOpen, setAddOpen] = useState(false);
  const [result, setResult] = useState<QueryResult | null>(null);
  // The rejection itself, not its text: `ErrorPane` classifies it on the
  // `kind` the backend sends, and flattening it to a string here would throw
  // that away (`String(dto)` is "[object Object]").
  const [error, setError] = useState<unknown>(null);
  const [source, setSource] = useState("");

  if (!isDesktopShell()) {
    return <Notice title="The database manager needs the desktop shell" />;
  }

  const connection =
    connections?.find((c) => c.id === selectedId) ?? null;
  const pending = runQuery.isPending || runTableRows.isPending;

  function clearResults() {
    setResult(null);
    setError(null);
    setSource("");
  }

  function selectConnection(id: string) {
    setSelectedId(id === selectedId ? null : id);
    setSelectedTable(null);
    clearResults();
  }

  function runSql() {
    if (!connection || !sql.trim() || pending) return;
    const target = connection;
    setSelectedTable(null);
    runQuery.mutate(
      { id: target.id, sql: sql.trim(), allowWrite },
      {
        onSuccess: (data) => {
          setResult(data);
          setError(null);
          setSource(`Query · ${target.name}`);
        },
        onError: (failure) => {
          setResult(null);
          setError(failure);
          setSource(`Query · ${target.name}`);
        },
      },
    );
  }

  function openTable(table: string) {
    if (!connection || pending) return;
    const target = connection;
    setSelectedTable(table);
    runTableRows.mutate(
      { id: target.id, table, limit: TABLE_ROW_LIMIT },
      {
        onSuccess: (data) => {
          setResult(data);
          setError(null);
          setSource(`${table} · ${target.name}`);
        },
        onError: (failure) => {
          setResult(null);
          setError(failure);
          setSource(`${table} · ${target.name}`);
        },
      },
    );
  }

  return (
    <div className="grid h-full grid-cols-[300px_1fr]">
      <aside className="min-h-0 overflow-y-auto border-r p-2">
        <div className="flex items-center justify-between px-2 pb-1">
          <p className="text-[10px] font-medium uppercase tracking-wider text-muted-foreground">
            Connections
          </p>
          <Button
            variant="ghost"
            size="icon"
            className="size-5"
            aria-label="Add connection"
            title="Add connection"
            onClick={() => setAddOpen(true)}
          >
            <Plus className="size-3.5" />
          </Button>
        </div>

        {connections?.length === 0 && (
          <p className="px-2 text-xs text-muted-foreground">
            No databases yet — add a SQLite file to get started.
          </p>
        )}

        <ul className="space-y-0.5">
          {connections?.map((c) => (
            <li key={c.id}>
              <div
                className={cn(
                  "group flex items-center gap-1.5 rounded-md px-2 py-1",
                  c.id === selectedId ? "bg-accent" : "hover:bg-accent/60",
                )}
              >
                <button
                  type="button"
                  onClick={() => selectConnection(c.id)}
                  title={c.path}
                  className="flex min-w-0 flex-1 items-center gap-1.5 text-left"
                >
                  <Database className="size-3.5 shrink-0 text-muted-foreground" />
                  <span className="truncate text-xs">{c.name}</span>
                  <span className="truncate font-mono text-[10px] text-muted-foreground">
                    {basename(c.path)}
                  </span>
                </button>
                <Button
                  variant="ghost"
                  size="icon"
                  className="size-5 opacity-0 group-hover:opacity-70 hover:!opacity-100"
                  aria-label={`Delete ${c.name}`}
                  onClick={() =>
                    remove.mutate(c.id, {
                      onSuccess: () => {
                        if (c.id === selectedId) {
                          setSelectedId(null);
                          setSelectedTable(null);
                          clearResults();
                        }
                        toast.success(`Removed "${c.name}"`);
                      },
                      onError: (failure) => toast.error(dbErrorMessage(failure)),
                    })
                  }
                >
                  <Trash2 className="size-3" />
                </Button>
              </div>

              {c.id === selectedId && (
                <SchemaTree
                  connectionId={c.id}
                  selectedTable={selectedTable}
                  onOpenTable={openTable}
                />
              )}
            </li>
          ))}
        </ul>
      </aside>

      <section className="flex min-h-0 flex-col">
        <div className="flex shrink-0 items-center gap-2 border-b px-3 py-2">
          <span className="flex min-w-0 items-center gap-1.5 text-xs text-muted-foreground">
            <Database className="size-3.5 shrink-0" />
            {connection ? (
              <>
                Running against
                <span className="truncate font-medium text-foreground">
                  {connection.name}
                </span>
              </>
            ) : (
              "No connection selected"
            )}
          </span>
          <div className="flex-1" />
          <button
            type="button"
            onClick={() => setAllowWrite(!allowWrite)}
            aria-label={WRITE_TOGGLE_LABEL}
            aria-pressed={allowWrite}
            className={cn(
              "inline-flex items-center gap-1.5 rounded-full border px-2.5 py-0.5 text-[11px] transition-colors",
              allowWrite
                ? "border-yellow-500/50 bg-yellow-500/10 text-foreground"
                : "border-border text-muted-foreground hover:text-foreground",
            )}
            title={
              allowWrite
                ? "Writes enabled — INSERT/UPDATE/DELETE/DDL will change this database file. Click to go back to read-only."
                : "Read-only — the backend refuses any statement that isn't a read. Click to allow writes."
            }
          >
            {allowWrite ? (
              <ShieldAlert className="size-3" />
            ) : (
              <ShieldCheck className="size-3" />
            )}
            {allowWrite ? "Writes enabled" : "Read-only"}
          </button>
          <Button
            size="sm"
            disabled={!connection || !sql.trim() || pending}
            onClick={runSql}
          >
            <Play className="size-3.5" />
            {runQuery.isPending ? "Running…" : "Run"}
          </Button>
        </div>

        {allowWrite && (
          <p className="flex shrink-0 items-center gap-1.5 border-b border-yellow-500/30 bg-yellow-500/10 px-3 py-1 text-[11px] text-yellow-200">
            <TriangleAlert className="size-3 shrink-0" />
            Writes are enabled — a statement run now can modify or delete data
            {connection ? ` in ${connection.name}` : ""}. There is no undo.
          </p>
        )}

        <div className="shrink-0 border-b p-3">
          <textarea
            value={sql}
            onChange={(e) => setSql(e.target.value)}
            onKeyDown={(e) => {
              if ((e.ctrlKey || e.metaKey) && e.key === "Enter") {
                e.preventDefault();
                runSql();
              }
            }}
            spellCheck={false}
            placeholder="SELECT * FROM sqlite_master;"
            rows={6}
            className={cn(
              "w-full resize-none rounded-md border bg-transparent px-2.5 py-1.5 font-mono text-xs outline-none placeholder:text-muted-foreground focus-visible:border-ring",
              allowWrite && "border-yellow-500/40 focus-visible:border-yellow-500",
            )}
          />
          <p className="pt-1.5 text-[10px] text-muted-foreground">
            Ctrl+Enter runs the query.
          </p>
        </div>

        <div className="min-h-0 flex-1 overflow-hidden">
          {pending ? (
            <div className="flex h-full items-center justify-center text-sm text-muted-foreground">
              Running…
            </div>
          ) : error ? (
            <ErrorPane error={error} onEnableWrites={() => setAllowWrite(true)} />
          ) : result ? (
            <ResultPane result={result} source={source} />
          ) : (
            <div className="flex h-full flex-col items-center justify-center gap-2 text-muted-foreground">
              <Table2 className="size-6" />
              <p className="text-sm">
                {connection
                  ? "Run a query, or click a table to browse its rows."
                  : "Select a connection to start querying."}
              </p>
            </div>
          )}
        </div>
      </section>

      <AddConnectionDialog
        open={addOpen}
        pending={connect.isPending}
        onOpenChange={setAddOpen}
        onAdd={(name, path) =>
          connect.mutate(
            { name, path },
            {
              onSuccess: (created: DbConnection) => {
                setAddOpen(false);
                setSelectedId(created.id);
                setSelectedTable(null);
                clearResults();
                toast.success(`Connected to "${created.name}"`);
              },
              onError: (failure) => toast.error(dbErrorMessage(failure)),
            },
          )
        }
      />
    </div>
  );
}

function SchemaTree({
  connectionId,
  selectedTable,
  onOpenTable,
}: {
  connectionId: string;
  selectedTable: string | null;
  onOpenTable: (table: string) => void;
}) {
  const { data: schema, isLoading, error } = useDbSchema(connectionId);
  const [expanded, setExpanded] = useState<Set<string>>(new Set());

  if (isLoading) {
    return (
      <div className="space-y-1 py-1 pl-4 pr-2">
        <Skeleton className="h-4 w-full" />
        <Skeleton className="h-4 w-2/3" />
      </div>
    );
  }
  if (error) {
    return (
      <p className="py-1 pl-4 pr-2 text-[11px] text-destructive">
        {dbErrorMessage(error)}
      </p>
    );
  }
  if (!schema) return null;

  return (
    <div className="pb-1">
      <p className="px-2 py-1 pl-4 text-[10px] text-muted-foreground">
        {schema.tables.length} tables · {formatBytes(schema.sizeBytes)}
      </p>
      <ul>
        {schema.tables.map((table) => {
          const isOpen = expanded.has(table.name);
          return (
            <li key={table.name}>
              <div
                className={cn(
                  "flex items-center gap-0.5 rounded pl-2 pr-1.5",
                  selectedTable === table.name
                    ? "bg-accent/70"
                    : "hover:bg-accent/40",
                )}
              >
                <button
                  type="button"
                  aria-label={`${isOpen ? "Hide" : "Show"} columns of ${table.name}`}
                  onClick={() =>
                    setExpanded((prev) => {
                      const next = new Set(prev);
                      if (next.has(table.name)) next.delete(table.name);
                      else next.add(table.name);
                      return next;
                    })
                  }
                  className="shrink-0 rounded p-0.5 text-muted-foreground hover:text-foreground"
                >
                  {isOpen ? (
                    <ChevronDown className="size-3" />
                  ) : (
                    <ChevronRight className="size-3" />
                  )}
                </button>
                <button
                  type="button"
                  onClick={() => onOpenTable(table.name)}
                  title={`${table.name} — ${table.rowCount.toLocaleString()} rows`}
                  className="flex min-w-0 flex-1 items-center gap-1.5 py-0.5 text-left"
                >
                  {table.kind === "view" ? (
                    <Eye className="size-3 shrink-0 text-muted-foreground" />
                  ) : (
                    <Table2 className="size-3 shrink-0 text-muted-foreground" />
                  )}
                  <span className="truncate text-xs">{table.name}</span>
                  <Badge
                    variant="outline"
                    className="h-4 shrink-0 px-1 text-[9px] text-muted-foreground"
                  >
                    {table.kind}
                  </Badge>
                  <span className="shrink-0 font-mono text-[10px] text-muted-foreground">
                    {table.rowCount.toLocaleString()}
                  </span>
                </button>
              </div>

              {isOpen && (
                <ul className="pb-1 pl-7 pr-1.5">
                  {table.columns.map((column) => (
                    <li
                      key={column.name}
                      title={
                        column.defaultValue
                          ? `default: ${column.defaultValue}`
                          : undefined
                      }
                      className="flex items-center gap-1.5 py-px"
                    >
                      <span className="truncate font-mono text-[11px] text-foreground/80">
                        {column.name}
                      </span>
                      <span className="shrink-0 font-mono text-[10px] text-muted-foreground">
                        {column.dataType || "?"}
                      </span>
                      <div className="ml-auto flex shrink-0 items-center gap-1">
                        {column.primaryKey && (
                          <span
                            title="Primary key"
                            className="rounded bg-yellow-500/15 px-1 text-[9px] font-medium text-yellow-300"
                          >
                            PK
                          </span>
                        )}
                        {column.notNull && (
                          <span
                            title="NOT NULL — this column always requires a value"
                            className="rounded bg-muted px-1 text-[9px] text-muted-foreground"
                          >
                            NOT NULL
                          </span>
                        )}
                      </div>
                    </li>
                  ))}
                </ul>
              )}
            </li>
          );
        })}
      </ul>
    </div>
  );
}

function ResultPane({
  result,
  source,
}: {
  result: QueryResult;
  source: string;
}) {
  const hasGrid = result.columns.length > 0;

  return (
    <div className="flex h-full flex-col">
      <div className="flex shrink-0 flex-wrap items-center gap-2 border-b px-3 py-2">
        {!result.readOnly && (
          <Badge
            variant="outline"
            className="h-5 border-yellow-500/50 bg-yellow-500/10 px-1.5 text-[10px] text-yellow-200"
          >
            write
          </Badge>
        )}
        <span className="text-xs text-muted-foreground">
          {result.readOnly
            ? `${result.rowCount.toLocaleString()} ${result.rowCount === 1 ? "row" : "rows"}`
            : `${result.rowsAffected.toLocaleString()} ${result.rowsAffected === 1 ? "row" : "rows"} affected`}
          {" · "}
          {result.durationMs} ms
        </span>
        {result.truncated && (
          <span className="text-xs text-yellow-300/90">
            showing first {result.rows.length.toLocaleString()} rows — add a
            LIMIT or a WHERE clause to see the rest
          </span>
        )}
        <span className="ml-auto truncate font-mono text-[10px] text-muted-foreground">
          {source}
        </span>
      </div>

      {hasGrid ? (
        result.rows.length === 0 ? (
          <div className="flex min-h-0 flex-1 items-center justify-center text-sm text-muted-foreground">
            No rows returned.
          </div>
        ) : (
          <div className="min-h-0 flex-1 overflow-auto">
            <table className="w-max min-w-full border-collapse font-mono text-xs">
              <thead className="sticky top-0 z-10 bg-card">
                <tr>
                  {result.columns.map((column) => (
                    <th
                      key={column}
                      className="whitespace-nowrap border-b px-3 py-1.5 text-left font-medium"
                    >
                      {column}
                    </th>
                  ))}
                </tr>
              </thead>
              <tbody>
                {result.rows.map((row, i) => (
                  <tr key={i} className="hover:bg-accent/40">
                    {row.map((cell, j) => (
                      <td
                        key={j}
                        title={cell ?? "NULL"}
                        className="max-w-xs truncate border-b border-border/40 px-3 py-1"
                      >
                        {cell === null ? (
                          <span className="italic text-muted-foreground/60">
                            NULL
                          </span>
                        ) : (
                          cell
                        )}
                      </td>
                    ))}
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )
      ) : (
        <div className="flex min-h-0 flex-1 flex-col items-center justify-center gap-1 text-center">
          <p className="text-sm font-medium">
            {result.rowsAffected.toLocaleString()}{" "}
            {result.rowsAffected === 1 ? "row" : "rows"} affected
          </p>
          <p className="text-xs text-muted-foreground">
            The statement completed in {result.durationMs} ms and returned no
            rows.
          </p>
        </div>
      )}
    </div>
  );
}

function ErrorPane({
  error,
  onEnableWrites,
}: {
  error: unknown;
  onEnableWrites: () => void;
}) {
  const blocked = isWriteBlocked(error);
  const message = dbErrorMessage(error);

  return (
    <div className="h-full overflow-auto p-4">
      <Card
        className={cn(
          "py-4",
          blocked ? "border-yellow-500/40 bg-yellow-500/5" : "border-destructive/40",
        )}
      >
        <CardContent className="space-y-2">
          {blocked ? (
            <>
              <p className="flex items-center gap-2 text-sm font-medium">
                <ShieldAlert className="size-4 shrink-0 text-yellow-400" />
                Blocked — this statement writes to the database
              </p>
              <p className="text-xs text-muted-foreground">
                The connection is read-only by default, so INSERT, UPDATE,
                DELETE and schema changes are refused before they reach the
                file. Flip the <span className="text-foreground">Read-only</span>{" "}
                toggle above to <span className="text-foreground">Writes
                enabled</span> and run it again if you meant to change data.
              </p>
              <Button
                variant="outline"
                size="sm"
                className="border-yellow-500/50 text-yellow-200 hover:bg-yellow-500/10"
                onClick={onEnableWrites}
              >
                <ShieldAlert className="size-3.5" />
                Enable writes
              </Button>
            </>
          ) : (
            <p className="flex items-center gap-2 text-sm font-medium text-destructive">
              <TriangleAlert className="size-4 shrink-0" />
              Query failed
            </p>
          )}
          <pre className="whitespace-pre-wrap break-words rounded-md bg-muted/40 p-2.5 font-mono text-[11px] text-muted-foreground">
            {message}
          </pre>
        </CardContent>
      </Card>
    </div>
  );
}

function AddConnectionDialog({
  open,
  pending,
  onOpenChange,
  onAdd,
}: {
  open: boolean;
  pending: boolean;
  onOpenChange: (open: boolean) => void;
  onAdd: (name: string, path: string) => void;
}) {
  const [name, setName] = useState("");
  const [path, setPath] = useState("");

  function submit() {
    if (!name.trim() || !path.trim()) return;
    onAdd(name.trim(), path.trim());
    setName("");
    setPath("");
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>Add connection</DialogTitle>
        </DialogHeader>
        <div className="space-y-3">
          <div className="space-y-1.5">
            <Label htmlFor="db-name">Name</Label>
            <Input
              id="db-name"
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder="e.g. App database"
              autoFocus
            />
          </div>
          <div className="space-y-1.5">
            <Label htmlFor="db-path">File path</Label>
            <Input
              id="db-path"
              value={path}
              onChange={(e) => setPath(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") submit();
              }}
              placeholder="C:\\projects\\app\\data.db"
              className="font-mono text-xs"
            />
            <p className="text-[11px] text-muted-foreground">
              Full path to a SQLite file (.db, .sqlite, .sqlite3).
            </p>
          </div>
        </div>
        <DialogFooter>
          <Button variant="ghost" onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <Button
            disabled={!name.trim() || !path.trim() || pending}
            onClick={submit}
          >
            {pending ? "Connecting…" : "Connect"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function Notice({ title }: { title: string }) {
  return (
    <div className="mx-auto max-w-3xl p-6">
      <Card>
        <CardContent className="flex flex-col items-center gap-2 py-12 text-center">
          <Database className="size-6 text-muted-foreground" />
          <p className="font-medium">{title}</p>
        </CardContent>
      </Card>
    </div>
  );
}

function basename(path: string): string {
  return path.split(/[\\/]/).pop() ?? path;
}
