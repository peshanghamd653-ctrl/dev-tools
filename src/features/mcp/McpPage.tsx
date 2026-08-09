import { useState } from "react";
import { Plug, Plus, Sparkles, Trash2 } from "lucide-react";
import { toast } from "sonner";

import {
  isDesktopShell,
  type McpServer,
  type McpTool,
} from "@/shared/ipc/client";
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
import {
  useMcpDiscoverTools,
  useMcpServerCreate,
  useMcpServerDelete,
  useMcpServers,
} from "./hooks";

export function McpPage() {
  const { data: servers, isLoading } = useMcpServers();
  const remove = useMcpServerDelete();
  const create = useMcpServerCreate();
  const [addOpen, setAddOpen] = useState(false);

  if (!isDesktopShell()) {
    return <EmptyCard title="MCP servers need the desktop shell" />;
  }

  return (
    <div className="mx-auto max-w-3xl space-y-4 p-6">
      <div className="flex items-start justify-between gap-4">
        <div>
          <h1 className="text-xl font-semibold tracking-tight">MCP Servers</h1>
          <p className="text-sm text-muted-foreground">
            Configure external tool servers and see what they offer.
          </p>
        </div>
        <Button size="sm" onClick={() => setAddOpen(true)}>
          <Plus className="size-4" />
          Add server
        </Button>
      </div>

      <p className="flex items-start gap-1.5 rounded-md border border-yellow-500/30 bg-yellow-500/5 px-3 py-2 text-xs text-yellow-200">
        <Sparkles className="mt-px size-3.5 shrink-0" />
        Discovery only, for now: connecting shows what a server offers, but its
        tools are not yet callable from the AI assistant. Handing an external
        process's tools to the agent loop needs its own approval and trust
        design first.
      </p>

      {isLoading && <Skeleton className="h-20 w-full rounded-xl" />}

      {servers && servers.length === 0 && (
        <EmptyCard title="No servers configured">
          Add an MCP server&apos;s launch command — e.g.{" "}
          <code className="rounded bg-muted px-1 py-0.5 font-mono text-[11px]">
            npx -y @modelcontextprotocol/server-filesystem /path
          </code>{" "}
          — to see what tools it offers.
        </EmptyCard>
      )}

      {servers && servers.length > 0 && (
        <ul className="space-y-2">
          {servers.map((server) => (
            <li key={server.id}>
              <ServerRow
                server={server}
                onDelete={() =>
                  remove.mutate(server.id, {
                    onSuccess: () => toast.success(`Removed "${server.name}"`),
                    onError: (error) => toast.error(String(error)),
                  })
                }
              />
            </li>
          ))}
        </ul>
      )}

      <AddServerDialog
        open={addOpen}
        pending={create.isPending}
        onOpenChange={setAddOpen}
        onAdd={(name, command, args) =>
          create.mutate(
            { name, command, args },
            {
              onSuccess: (server) => {
                setAddOpen(false);
                toast.success(`Added "${server.name}"`);
              },
              onError: (error) => toast.error(String(error)),
            },
          )
        }
      />
    </div>
  );
}

