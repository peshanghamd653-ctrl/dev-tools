import { useState } from "react";
import { Check, Pencil, Trash2, X } from "lucide-react";
import { toast } from "sonner";

import { useAppInfo } from "@/features/app/hooks";
import { SecretsSection } from "@/features/secrets/SecretsSection";
import {
  useDeleteWorkspace,
  useRenameWorkspace,
  useWorkspaces,
} from "@/features/workspaces/hooks";
import { Button } from "@/shared/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/shared/ui/card";
import { Input } from "@/shared/ui/input";
import { ThemePicker } from "@/shared/theme/ThemePicker";
import { Separator } from "@/shared/ui/separator";

export function SettingsPage() {
  const { data: appInfo } = useAppInfo();

  return (
    <div className="mx-auto max-w-3xl space-y-6 p-6">
      <div>
        <h1 className="text-xl font-semibold tracking-tight">Settings</h1>
        <p className="text-sm text-muted-foreground">
          Application preferences and workspace management
        </p>
      </div>

      <Card>
        <CardHeader>
          <CardTitle className="text-base">Workspaces</CardTitle>
          <CardDescription>
            Rename or remove workspaces. The last workspace cannot be deleted.
          </CardDescription>
        </CardHeader>
        <CardContent>
          <WorkspaceList />
        </CardContent>
      </Card>

      <SecretsSection />

      <Card>
        <CardHeader>
          <CardTitle className="text-base">Appearance</CardTitle>
          <CardDescription>
            DevOS is dark-first — Midnight is the default. Your choice is
            remembered and applied before the window paints.
          </CardDescription>
        </CardHeader>
        <CardContent>
          <ThemePicker />
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle className="text-base">About</CardTitle>
          <CardDescription>Runtime details for this install</CardDescription>
        </CardHeader>
        <CardContent className="space-y-3 text-sm">
          <InfoRow label="Version" value={appInfo?.version ?? "—"} />
          <Separator />
          <InfoRow
            label="Kernel startup"
            value={appInfo ? `${appInfo.startupMs} ms` : "—"}
          />
          <Separator />
          <InfoRow label="Database" value={appInfo?.dbPath ?? "—"} mono />
        </CardContent>
      </Card>
    </div>
  );
}

function InfoRow({
  label,
  value,
  mono,
}: {
  label: string;
  value: string;
  mono?: boolean;
}) {
  return (
    <div className="flex items-baseline justify-between gap-4">
      <span className="shrink-0 text-muted-foreground">{label}</span>
      <span className={mono ? "truncate font-mono text-xs" : ""}>{value}</span>
    </div>
  );
}

function WorkspaceList() {
  const { data: workspaces } = useWorkspaces();
  const renameWorkspace = useRenameWorkspace();
  const deleteWorkspace = useDeleteWorkspace();
  const [editingId, setEditingId] = useState<string | null>(null);
  const [draftName, setDraftName] = useState("");

  if (!workspaces) {
    return <p className="text-sm text-muted-foreground">Loading…</p>;
  }

  function commitRename(id: string) {
    renameWorkspace.mutate(
      { id, name: draftName },
      {
        onSuccess: () => {
          toast.success("Workspace renamed");
          setEditingId(null);
        },
        onError: (error) => toast.error(String(error)),
      },
    );
  }

  return (
    <ul className="space-y-1">
      {workspaces.map((workspace) => (
        <li
          key={workspace.id}
          className="flex h-10 items-center gap-2 rounded-md px-2 hover:bg-accent"
        >
          {editingId === workspace.id ? (
            <>
              <Input
                value={draftName}
                onChange={(e) => setDraftName(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") commitRename(workspace.id);
                  if (e.key === "Escape") setEditingId(null);
                }}
                className="h-7 max-w-xs"
                autoFocus
              />
              <Button
                variant="ghost"
                size="icon"
                className="size-7"
                onClick={() => commitRename(workspace.id)}
                aria-label="Save name"
              >
                <Check className="size-4" />
              </Button>
              <Button
                variant="ghost"
                size="icon"
                className="size-7"
                onClick={() => setEditingId(null)}
                aria-label="Cancel rename"
              >
                <X className="size-4" />
              </Button>
            </>
          ) : (
            <>
              <span className="flex-1 text-sm">{workspace.name}</span>
              <Button
                variant="ghost"
                size="icon"
                className="size-7 text-muted-foreground"
                onClick={() => {
                  setEditingId(workspace.id);
                  setDraftName(workspace.name);
                }}
                aria-label={`Rename ${workspace.name}`}
              >
                <Pencil className="size-3.5" />
              </Button>
              <Button
                variant="ghost"
                size="icon"
                className="size-7 text-muted-foreground hover:text-destructive"
                disabled={workspaces.length <= 1}
                onClick={() =>
                  deleteWorkspace.mutate(workspace.id, {
                    onSuccess: () =>
                      toast.success(`Deleted "${workspace.name}"`),
                    onError: (error) => toast.error(String(error)),
                  })
                }
                aria-label={`Delete ${workspace.name}`}
              >
                <Trash2 className="size-3.5" />
              </Button>
            </>
          )}
        </li>
      ))}
    </ul>
  );
}
