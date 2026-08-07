import { useState } from "react";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import {
  DatabaseBackup,
  FolderOpen,
  RotateCcw,
  TriangleAlert,
} from "lucide-react";
import { toast } from "sonner";

import { ipc, isDesktopShell, type BackupEntry } from "@/shared/ipc/client";
import { formatBytes } from "@/shared/lib/format";
import { Button } from "@/shared/ui/button";
import { Badge } from "@/shared/ui/badge";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/shared/ui/card";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/shared/ui/dialog";
import { Input } from "@/shared/ui/input";
import { Label } from "@/shared/ui/label";
import {
  CONFIRM_WORD,
  KIND_HINT,
  KIND_LABEL,
  formatBackupTime,
  isRestoreConfirmed,
  sortBackups,
} from "./backups";
import { useBackups, useCancelRestore, useStageRestore } from "./hooks";

/**
 * Backups, and the way back from one.
 *
 * Restoring is destructive and unrecoverable in the ordinary sense, so it is
 * built like the SQL write toggle rather than like a save button: the button
 * that starts it says what it replaces, the dialog behind it says it again in
 * the specific — this file, replacing your current data — and the confirm
 * button stays disabled until the word is typed.
 *
 * The one thing that makes it survivable is not in this file: the kernel
 * preserves the current database before replacing it, and that copy comes back
 * in this same list as "Replaced by a restore". The dialog says so, because a
 * warning that only threatens is a warning people learn to click past.
 */
export function BackupsSection() {
  const { data: listing, isLoading } = useBackups();
  const [pendingChoice, setPendingChoice] = useState<BackupEntry | null>(null);

  const entries = listing ? sortBackups(listing.entries) : [];
  const staged = listing?.pending ?? null;

  return (
    <Card id="backups">
      <CardHeader>
        <CardTitle className="flex items-center gap-2 text-base">
          <DatabaseBackup className="size-4 text-muted-foreground" />
          Backups
        </CardTitle>
        <CardDescription>
          DevOS snapshots its database at startup — once a day, and again
          before any schema migration. Restoring one replaces everything the
          app knows: workspaces, projects, conversations, saved requests,
          snippets, monitors. The database in use at the time is always kept
          first, and reappears in this list, so a restore can itself be undone.
        </CardDescription>
      </CardHeader>

      <CardContent className="space-y-4">
        {!isDesktopShell() ? (
          <p className="text-sm text-muted-foreground">
            Backups live beside the database in the desktop shell — a browser
            tab can&apos;t reach them.
          </p>
        ) : (
          <>
            {staged && <StagedBanner source={staged.source} />}

            <div className="flex items-center justify-between gap-3">
              <p className="text-[11px] uppercase tracking-wider text-muted-foreground">
                {entries.length === 0
                  ? "Backup folder"
                  : `${entries.length} backup${entries.length === 1 ? "" : "s"}`}
              </p>
              {listing && (
                <Button
                  variant="ghost"
                  size="sm"
                  className="h-7 gap-1.5 text-muted-foreground"
                  onClick={() => reveal(listing.dir)}
                >
                  <FolderOpen className="size-3.5" />
                  Reveal folder
                </Button>
              )}
            </div>

            {isLoading && (
              <p className="text-sm text-muted-foreground">Loading…</p>
            )}

            {listing && entries.length === 0 && (
              <p className="rounded-md border border-dashed px-3 py-6 text-center text-sm text-muted-foreground">
                No backups yet. The first one is written the next time DevOS
                starts.
              </p>
            )}

            <ul className="space-y-1">
              {entries.map((entry) => (
                <li
                  key={entry.name}
                  className="group flex items-center gap-3 rounded-md px-2 py-2 hover:bg-accent"
                >
                  <div className="min-w-0 flex-1 space-y-1">
                    <div className="flex items-center gap-2">
                      <span className="truncate font-mono text-xs">
                        {entry.name}
                      </span>
                      <Badge
                        variant={
                          entry.kind === "replaced" ? "outline" : "secondary"
                        }
                        title={KIND_HINT[entry.kind]}
                      >
                        {KIND_LABEL[entry.kind]}
                      </Badge>
                    </div>
                    <p className="text-[11px] text-muted-foreground">
                      {formatBackupTime(entry.modifiedAt)} ·{" "}
                      {formatBytes(entry.size)}
                    </p>
                  </div>
                  <Button
                    variant="ghost"
                    size="icon"
                    className="size-7 shrink-0 text-muted-foreground opacity-0 group-hover:opacity-100 focus-visible:opacity-100"
                    aria-label={`Reveal ${entry.name} in Explorer`}
                    title="Reveal in Explorer"
                    onClick={() => reveal(entry.path)}
                  >
                    <FolderOpen className="size-3.5" />
                  </Button>
                  <Button
                    variant="outline"
                    size="sm"
                    className="h-7 shrink-0"
                    disabled={staged !== null}
                    onClick={() => setPendingChoice(entry)}
                  >
                    Restore…
                  </Button>
                </li>
              ))}
            </ul>
          </>
        )}
      </CardContent>

      <RestoreDialog
        entry={pendingChoice}
        onClose={() => setPendingChoice(null)}
      />
    </Card>
  );
}