function ServerRow({
  server,
  onDelete,
}: {
  server: McpServer;
  onDelete: () => void;
}) {
  const discover = useMcpDiscoverTools();
  const [result, setResult] = useState<{
    serverName: string;
    tools: McpTool[];
  } | null>(null);

  return (
    <Card className="group gap-0 py-3">
      <CardContent className="space-y-2 px-4">
        <div className="flex items-center gap-3">
          <Plug className="size-4 shrink-0 text-muted-foreground" />
          <div className="min-w-0 flex-1">
            <p className="truncate text-sm font-medium">{server.name}</p>
            <p className="truncate font-mono text-[11px] text-muted-foreground">
              {[server.command, ...server.args].join(" ")}
            </p>
          </div>
          <Button
            variant="outline"
            size="sm"
            className="h-7 shrink-0 gap-1 text-xs"
            disabled={discover.isPending}
            onClick={() => {
              setResult(null);
              discover.mutate(
                { command: server.command, args: server.args },
                {
                  onSuccess: ([serverName, tools]) =>
                    setResult({ serverName, tools }),
                  onError: (error) => toast.error(String(error)),
                },
              );
            }}
          >
            {discover.isPending ? "Connecting…" : "Discover tools"}
          </Button>
          <Button
            variant="ghost"
            size="icon"
            className="size-7 shrink-0 opacity-0 transition-opacity group-hover:opacity-70 hover:!opacity-100"
            aria-label={`Delete ${server.name}`}
            onClick={onDelete}
          >
            <Trash2 className="size-3.5" />
          </Button>
        </div>

        {result && (
          <div className="space-y-1 rounded-md border bg-card px-2.5 py-1.5">
            <p className="text-[11px] text-muted-foreground">
              Reports itself as{" "}
              <span className="font-medium text-foreground">
                {result.serverName}
              </span>
              {" · "}
              {result.tools.length}{" "}
              {result.tools.length === 1 ? "tool" : "tools"}
            </p>
            {result.tools.length === 0 ? (
              <p className="text-xs text-muted-foreground">
                No tools advertised.
              </p>
            ) : (
              <ul className="space-y-1">
                {result.tools.map((tool) => (
                  <li key={tool.name} className="text-xs">
                    <span className="font-mono font-medium">{tool.name}</span>
                    {tool.description && (
                      <span className="text-muted-foreground">
                        {" — "}
                        {tool.description}
                      </span>
                    )}
                  </li>
                ))}
              </ul>
            )}
          </div>
        )}
      </CardContent>
    </Card>
  );
}

function AddServerDialog({
  open,
  pending,
  onOpenChange,
  onAdd,
}: {
  open: boolean;
  pending: boolean;
  onOpenChange: (open: boolean) => void;
  onAdd: (name: string, command: string, args: string[]) => void;
}) {
  const [name, setName] = useState("");
  const [command, setCommand] = useState("");
  const [argsText, setArgsText] = useState("");

  const canSubmit = Boolean(name.trim()) && Boolean(command.trim()) && !pending;

  function submit() {
    if (!canSubmit) return;
    // A naive whitespace split — good enough for the common case (npx/uvx
    // with a package name and a path), and not a shim for a real shell
    // parser. An argument that needs its own embedded space isn't
    // representable here; that's a real limitation, not an oversight.
    const args = argsText.split(/\s+/).filter(Boolean);
    onAdd(name.trim(), command.trim(), args);
    setName("");
    setCommand("");
    setArgsText("");
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>Add MCP server</DialogTitle>
        </DialogHeader>
        <div className="space-y-3">
          <div className="space-y-1.5">
            <Label htmlFor="mcp-name">Name</Label>
            <Input
              id="mcp-name"
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder="e.g. Filesystem"
              autoFocus
            />
          </div>
          <div className="space-y-1.5">
            <Label htmlFor="mcp-command">Command</Label>
            <Input
              id="mcp-command"
              value={command}
              onChange={(e) => setCommand(e.target.value)}
              placeholder="npx"
              className="font-mono text-xs"
            />
          </div>
          <div className="space-y-1.5">
            <Label htmlFor="mcp-args">Arguments</Label>
            <Input
              id="mcp-args"
              value={argsText}
              onChange={(e) => setArgsText(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") submit();
              }}
              placeholder="-y @modelcontextprotocol/server-filesystem /path"
              className="font-mono text-xs"
            />
            <p className="text-[11px] text-muted-foreground">
              Space-separated. No support yet for an argument that itself
              contains a space.
            </p>
          </div>
        </div>
        <DialogFooter>
          <Button variant="ghost" onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <Button disabled={!canSubmit} onClick={submit}>
            {pending ? "Adding…" : "Add server"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function EmptyCard({
  title,
  children,
}: {
  title: string;
  children?: React.ReactNode;
}) {
  return (
    <div className="mx-auto max-w-2xl py-6">
      <Card>
        <CardContent className="flex flex-col items-center gap-2 py-12 text-center">
          <Plug className="size-6 text-muted-foreground" />
          <p className="font-medium">{title}</p>
          {children && (
            <p className="max-w-md text-sm text-muted-foreground">{children}</p>
          )}
        </CardContent>
      </Card>
    </div>
  );
}
