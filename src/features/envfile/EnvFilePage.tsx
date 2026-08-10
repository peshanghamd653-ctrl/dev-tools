import { useEffect, useState } from "react";
import { Eye, EyeOff, FolderPlus, Plus, Trash2 } from "lucide-react";
import { toast } from "sonner";

import { useGitStore } from "@/features/git/store";
import { useProjects } from "@/features/projects/hooks";
import { useActiveWorkspace } from "@/features/workspaces/hooks";
import { isDesktopShell } from "@/shared/ipc/client";
import { cn } from "@/shared/lib/utils";
import { Button } from "@/shared/ui/button";
import { Card, CardContent } from "@/shared/ui/card";
import { Input } from "@/shared/ui/input";
import { Label } from "@/shared/ui/label";
import {
  useEnvFileDeleteKey,
  useEnvFileEntries,
  useEnvFileList,
  useEnvFileSet,
} from "./hooks";

const DEFAULT_FILE_NAME = ".env";

/**
 * Reads and edits a project's own `.env` files directly — plaintext values,
 * on purpose. `SecretsSection` (Settings) is the encrypted, write-only
 * vault; this is for the values a running project actually needs, which
 * have to be readable by something other than DevOS anyway.
 */
export function EnvFilePage() {
  const activeWorkspace = useActiveWorkspace();
  const { data: projects } = useProjects(activeWorkspace?.id);
  const selectedProjectId = useGitStore((s) => s.selectedProjectId);
  const project =
    projects?.find((p) => p.id === selectedProjectId) ?? projects?.[0] ?? null;

  const fileList = useEnvFileList(project?.path ?? null);
  const [selectedFile, setSelectedFile] = useState<string | null>(null);

  useEffect(() => {
    if (!fileList.data) return;
    if (selectedFile && fileList.data.includes(selectedFile)) return;
    setSelectedFile(fileList.data[0] ?? null);
  }, [fileList.data, selectedFile]);

  if (!isDesktopShell()) {
    return <EmptyCard title="Environment variables need the desktop shell" />;
  }

  if (!project) {
    return (
      <EmptyCard title="No project yet">
        Add a project to read and edit its .env files.
      </EmptyCard>
    );
  }

  const files = fileList.data ?? [];
  const activeFile = selectedFile ?? DEFAULT_FILE_NAME;

  return (
    <div className="mx-auto max-w-2xl space-y-4 p-6">
      <div>
        <h1 className="text-xl font-semibold tracking-tight">
          Environment Variables
        </h1>
        <p className="text-sm text-muted-foreground">
          Read and edit .env files for {project.name}. Values are plaintext on
          disk already — this just shows them.
        </p>
      </div>

      {files.length > 0 && (
        <div className="flex flex-wrap gap-1.5" aria-label="Env files">
          {files.map((file) => (
            <button
              key={file}
              type="button"
              onClick={() => setSelectedFile(file)}
              className={cn(
                "rounded-md border px-2.5 py-1 font-mono text-xs",
                file === activeFile
                  ? "border-ring bg-accent"
                  : "text-muted-foreground hover:bg-accent/60",
              )}
            >
              {file}
            </button>
          ))}
        </div>
      )}

      {files.length === 0 && !fileList.isLoading && (
        <p className="text-xs text-muted-foreground">
          No .env file found at the project root yet. Adding a variable below
          creates <span className="font-mono">{DEFAULT_FILE_NAME}</span>.
        </p>
      )}

      <EnvFileEditor projectPath={project.path} fileName={activeFile} />
    </div>
  );
}

