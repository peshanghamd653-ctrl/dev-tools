import { useState } from "react";
import { EyeOff, KeyRound, Trash2 } from "lucide-react";
import { toast } from "sonner";

import { isDesktopShell } from "@/shared/ipc/client";
import { Button } from "@/shared/ui/button";
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
import { useSecretDelete, useSecretNames, useSecretSet } from "./hooks";
import { normalizeSecretName, secretNameStatus, sortSecretNames } from "./names";

export function SecretsSection() {
  const { data: names, isLoading } = useSecretNames();
  const [pendingDelete, setPendingDelete] = useState<string | null>(null);

  const stored = names ? sortSecretNames(names) : [];

  return (
    <Card id="secrets">
      <CardHeader>
        <CardTitle className="flex items-center gap-2 text-base">
          <KeyRound className="size-4 text-muted-foreground" />
          Secrets
        </CardTitle>
        <CardDescription>
          API keys and tokens, encrypted with a master key held in the OS
          credential store. Values are write-only: DevOS can list the names
          below but has no way to read a value back out, so there is nothing to
          reveal and no way to recover one you have lost.
        </CardDescription>
      </CardHeader>

      <CardContent className="space-y-5">
        {!isDesktopShell() ? (
          <p className="text-sm text-muted-foreground">
            The secret store lives in the desktop shell — a browser tab can't
            reach it.
          </p>
        ) : (
          <>
            <div className="space-y-1">
              <div className="flex items-center gap-1.5 pb-1 text-[11px] uppercase tracking-wider text-muted-foreground">
                <EyeOff className="size-3" />
                Stored names only — never values
              </div>

              {isLoading && (
                <p className="text-sm text-muted-foreground">Loading…</p>
              )}

              {names && stored.length === 0 && (
                <p className="rounded-md border border-dashed px-3 py-6 text-center text-sm text-muted-foreground">
                  Nothing stored yet.
                </p>
              )}

              <ul className="space-y-1">
                {stored.map((name) => (
                  <li
                    key={name}
                    className="group flex h-10 items-center gap-2 rounded-md px-2 hover:bg-accent"
                  >
                    <span className="min-w-0 flex-1 truncate font-mono text-sm">
                      {name}
                    </span>
                    <span
                      className="shrink-0 font-mono text-xs tracking-widest text-muted-foreground"
                      title="The stored value is never sent to the interface"
                      aria-label="Value hidden — it never leaves the encrypted store"
                    >
                      ••••••••
                    </span>
                    <Button
                      variant="ghost"
                      size="icon"
                      className="size-7 shrink-0 text-muted-foreground opacity-0 hover:text-destructive group-hover:opacity-100 focus-visible:opacity-100"
                      aria-label={`Delete ${name}`}
                      title={`Delete ${name}`}
                      onClick={() => setPendingDelete(name)}
                    >
                      <Trash2 className="size-3.5" />
                    </Button>
                  </li>
                ))}
              </ul>
            </div>

            <AddSecretForm existing={stored} />

            <p className="text-[11px] leading-relaxed text-muted-foreground">
              DevOS reads two names itself today:{" "}
              <code className="font-mono text-foreground">vercel_token</code>{" "}
              for the deployments page and{" "}
              <code className="font-mono text-foreground">
                anthropic-api-key
              </code>{" "}
              for the AI assistant. Every other name is yours — a module that
              needs a credential says which name it looks for.
            </p>
          </>
        )}
      </CardContent>

      <DeleteSecretDialog
        name={pendingDelete}
        onClose={() => setPendingDelete(null)}
      />
    </Card>
  );
}

function AddSecretForm({ existing }: { existing: string[] }) {
  const save = useSecretSet();
  const [name, setName] = useState("");
  const [value, setValue] = useState("");

  const status = secretNameStatus(existing, name);
  const canSubmit = status !== "empty" && value !== "" && !save.isPending;

  function submit() {
    if (!canSubmit) return;
    const secretName = normalizeSecretName(name);
    save.mutate(
      { name: secretName, value },
      {
        onSuccess: () => {
          // Out of component state the moment it is stored — a password field
          // still holds a readable string in memory and in React DevTools.
          setValue("");
          setName("");
          toast.success(
            status === "overwrite"
              ? `Replaced the value stored under "${secretName}"`
              : `Stored "${secretName}"`,
          );
        },
        onError: (failure) => toast.error(String(failure)),
      },
    );
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
          <Label htmlFor="secret-name">Name</Label>
          <Input
            id="secret-name"
            value={name}
            onChange={(event) => setName(event.target.value)}
            placeholder="vercel_token"
            autoComplete="off"
            spellCheck={false}
            className="font-mono text-xs"
          />
        </div>
        <div className="space-y-1.5">
          <Label htmlFor="secret-value">Value</Label>
          <Input
            id="secret-value"
            type="password"
            value={value}
            onChange={(event) => setValue(event.target.value)}
            placeholder="••••••••••••"
            autoComplete="off"
            spellCheck={false}
            className="font-mono text-xs"
          />
        </div>
      </div>

      <div className="flex items-center gap-3">
        <Button type="submit" size="sm" disabled={!canSubmit}>
          {save.isPending
            ? "Saving…"
            : status === "overwrite"
              ? "Replace value"
              : "Save secret"}
        </Button>
        <p className="text-[11px] text-muted-foreground">
          {status === "overwrite"
            ? `A secret named "${normalizeSecretName(name)}" already exists — saving replaces its value, and the old one is gone.`
            : "Saving over a name that already exists replaces its value."}
        </p>
      </div>
    </form>
  );
}

/**
 * Deleting is unrecoverable in the strongest sense: the value was never
 * readable, so there is no copy anywhere in DevOS to restore it from.
 */
function DeleteSecretDialog({
  name,
  onClose,
}: {
  name: string | null;
  onClose: () => void;
}) {
  const remove = useSecretDelete();

  return (
    <Dialog open={name !== null} onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>Delete this secret?</DialogTitle>
          <DialogDescription>
            <span className="font-mono text-foreground">{name}</span> is
            encrypted and unreadable, so DevOS cannot show you what it is about
            to remove and cannot put it back. Anything using this name stops
            working until you store a new value under it.
          </DialogDescription>
        </DialogHeader>
        <DialogFooter>
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button
            variant="destructive"
            disabled={remove.isPending}
            onClick={() => {
              if (!name) return;
              remove.mutate(name, {
                onSuccess: () => {
                  toast.success(`Deleted "${name}"`);
                  onClose();
                },
                onError: (failure) => toast.error(String(failure)),
              });
            }}
          >
            {remove.isPending ? "Deleting…" : "Delete secret"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
