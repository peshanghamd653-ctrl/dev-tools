import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  ChevronsUpDown,
  Check,
  Eye,
  EyeOff,
  History,
  Layers,
  Plus,
  Save,
  Send,
  Trash2,
  X,
} from "lucide-react";
import { toast } from "sonner";

import {
  ipc,
  isDesktopShell,
  type ApiEnvironment,
  type ApiEnvVar,
  type ApiHeader,
  type ApiRequestSpec,
  type ApiResponse,
  type SavedRequest,
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
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/shared/ui/dropdown-menu";
import { Input } from "@/shared/ui/input";
import { Label } from "@/shared/ui/label";

const METHODS = ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"];

interface HeaderRow {
  name: string;
  value: string;
}

export function ApiClientPage() {
  const queryClient = useQueryClient();
  const [method, setMethod] = useState("GET");
  const [url, setUrl] = useState("");
  const [headers, setHeaders] = useState<HeaderRow[]>([]);
  const [body, setBody] = useState("");
  const [tab, setTab] = useState<"headers" | "body">("headers");
  const [responseTab, setResponseTab] = useState<"body" | "headers">("body");
  const [response, setResponse] = useState<ApiResponse | null>(null);
  const [saveOpen, setSaveOpen] = useState(false);
  const [envDialogOpen, setEnvDialogOpen] = useState(false);

  const { data: saved } = useQuery({
    queryKey: ["api", "saved"],
    queryFn: ipc.apiRequests,
    enabled: isDesktopShell(),
  });
  const { data: history } = useQuery({
    queryKey: ["api", "history"],
    queryFn: ipc.apiHistory,
    enabled: isDesktopShell(),
  });
  const { data: environments } = useQuery({
    queryKey: ["api", "environments"],
    queryFn: ipc.apiEnvironments,
    enabled: isDesktopShell(),
  });
  const activeEnvironment = environments?.find((e) => e.active) ?? null;

  const setActiveEnvironment = useMutation({
    mutationFn: (id: string | null) => ipc.apiEnvironmentSetActive(id),
    onSuccess: () =>
      queryClient.invalidateQueries({ queryKey: ["api", "environments"] }),
    onError: (error) => toast.error(String(error)),
  });

  const spec = (): ApiRequestSpec => ({
    method,
    url: url.trim(),
    headers: headers.filter((h) => h.name.trim()) as ApiHeader[],
    body: body.trim() ? body : null,
  });

  const send = useMutation({
    mutationFn: () => ipc.apiSend(spec()),
    onSuccess: (result) => {
      setResponse(result);
      void queryClient.invalidateQueries({ queryKey: ["api", "history"] });
    },
    onError: (error) => toast.error(String(error)),
  });

  function load(request: SavedRequest) {
    setMethod(request.method);
    setUrl(request.url);
    setHeaders(request.headers);
    setBody(request.body ?? "");
    setResponse(null);
  }

  if (!isDesktopShell()) {
    return (
      <div className="mx-auto max-w-3xl p-6">
        <Card>
          <CardContent className="flex flex-col items-center gap-2 py-12 text-center">
            <Send className="size-6 text-muted-foreground" />
            <p className="font-medium">
              The API client needs the desktop shell
            </p>
          </CardContent>
        </Card>
      </div>
    );
  }

  const collections = new Map<string, SavedRequest[]>();
  for (const request of saved ?? []) {
    const group = collections.get(request.collection) ?? [];
    group.push(request);
    collections.set(request.collection, group);
  }

  return (
    <div className="grid h-full grid-cols-[280px_1fr]">
      <aside className="min-h-0 overflow-y-auto border-r p-2">
        {[...collections.entries()].map(([collection, requests]) => (
          <section key={collection} className="pb-3">
            <p className="px-2 pb-1 text-[10px] font-medium uppercase tracking-wider text-muted-foreground">
              {collection}
            </p>
            <ul className="space-y-0.5">
              {requests.map((request) => (
                <li
                  key={request.id}
                  className="group flex items-center gap-1.5 rounded-md px-2 py-1 hover:bg-accent/60"
                >
                  <button
                    type="button"
                    onClick={() => load(request)}
                    className="flex min-w-0 flex-1 items-center gap-1.5 text-left"
                  >
                    <MethodBadge method={request.method} />
                    <span className="truncate text-xs">{request.name}</span>
                  </button>
                  <Button
                    variant="ghost"
                    size="icon"
                    className="size-5 opacity-0 group-hover:opacity-70 hover:!opacity-100"
                    aria-label={`Delete ${request.name}`}
                    onClick={() =>
                      void ipc
                        .apiRequestDelete(request.id)
                        .then(() =>
                          queryClient.invalidateQueries({
                            queryKey: ["api", "saved"],
                          }),
                        )
                        .catch((error) => toast.error(String(error)))
                    }
                  >
                    <Trash2 className="size-3" />
                  </Button>
                </li>
              ))}
            </ul>
          </section>
        ))}

        <section>
          <p className="flex items-center gap-1 px-2 pb-1 text-[10px] font-medium uppercase tracking-wider text-muted-foreground">
            <History className="size-3" />
            History
          </p>
          {history?.length === 0 && (
            <p className="px-2 text-xs text-muted-foreground">
              Sent requests appear here.
            </p>
          )}
          <ul className="space-y-0.5">
            {history?.slice(0, 20).map((entry) => (
              <li key={entry.id}>
                <button
                  type="button"
                  onClick={() => {
                    setMethod(entry.method);
                    setUrl(entry.url);
                    setResponse(null);
                  }}
                  className="flex w-full items-center gap-1.5 rounded-md px-2 py-1 text-left hover:bg-accent/60"
                >
                  <MethodBadge method={entry.method} />
                  <span className="min-w-0 flex-1 truncate font-mono text-[11px]">
                    {entry.url}
                  </span>
                  <span
                    className={cn(
                      "text-[10px]",
                      entry.status < 400 ? "text-emerald-400" : "text-red-400",
                    )}
                  >
                    {entry.status}
                  </span>
                </button>
              </li>
            ))}
          </ul>
        </section>
      </aside>

      <section className="flex min-h-0 flex-col">
        <div className="flex shrink-0 items-center gap-2 border-b p-3">
          <select
            value={method}
            onChange={(e) => setMethod(e.target.value)}
            className="h-9 rounded-md border bg-transparent px-2 font-mono text-xs outline-none focus-visible:border-ring"
          >
            {METHODS.map((m) => (
              <option key={m} value={m} className="bg-popover">
                {m}
              </option>
            ))}
          </select>
          <Input
            value={url}
            onChange={(e) => setUrl(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && url.trim()) send.mutate();
            }}
            placeholder="https://api.example.com/v1/users"
            className="flex-1 font-mono text-xs"
          />
          <Button
            disabled={!url.trim() || send.isPending}
            onClick={() => send.mutate()}
          >
            <Send className="size-4" />
            {send.isPending ? "Sending…" : "Send"}
          </Button>
          <Button
            variant="outline"
            size="icon"
            aria-label="Save request"
            disabled={!url.trim()}
            onClick={() => setSaveOpen(true)}
          >
            <Save className="size-4" />
          </Button>
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button
                variant="outline"
                size="sm"
                aria-label="Environment"
                className="gap-1 px-2 text-xs"
              >
                <Layers className="size-3.5 text-muted-foreground" />
                {activeEnvironment ? activeEnvironment.name : "No environment"}
                <ChevronsUpDown className="size-3 text-muted-foreground" />
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end" className="w-56">
              <DropdownMenuLabel>Environment</DropdownMenuLabel>
              <DropdownMenuItem
                onClick={() => setActiveEnvironment.mutate(null)}
              >
                <Check
                  className={cn(
                    "size-3.5",
                    activeEnvironment ? "opacity-0" : "opacity-100",
                  )}
                />
                No environment
              </DropdownMenuItem>
              {environments?.map((env) => (
                <DropdownMenuItem
                  key={env.id}
                  onClick={() => setActiveEnvironment.mutate(env.id)}
                >
                  <Check
                    className={cn(
                      "size-3.5",
                      activeEnvironment?.id === env.id
                        ? "opacity-100"
                        : "opacity-0",
                    )}
                  />
                  {env.name}
                </DropdownMenuItem>
              ))}
              <DropdownMenuSeparator />
              <DropdownMenuItem onClick={() => setEnvDialogOpen(true)}>
                Manage environments…
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        </div>

        <div className="shrink-0 border-b">
          <div className="flex gap-1 px-3 pt-2">
            <EditorTab
              active={tab === "headers"}
              label={`Headers${headers.filter((h) => h.name.trim()).length ? ` (${headers.filter((h) => h.name.trim()).length})` : ""}`}
              onClick={() => setTab("headers")}
            />
            <EditorTab
              active={tab === "body"}
              label="Body"
              onClick={() => setTab("body")}
            />
          </div>
          <div className="max-h-48 overflow-y-auto p-3">
            {tab === "headers" ? (
              <div className="space-y-1.5">
                {headers.map((header, i) => (
                  <div key={i} className="flex gap-1.5">
                    <Input
                      value={header.name}
                      onChange={(e) =>
                        setHeaders((rows) =>
                          rows.map((r, j) =>
                            j === i ? { ...r, name: e.target.value } : r,
                          ),
                        )
                      }
                      placeholder="Header"
                      className="h-7 flex-1 font-mono text-xs"
                    />
                    <Input
                      value={header.value}
                      onChange={(e) =>
                        setHeaders((rows) =>
                          rows.map((r, j) =>
                            j === i ? { ...r, value: e.target.value } : r,
                          ),
                        )
                      }
                      placeholder="Value"
                      className="h-7 flex-[2] font-mono text-xs"
                    />
                    <Button
                      variant="ghost"
                      size="icon"
                      className="size-7"
                      aria-label="Remove header"
                      onClick={() =>
                        setHeaders((rows) => rows.filter((_, j) => j !== i))
                      }
                    >
                      <X className="size-3.5" />
                    </Button>
                  </div>
                ))}
                <Button
                  variant="ghost"
                  size="sm"
                  className="h-7 gap-1 text-xs text-muted-foreground"
                  onClick={() =>
                    setHeaders((rows) => [...rows, { name: "", value: "" }])
                  }
                >
                  <Plus className="size-3.5" />
                  Add header
                </Button>
              </div>
            ) : (
              <textarea
                value={body}
                onChange={(e) => setBody(e.target.value)}
                placeholder='{"raw": "request body"}'
                rows={5}
                className="w-full resize-none rounded-md border bg-transparent px-2.5 py-1.5 font-mono text-xs outline-none placeholder:text-muted-foreground focus-visible:border-ring"
              />
            )}
          </div>
        </div>

        <div className="min-h-0 flex-1 overflow-hidden">
          {response ? (
            <div className="flex h-full flex-col">
              <div className="flex shrink-0 items-center gap-2 border-b px-3 py-2">
                <Badge
                  className={cn(
                    "font-mono",
                    response.status < 400
                      ? "bg-emerald-500/15 text-emerald-300"
                      : "bg-red-500/15 text-red-300",
                  )}
                  variant="outline"
                >
                  {response.status}
                </Badge>
                <span className="text-xs text-muted-foreground">
                  {response.durationMs} ms · {formatBytes(response.sizeBytes)}
                  {response.truncated && " · truncated"}
                </span>
                <div className="flex-1" />
                <EditorTab
                  active={responseTab === "body"}
                  label="Body"
                  onClick={() => setResponseTab("body")}
                />
                <EditorTab
                  active={responseTab === "headers"}
                  label={`Headers (${response.headers.length})`}
                  onClick={() => setResponseTab("headers")}
                />
              </div>
              <div className="min-h-0 flex-1 overflow-auto p-3">
                {responseTab === "body" ? (
                  <pre className="whitespace-pre-wrap break-all font-mono text-xs">
                    {prettyBody(response.body)}
                  </pre>
                ) : (
                  <table className="w-full font-mono text-xs">
                    <tbody>
                      {response.headers.map((header, i) => (
                        <tr key={i} className="align-top">
                          <td className="whitespace-nowrap pr-4 text-muted-foreground">
                            {header.name}
                          </td>
                          <td className="break-all">{header.value}</td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                )}
              </div>
            </div>
          ) : (
            <div className="flex h-full items-center justify-center text-sm text-muted-foreground">
              {send.isPending
                ? "Sending…"
                : "Send a request to see the response."}
            </div>
          )}
        </div>
      </section>

      <SaveDialog
        open={saveOpen}
        onOpenChange={setSaveOpen}
        onSave={(name, collection) => {
          void ipc
            .apiSave(name, collection, spec())
            .then(() => {
              toast.success(`Saved "${name}"`);
              setSaveOpen(false);
              return queryClient.invalidateQueries({
                queryKey: ["api", "saved"],
              });
            })
            .catch((error) => toast.error(String(error)));
        }}
      />

      <EnvironmentsDialog
        open={envDialogOpen}
        onOpenChange={setEnvDialogOpen}
        environments={environments ?? []}
      />
    </div>
  );
}

function SaveDialog({
  open,
  onOpenChange,
  onSave,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onSave: (name: string, collection: string) => void;
}) {
  const [name, setName] = useState("");
  const [collection, setCollection] = useState("");
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-sm">
        <DialogHeader>
          <DialogTitle>Save request</DialogTitle>
        </DialogHeader>
        <div className="space-y-3">
          <div className="space-y-1.5">
            <Label htmlFor="req-name">Name</Label>
            <Input
              id="req-name"
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder="e.g. List users"
              autoFocus
            />
          </div>
          <div className="space-y-1.5">
            <Label htmlFor="req-collection">Collection</Label>
            <Input
              id="req-collection"
              value={collection}
              onChange={(e) => setCollection(e.target.value)}
              placeholder="Default"
            />
          </div>
        </div>
        <DialogFooter>
          <Button variant="ghost" onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <Button
            disabled={!name.trim()}
            onClick={() => onSave(name.trim(), collection.trim())}
          >
            Save
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function EnvironmentsDialog({
  open,
  onOpenChange,
  environments,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  environments: ApiEnvironment[];
}) {
  const queryClient = useQueryClient();
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [name, setName] = useState("");
  const [vars, setVars] = useState<ApiEnvVar[]>([]);
  const [revealed, setRevealed] = useState<Set<number>>(new Set());

  const selected = environments.find((e) => e.id === selectedId) ?? null;

  function select(env: ApiEnvironment) {
    setSelectedId(env.id);
    setName(env.name);
    setVars(env.vars);
    setRevealed(new Set());
  }

  function startNew() {
    setSelectedId(null);
    setName("");
    setVars([]);
    setRevealed(new Set());
  }

  const invalidate = () =>
    queryClient.invalidateQueries({ queryKey: ["api", "environments"] });

  const create = useMutation({
    mutationFn: (newName: string) => ipc.apiEnvironmentCreate(newName),
    onSuccess: (env) => {
      void invalidate();
      select(env);
    },
    onError: (error) => toast.error(String(error)),
  });

  const update = useMutation({
    mutationFn: () => {
      if (!selected) throw new Error("No environment selected");
      return ipc.apiEnvironmentUpdate(selected.id, name.trim(), vars);
    },
    onSuccess: (env) => {
      void invalidate();
      select(env);
      toast.success(`Saved "${env.name}"`);
    },
    onError: (error) => toast.error(String(error)),
  });

  const remove = useMutation({
    mutationFn: (id: string) => ipc.apiEnvironmentDelete(id),
    onSuccess: () => {
      void invalidate();
      startNew();
    },
    onError: (error) => toast.error(String(error)),
  });

  const setActive = useMutation({
    mutationFn: (id: string) => ipc.apiEnvironmentSetActive(id),
    onSuccess: () => void invalidate(),
    onError: (error) => toast.error(String(error)),
  });

  function updateVar(i: number, patch: Partial<ApiEnvVar>) {
    setVars((rows) => rows.map((r, j) => (j === i ? { ...r, ...patch } : r)));
  }

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        onOpenChange(next);
        if (!next) startNew();
      }}
    >
      <DialogContent className="sm:max-w-2xl">
        <DialogHeader>
          <DialogTitle>Environments</DialogTitle>
        </DialogHeader>
        <div className="grid grid-cols-[160px_1fr] gap-4">
          <div className="space-y-1">
            <ul className="space-y-0.5">
              {environments.map((env) => (
                <li
                  key={env.id}
                  className="group flex items-center gap-1 rounded-md px-1.5 py-1 hover:bg-accent/60"
                >
                  <button
                    type="button"
                    onClick={() => select(env)}
                    className={cn(
                      "min-w-0 flex-1 truncate text-left text-xs",
                      selectedId === env.id && "font-medium text-foreground",
                    )}
                  >
                    {env.name}
                    {env.active && (
                      <span className="ml-1 text-[10px] text-emerald-400">
                        active
                      </span>
                    )}
                  </button>
                  <Button
                    variant="ghost"
                    size="icon"
                    className="size-5 opacity-0 group-hover:opacity-70 hover:!opacity-100"
                    aria-label={`Delete ${env.name}`}
                    onClick={() => remove.mutate(env.id)}
                  >
                    <Trash2 className="size-3" />
                  </Button>
                </li>
              ))}
            </ul>
            <Button
              variant="ghost"
              size="sm"
              className="h-7 gap-1 px-1.5 text-xs text-muted-foreground"
              onClick={startNew}
            >
              <Plus className="size-3.5" />
              New environment
            </Button>
          </div>

          <div className="min-w-0 space-y-3 border-l pl-4">
            <div className="space-y-1.5">
              <Label htmlFor="env-name">Name</Label>
              <Input
                id="env-name"
                value={name}
                onChange={(e) => setName(e.target.value)}
                placeholder="e.g. Production"
                autoFocus
              />
            </div>

            <div className="space-y-1.5">
              <Label>Variables</Label>
              {!selected ? (
                <p className="text-xs text-muted-foreground">
                  Create the environment first, then add variables.
                </p>
              ) : (
                <div className="space-y-1.5">
                  {vars.map((v, i) => (
                    <div key={i} className="flex items-center gap-1.5">
                      <Input
                        value={v.key}
                        onChange={(e) => updateVar(i, { key: e.target.value })}
                        placeholder="API_URL"
                        className="h-7 flex-1 font-mono text-xs"
                      />
                      <Input
                        value={v.value}
                        onChange={(e) =>
                          updateVar(i, { value: e.target.value })
                        }
                        placeholder="Value"
                        type={
                          v.secret && !revealed.has(i) ? "password" : "text"
                        }
                        className="h-7 flex-[2] font-mono text-xs"
                      />
                      <Button
                        variant="ghost"
                        size="icon"
                        className="size-7"
                        aria-label={
                          revealed.has(i) ? "Hide value" : "Reveal value"
                        }
                        onClick={() =>
                          setRevealed((set) => {
                            const next = new Set(set);
                            if (next.has(i)) next.delete(i);
                            else next.add(i);
                            return next;
                          })
                        }
                      >
                        {revealed.has(i) ? (
                          <EyeOff className="size-3.5" />
                        ) : (
                          <Eye className="size-3.5" />
                        )}
                      </Button>
                      <Button
                        variant={v.secret ? "secondary" : "ghost"}
                        size="icon"
                        className="size-7"
                        aria-label={
                          v.secret ? "Marked secret" : "Mark as secret"
                        }
                        title="Secret only masks the value in this editor — it is not encrypted"
                        onClick={() => updateVar(i, { secret: !v.secret })}
                      >
                        <span className="text-[10px] font-semibold">S</span>
                      </Button>
                      <Button
                        variant="ghost"
                        size="icon"
                        className="size-7"
                        aria-label="Remove variable"
                        onClick={() =>
                          setVars((rows) => rows.filter((_, j) => j !== i))
                        }
                      >
                        <X className="size-3.5" />
                      </Button>
                    </div>
                  ))}
                  <Button
                    variant="ghost"
                    size="sm"
                    className="h-7 gap-1 text-xs text-muted-foreground"
                    onClick={() =>
                      setVars((rows) => [
                        ...rows,
                        { key: "", value: "", secret: false },
                      ])
                    }
                  >
                    <Plus className="size-3.5" />
                    Add variable
                  </Button>
                  <p className="text-[11px] text-muted-foreground">
                    Marking a variable secret only masks it in this editor —
                    values are stored as plain text, same as headers.
                  </p>
                </div>
              )}
            </div>
          </div>
        </div>
        <DialogFooter>
          {selected && !selected.active && (
            <Button
              variant="outline"
              onClick={() => setActive.mutate(selected.id)}
            >
              Set as active
            </Button>
          )}
          <Button variant="ghost" onClick={() => onOpenChange(false)}>
            Close
          </Button>
          <Button
            disabled={!name.trim()}
            onClick={() =>
              selected ? update.mutate() : create.mutate(name.trim())
            }
          >
            {selected ? "Save" : "Create"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

const METHOD_COLORS: Record<string, string> = {
  GET: "text-emerald-400",
  POST: "text-sky-400",
  PUT: "text-yellow-400",
  PATCH: "text-yellow-400",
  DELETE: "text-red-400",
};

function MethodBadge({ method }: { method: string }) {
  return (
    <span
      className={cn(
        "w-11 shrink-0 font-mono text-[10px] font-semibold",
        METHOD_COLORS[method] ?? "text-muted-foreground",
      )}
    >
      {method}
    </span>
  );
}

function EditorTab({
  active,
  label,
  onClick,
}: {
  active: boolean;
  label: string;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={cn(
        "rounded px-2 py-0.5 text-xs",
        active
          ? "bg-accent text-foreground"
          : "text-muted-foreground hover:bg-accent/60",
      )}
    >
      {label}
    </button>
  );
}

function prettyBody(body: string): string {
  try {
    return JSON.stringify(JSON.parse(body), null, 2);
  } catch {
    return body;
  }
}