function EnvFileEditor({
  projectPath,
  fileName,
}: {
  projectPath: string;
  fileName: string;
}) {
  const entries = useEnvFileEntries(projectPath, fileName);
  const setEntry = useEnvFileSet(projectPath, fileName);
  const deleteEntry = useEnvFileDeleteKey(projectPath, fileName);
  const [revealed, setRevealed] = useState<Set<string>>(new Set());

  function toggleRevealed(key: string) {
    setRevealed((prev) => {
      const next = new Set(prev);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  }

  const rows = entries.data ?? [];

  return (
    <div className="space-y-3">
      {entries.isLoading && (
        <p className="text-sm text-muted-foreground">Loading…</p>
      )}

      {!entries.isLoading && rows.length === 0 && (
        <p className="rounded-md border border-dashed px-3 py-6 text-center text-sm text-muted-foreground">
          Nothing in {fileName} yet.
        </p>
      )}

      <ul className="space-y-1" aria-label="Environment variables">
        {rows.map((entry) => (
          <li
            key={entry.key}
            className="group flex items-center gap-2 rounded-md border px-2.5 py-1.5"
          >
            <span className="w-40 shrink-0 truncate font-mono text-xs">
              {entry.key}
            </span>
            <span className="min-w-0 flex-1 truncate font-mono text-xs text-muted-foreground">
              {revealed.has(entry.key) ? entry.value : "••••••••"}
            </span>
            <Button
              variant="ghost"
              size="icon"
              className="size-7 shrink-0"
              aria-label={
                revealed.has(entry.key)
                  ? `Hide ${entry.key}`
                  : `Reveal ${entry.key}`
              }
              onClick={() => toggleRevealed(entry.key)}
            >
              {revealed.has(entry.key) ? (
                <EyeOff className="size-3.5" />
              ) : (
                <Eye className="size-3.5" />
              )}
            </Button>
            <Button
              variant="ghost"
              size="icon"
              className="size-7 shrink-0 text-muted-foreground opacity-0 hover:text-destructive group-hover:opacity-100 focus-visible:opacity-100"
              aria-label={`Delete ${entry.key}`}
              onClick={() =>
                deleteEntry.mutate(entry.key, {
                  onSuccess: () => toast.success(`Deleted "${entry.key}"`),
                  onError: (error) => toast.error(String(error)),
                })
              }
            >
              <Trash2 className="size-3.5" />
            </Button>
          </li>
        ))}
      </ul>

      <AddEntryForm
        existing={rows.map((r) => r.key)}
        onSet={(key, value) =>
          setEntry.mutate(
            { key, value },
            {
              onSuccess: () => toast.success(`Saved "${key}"`),
              onError: (error) => toast.error(String(error)),
            },
          )
        }
        pending={setEntry.isPending}
      />
    </div>
  );
}

function AddEntryForm({
  existing,
  onSet,
  pending,
}: {
  existing: string[];
  onSet: (key: string, value: string) => void;
  pending: boolean;
}) {
  const [key, setKey] = useState("");
  const [value, setValue] = useState("");
  const overwriting = existing.includes(key.trim());
  const canSubmit = key.trim() !== "" && !pending;

  function submit() {
    if (!canSubmit) return;
    onSet(key.trim(), value);
    setKey("");
    setValue("");
  }

  return (
    <form
      className="space-y-3 rounded-lg border p-3"
      onSubmit={(event) => {
        event.preventDefault();
        submit();
      }}
    >
      <div className="grid gap-3 sm:grid-cols-2">
        <div className="space-y-1.5">
          <Label htmlFor="envvar-key">Key</Label>
          <Input
            id="envvar-key"
            value={key}
            onChange={(event) => setKey(event.target.value)}
            placeholder="PORT"
            autoComplete="off"
            spellCheck={false}
            className="font-mono text-xs"
          />
        </div>
        <div className="space-y-1.5">
          <Label htmlFor="envvar-value">Value</Label>
          <Input
            id="envvar-value"
            value={value}
            onChange={(event) => setValue(event.target.value)}
            autoComplete="off"
            spellCheck={false}
            className="font-mono text-xs"
          />
        </div>
      </div>
      <Button type="submit" size="sm" disabled={!canSubmit}>
        <Plus className="size-4" />
        {pending ? "Saving…" : overwriting ? "Replace value" : "Add"}
      </Button>
    </form>
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
          <FolderPlus className="size-6 text-muted-foreground" />
          <p className="font-medium">{title}</p>
          {children && (
            <p className="max-w-md text-sm text-muted-foreground">{children}</p>
          )}
        </CardContent>
      </Card>
    </div>
  );
}
