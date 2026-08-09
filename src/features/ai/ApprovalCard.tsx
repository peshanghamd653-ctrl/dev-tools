import { useState } from "react";

import { cn } from "@/shared/lib/utils";
import { Button } from "@/shared/ui/button";
import type { PendingApproval } from "./hooks";

/**
 * How much of one field is shown before the rest is folded away.
 *
 * The card used to slice every value at 2000 characters and append a bare
 * "…", inside a `max-h-40` pane — so the remainder was neither visible nor
 * reachable, and padding a `run_command` argument pushed the payload out of
 * sight entirely. `security.md` claims the user approves with "the full
 * arguments" in front of them, so the rule here is: a fold is always
 * *labelled* with what it hides, and the full text is always one click away
 * (SEC-008).
 */
const INLINE_CHARS = 600;

interface ApprovalField {
  label: string;
  value: string;
  /** Why this field matters, shown next to the label. */
  hint?: string;
}

/**
 * What the user is actually consenting to, per tool. Anything unrecognised
 * falls back to the raw JSON rather than being summarised away — an unknown
 * mutating tool is exactly when guessing is worst.
 */
function approvalFields(approval: PendingApproval): ApprovalField[] {
  let input: Record<string, unknown>;
  try {
    input = JSON.parse(approval.input) as Record<string, unknown>;
  } catch {
    return [{ label: "Input", value: approval.input }];
  }

  const str = (key: string) =>
    typeof input[key] === "string" ? (input[key] as string) : "";

  switch (approval.name) {
    case "run_command":
      return [{ label: "Command", value: str("command") }];
    case "edit_file":
      return [
        { label: "File", value: str("path") },
        { label: "Replace", value: str("old_string") },
        { label: "With", value: str("new_string") },
      ];
    case "write_file":
      return [
        { label: "New file", value: str("path") },
        { label: "Content", value: str("content") },
      ];
    case "save_memory":
      return [
        {
          label: "Remember",
          value: str("content"),
          hint: "stored, and added to every future conversation about this project",
        },
      ];
    case "run_tests":
      return [
        {
          label: "Command",
          value: str("command"),
          hint: "detected for this project — not chosen by the model",
        },
      ];
    default:
      return [{ label: "Input", value: approval.input }];
  }
}

/**
 * `%s wants to ...`, filled in with whichever provider is actually running —
 * this used to hardcode "Claude" for every tool, which became wrong the
 * moment Ollama could drive the same tool loop (see
 * `providerSupportsTools` in `./providers.ts`). A denied or approved call
 * from an Ollama conversation was captioning itself with the wrong model's
 * name for as long as that gap existed.
 */
const TITLE_TEMPLATES: Record<string, string> = {
  run_command: "wants to run a command",
  edit_file: "wants to edit a file",
  write_file: "wants to create a file",
  save_memory: "wants to save this to project memory",
  run_tests: "wants to run the test suite",
};

function FieldView({ field }: { field: ApprovalField }) {
  const [expanded, setExpanded] = useState(false);
  const total = field.value.length;
  const folded = total > INLINE_CHARS && !expanded;
  const hidden = total - INLINE_CHARS;
  const shown = folded ? field.value.slice(0, INLINE_CHARS) : field.value;

  return (
    <div>
      <p className="text-[10px] uppercase tracking-wider text-muted-foreground">
        {field.label}
        {field.hint && (
          <span className="normal-case tracking-normal"> — {field.hint}</span>
        )}
        {total > INLINE_CHARS && (
          <span className="normal-case tracking-normal">
            {" "}
            · {total.toLocaleString()} characters
          </span>
        )}
      </p>
      <pre
        className={cn(
          "overflow-auto whitespace-pre-wrap break-all rounded bg-card px-2 py-1 font-mono text-xs",
          expanded ? "max-h-[60vh]" : "max-h-40",
        )}
      >
        {shown}
      </pre>
      {total > INLINE_CHARS && (
        <div className="flex items-center gap-2 pt-1">
          {folded && (
            <span className="text-[11px] text-yellow-500">
              {hidden.toLocaleString()} more characters hidden
            </span>
          )}
          <button
            type="button"
            onClick={() => setExpanded(!expanded)}
            className="text-[11px] underline underline-offset-2 hover:text-foreground"
          >
            {folded
              ? `Show all ${total.toLocaleString()} characters`
              : "Show less"}
          </button>
        </div>
      )}
    </div>
  );
}

/** Consent card for one mutating tool call. */
export function ApprovalCard({
  approval,
  onRespond,
  providerLabel = "The AI",
}: {
  approval: PendingApproval;
  onRespond: (id: string, approved: boolean) => void;
  /** e.g. "Claude", "Ollama" — whichever provider this conversation runs. */
  providerLabel?: string;
}) {
  const fields = approvalFields(approval);
  const action = TITLE_TEMPLATES[approval.name] ?? "wants to run a tool";

  return (
    <div className="rounded-lg border border-yellow-500/40 bg-yellow-500/5 p-3">
      <div className="flex items-center gap-2 pb-2">
        <span className="text-sm">⚡</span>
        <p className="text-sm font-medium">
          {providerLabel} {action}{" "}
          <span className="font-mono text-xs text-muted-foreground">
            {approval.name}
          </span>
        </p>
      </div>
      <div className="space-y-1.5 pb-3">
        {fields.map((field) => (
          <FieldView key={field.label} field={field} />
        ))}
      </div>
      <div className="flex gap-2">
        <Button size="sm" onClick={() => onRespond(approval.id, true)}>
          Approve
        </Button>
        <Button
          size="sm"
          variant="outline"
          onClick={() => onRespond(approval.id, false)}
        >
          Deny
        </Button>
      </div>
    </div>
  );
}
