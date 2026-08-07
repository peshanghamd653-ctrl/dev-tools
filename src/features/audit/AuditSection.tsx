import { ScrollText, ShieldAlert } from "lucide-react";

import { isDesktopShell, type AuditEntry } from "@/shared/ipc/client";
import { Badge } from "@/shared/ui/badge";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/shared/ui/card";
import {
  describeAction,
  describeActor,
  describeCoverage,
  formatAuditTime,
  isRefusal,
} from "./audit";
import { useAuditLog } from "./hooks";

/**
 * The audit log viewer.
 *
 * **Why it lives in Settings rather than in the sidebar.** The nav is already
 * thirteen items, and every one of them is somewhere you go *to do work*. This
 * is not that: it is the screen you open once, after something has already
 * gone wrong, to answer "did I approve that?" A fourteenth permanent nav entry
 * would charge every session for a surface used a handful of times a year, and
 * the things it records — secrets, backups, restores — are already managed on
 * this page, so it is where someone investigating is already standing.
 *
 * It renders as a list and nothing else. No filters, no search, no export, no
 * clear button — particularly no clear button, since an audit log with an
 * "empty this" control is a suggestion, not a record.
 */
export function AuditSection() {
  const { data: log, isLoading } = useAuditLog();
  const entries = log?.entries ?? [];

  return (
    <Card id="audit">
      <CardHeader>
        <CardTitle className="flex items-center gap-2 text-base">
          <ScrollText className="size-4 text-muted-foreground" />
          Audit log
        </CardTitle>
        <CardDescription>
          The handful of actions worth reconstructing later: AI tool calls you
          approved or refused, secrets stored or deleted, writes run from the
          SQL editor, issues filed, and database restores. Secret{" "}
          <strong>values</strong> are never recorded, nor are SQL statements,
          issue bodies or file contents — only that the action happened, to
          what, and when.
        </CardDescription>
      </CardHeader>

      <CardContent className="space-y-4">
        {!isDesktopShell() ? (
          <p className="text-sm text-muted-foreground">
            The audit log lives in the desktop shell — a browser tab can&apos;t
            reach it.
          </p>
        ) : (
          <>
            {isLoading && (
              <p className="text-sm text-muted-foreground">Loading…</p>
            )}

            {log && entries.length === 0 && (
              <p className="rounded-md border border-dashed px-3 py-6 text-center text-sm text-muted-foreground">
                Nothing recorded yet. Entries appear here the first time you
                approve an AI tool call, store a secret, or run a write in the
                SQL editor.
              </p>
            )}

            {entries.length > 0 && (
              <ul className="divide-y rounded-md border">
                {entries.map((entry) => (
                  <AuditRow key={entry.id} entry={entry} />
                ))}
              </ul>
            )}

            {/* The retention policy, stated rather than implied. Rendered
                whether or not there are rows, because "nothing here" and
                "nothing here, and anything older than 90 days is gone" are
                different answers to the same question. */}
            {log && (
              <p className="text-[11px] leading-relaxed text-muted-foreground">
                {describeCoverage(log)}
              </p>
            )}
          </>
        )}
      </CardContent>
    </Card>
  );
}

function AuditRow({ entry }: { entry: AuditEntry }) {
  const refused = isRefusal(entry);

  return (
    <li className="flex items-baseline gap-3 px-3 py-2 text-sm">
      <span className="w-32 shrink-0 font-mono text-[11px] text-muted-foreground">
        {formatAuditTime(entry.createdAt)}
      </span>
      <span className="flex min-w-0 flex-1 flex-col gap-0.5">
        <span className="flex items-center gap-2">
          {refused && (
            <ShieldAlert
              className="size-3.5 shrink-0 text-destructive"
              aria-hidden
            />
          )}
          <span className={refused ? "text-destructive" : undefined}>
            {describeAction(entry.action)}
          </span>
          <Badge variant="secondary" className="shrink-0 font-normal">
            {describeActor(entry.actor)}
          </Badge>
        </span>
        {entry.detail && (
          <span className="break-all font-mono text-[11px] text-muted-foreground">
            {entry.detail}
          </span>
        )}
      </span>
    </li>
  );
}