function reveal(path: string) {
  revealItemInDir(path).catch((error: unknown) =>
    toast.error(`Could not open the folder: ${String(error)}`),
  );
}

/**
 * A staged restore is a loaded gun sitting in the app's data directory: it
 * fires on the next launch whether or not anyone remembers asking for it. So
 * it gets a banner rather than a line of muted text, and cancelling is always
 * one click away.
 */
function StagedBanner({ source }: { source: string }) {
  const cancel = useCancelRestore();

  return (
    <div className="space-y-3 rounded-lg border border-destructive/40 bg-destructive/5 p-3">
      <div className="flex gap-2">
        <TriangleAlert className="mt-0.5 size-4 shrink-0 text-destructive" />
        <div className="space-y-1 text-sm">
          <p className="font-medium">Restore staged</p>
          <p className="text-muted-foreground">
            DevOS will replace its database with{" "}
            <span className="font-mono text-foreground">{source}</span> the next
            time it starts. Nothing has changed yet.
          </p>
        </div>
      </div>
      <div className="flex items-center gap-2">
        <Button
          size="sm"
          variant="destructive"
          onClick={() => {
            // This call never settles — the process is replaced. A rejection
            // means the restart did not happen, so it is the only branch
            // worth reporting.
            ipc
              .backupRestart()
              .catch((error: unknown) =>
                toast.error(`Could not restart: ${String(error)}`),
              );
          }}
        >
          <RotateCcw className="size-3.5" />
          Restart now
        </Button>
        <Button
          size="sm"
          variant="ghost"
          disabled={cancel.isPending}
          onClick={() =>
            cancel.mutate(undefined, {
              onSuccess: () => toast.success("Staged restore cancelled"),
              onError: (error) => toast.error(String(error)),
            })
          }
        >
          Cancel restore
        </Button>
      </div>
    </div>
  );
}

function RestoreDialog({
  entry,
  onClose,
}: {
  entry: BackupEntry | null;
  onClose: () => void;
}) {
  const stage = useStageRestore();
  const [typed, setTyped] = useState("");

  function close() {
    setTyped("");
    onClose();
  }

  const armed = isRestoreConfirmed(typed) && !stage.isPending;

  return (
    <Dialog open={entry !== null} onOpenChange={(open) => !open && close()}>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <TriangleAlert className="size-4 text-destructive" />
            Replace your database with this backup?
          </DialogTitle>
          <DialogDescription asChild>
            <div className="space-y-2">
              <p>
                Everything DevOS has recorded since{" "}
                <span className="font-mono text-foreground">
                  {entry ? formatBackupTime(entry.modifiedAt) : ""}
                </span>{" "}
                will stop being what the app uses — workspaces, projects, AI
                conversations, saved requests, snippets and monitors all come
                from this one file.
              </p>
              <p>
                Your current database is copied into the backups folder first,
                as{" "}
                <span className="font-mono text-foreground">
                  devos-replaced-…
                </span>
                , and will appear in this list. That copy is the only way back,
                so do not delete it.
              </p>
              <p>
                Nothing happens until DevOS restarts. You can cancel until then.
              </p>
            </div>
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-3">
          <div className="rounded-md border bg-muted/40 p-3 text-xs">
            <p className="font-mono break-all">{entry?.name}</p>
            <p className="pt-1 text-muted-foreground">
              {entry ? KIND_HINT[entry.kind] : ""}
            </p>
          </div>

          <div className="space-y-1.5">
            <Label htmlFor="restore-confirm">
              Type <span className="font-mono">{CONFIRM_WORD}</span> to confirm
            </Label>
            <Input
              id="restore-confirm"
              value={typed}
              onChange={(event) => setTyped(event.target.value)}
              autoComplete="off"
              spellCheck={false}
              className="font-mono text-xs"
            />
          </div>
        </div>

        <DialogFooter>
          <Button variant="ghost" onClick={close}>
            Cancel
          </Button>
          <Button
            variant="destructive"
            disabled={!armed}
            onClick={() => {
              if (!entry || !armed) return;
              stage.mutate(entry.name, {
                onSuccess: () => {
                  toast.success(`Restore staged from "${entry.name}"`);
                  close();
                },
                onError: (error) => toast.error(String(error)),
              });
            }}
          >
            {stage.isPending ? "Staging…" : "Stage restore"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
