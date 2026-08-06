import { useEffect, useRef, useState } from "react";
import { useNavigate } from "@tanstack/react-router";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  Bug,
  Camera,
  ClipboardCopy,
  ExternalLink,
  Eraser,
  FileQuestion,
  Highlighter,
  KeyRound,
  ShieldAlert,
  TriangleAlert,
  Undo2,
} from "lucide-react";
import { toast } from "sonner";

import { useAppInfo } from "@/features/app/hooks";
import {
  isDesktopShell,
  ipc,
  type CapturedShot,
  type CreatedIssue,
} from "@/shared/ipc/client";
import { cn } from "@/shared/lib/utils";
import { useDialogStore } from "@/shared/stores/dialogs";
import { Button } from "@/shared/ui/button";
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
import { ShotAnnotator } from "./ShotAnnotator";
import {
  classifyIssueError,
  copyPlan,
  defaultTargetKey,
  finalBody,
  GITHUB_TOKEN_SECRET,
  initialIssueBody,
  normalizeTitle,
  remotesLabel,
  targetChoices,
  type Annotation,
  type AnnotationTool,
  type SystemFacts,
  type TargetChoice,
} from "./compose";
import {
  useActiveProject,
  useIssueCapture,
  useIssueConfigured,
  useIssueCreate,
  useIssueTargets,
} from "./hooks";

type Step = "annotate" | "compose" | "review" | "done";

/**
 * Screenshot → GitHub issue. Mounted in the shell, opened by `Ctrl+Shift+S`
 * or the `issue.capture` palette entry — an action, not a page.
 *
 * The capture happens *before* anything renders. A dialog that opened first
 * and then asked for a screenshot would photograph itself, and the bug the
 * user is trying to report would be underneath it.
 */
export function CaptureIssueDialog() {
  const open = useDialogStore((s) => s.captureIssueOpen);
  const setOpen = useDialogStore((s) => s.setCaptureIssueOpen);

  const configured = useIssueConfigured(open);
  const {
    mutate: runCapture,
    reset: resetCapture,
    isPending: capturing,
    isError: captureFailed,
    error: captureError,
  } = useIssueCapture();
  const [shot, setShot] = useState<CapturedShot | null>(null);

  useEffect(() => {
    if (!open || configured.data !== true) return;
    if (shot !== null || capturing || captureFailed) return;
    // Whatever opened this — the command palette, another dialog — is on a
    // 200ms exit animation. Firing immediately photographs it half faded out,
    // sitting over the bug the user meant to show.
    const timer = setTimeout(
      () => runCapture(undefined, { onSuccess: setShot }),
      260,
    );
    return () => clearTimeout(timer);
  }, [captureFailed, capturing, configured.data, open, runCapture, shot]);

  useEffect(() => {
    if (open) return;
    setShot(null);
    resetCapture();
  }, [open, resetCapture]);

  // Everything except "we are mid-capture" is worth showing. While the
  // screenshot is being taken the shell stays exactly as the user left it.
  const visible =
    open &&
    (!isDesktopShell() ||
      configured.isError ||
      configured.data === false ||
      captureFailed ||
      shot !== null);

  return (
    <Dialog open={visible} onOpenChange={(next) => !next && setOpen(false)}>
      <DialogContent
        className={cn(
          "max-h-[90vh] overflow-y-auto",
          shot ? "sm:max-w-3xl" : "sm:max-w-md",
        )}
      >
        {!isDesktopShell() ? (
          <Notice
            icon={<Camera className="size-5 text-muted-foreground" />}
            title="Reporting a bug needs the desktop shell"
          >
            The screenshot is taken by the DevOS process and the GitHub token
            lives in its encrypted secret store — a browser tab has neither.
          </Notice>
        ) : configured.isError ? (
          <Notice
            icon={<TriangleAlert className="size-5 text-red-300" />}
            title="The issue module didn't answer"
          >
            {String(configured.error)}
          </Notice>
        ) : configured.data === false ? (
          <NotConfigured onNavigate={() => setOpen(false)} />
        ) : captureFailed ? (
          <Notice
            icon={<TriangleAlert className="size-5 text-red-300" />}
            title="The screenshot could not be taken"
          >
            <>
              {String(captureError)}
              <span className="mt-3 block">
                <Button size="sm" onClick={() => resetCapture()}>
                  Try again
                </Button>
              </span>
            </>
          </Notice>
        ) : shot ? (
          <IssueFlow shot={shot} onClose={() => setOpen(false)} />
        ) : null}
      </DialogContent>
    </Dialog>
  );
}

function IssueFlow({
  shot,
  onClose,
}: {
  shot: CapturedShot;
  onClose: () => void;
}) {
  const { data: appInfo } = useAppInfo();
  const project = useActiveProject();
  const targets = useIssueTargets(project?.path ?? null);
  const create = useIssueCreate();

  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const [step, setStep] = useState<Step>("annotate");
  const [tool, setTool] = useState<AnnotationTool>("redact");
  const [annotations, setAnnotations] = useState<Annotation[]>([]);
  const [shotReady, setShotReady] = useState(false);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [exportError, setExportError] = useState<string | null>(null);
  /**
   * The flattened PNG, produced once when the annotate step is left. From
   * that moment the only image this dialog holds is the one with the
   * redactions burned into it.
   */
  const [flattened, setFlattened] = useState<Blob | null>(null);
  const [title, setTitle] = useState("");
  const [body, setBody] = useState("");
  const [targetKey, setTargetKey] = useState<string | null>(null);
  const [created, setCreated] = useState<CreatedIssue | null>(null);

  const facts: SystemFacts = {
    appVersion: appInfo?.version ?? null,
    userAgent: typeof navigator === "undefined" ? "" : navigator.userAgent,
    shot: { width: shot.width, height: shot.height },
  };

  const choices = targets.data ? targetChoices(targets.data) : [];
  const target = choices.find((choice) => choice.key === targetKey) ?? null;

  // Keyed off the query's own array, not the derived one: `targetChoices`
  // returns a fresh array every render, and depending on it would re-run this
  // on every keystroke in the title field.
  useEffect(() => {
    if (targetKey !== null || !targets.data) return;
    setTargetKey(defaultTargetKey(targetChoices(targets.data)));
  }, [targetKey, targets.data]);

  useEffect(() => {
    if (step !== "annotate") return;
    function onKeyDown(event: KeyboardEvent) {
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "z") {
        event.preventDefault();
        setAnnotations((current) => current.slice(0, -1));
      }
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [step]);

  const redactions = annotations.filter((a) => a.tool === "redact").length;

  /**
   * Reads the canvas back as a PNG. This is the security-load-bearing step:
   * what leaves here is the bitmap the user was looking at, with the opaque
   * rectangles already part of it. The captured file is never re-read.
   */
  async function flattenAndContinue() {
    const canvas = canvasRef.current;
    if (!canvas || !shotReady) return;
    setExportError(null);
    try {
      const blob = await new Promise<Blob | null>((resolve, reject) => {
        try {
          canvas.toBlob((result) => resolve(result), "image/png");
        } catch (error) {
          reject(error instanceof Error ? error : new Error(String(error)));
        }
      });
      if (!blob) throw new Error("the canvas produced no image data");
      setFlattened(blob);
    } catch (error) {
      // Never fall through to the original file. If the flattened export
      // failed, the only image DevOS could offer is the unredacted one.
      setExportError(
        `DevOS could not export the annotated image (${error}). Nothing is copied or filed until it can — the captured file still contains everything you covered up.`,
      );
      return;
    }
    if (body === "") setBody(initialIssueBody(facts));
    setStep("compose");
  }

  async function copyImage() {
    const plan = copyPlan({
      annotated: annotations.length > 0,
      webviewClipboard: webviewCanWriteImages(),
    });
    try {
      if (plan === "canvas") {
        if (!flattened) throw new Error("the annotated image is no longer held");
        await navigator.clipboard.write([
          new ClipboardItem({ "image/png": flattened }),
        ]);
      } else if (plan === "file") {
        // Only reachable when nothing was drawn, so the file on disk and the
        // flattened canvas are the same picture.
        await ipc.issueCopyImage(shot.path);
      } else {
        toast.error(
          "This webview can't put an image on the clipboard, and DevOS won't copy the captured file instead — it still shows everything you redacted.",
        );
        return;
      }
      toast.success(
        annotations.length > 0
          ? "Annotated screenshot copied — paste it into the issue."
          : "Screenshot copied — paste it into the issue.",
      );
    } catch (error) {
      toast.error(`Could not copy the image: ${error}`);
    }
  }

  if (step === "done" && created) {
    return (
      <Created
        issue={created}
        annotated={annotations.length > 0}
        onCopy={copyImage}
        onClose={onClose}
      />
    );
  }

  if (step === "review") {
    return (
      <>
        <DialogHeader>
          <DialogTitle>Review before filing</DialogTitle>
          <DialogDescription>
            This is exactly what will be posted to{" "}
            <span className="font-mono text-foreground">
              {target?.label ?? "—"}
            </span>
            . Edit anything here; nothing is added after you press the button.
          </DialogDescription>
        </DialogHeader>

        <p className="flex items-start gap-1.5 rounded-md border border-yellow-500/30 bg-yellow-500/10 px-2.5 py-1.5 text-[11px] text-yellow-200">
          <TriangleAlert className="mt-px size-3 shrink-0" />
          Creating an issue is the one thing DevOS does that writes to the
          outside world. On a public repository this text is public, and it
          cannot be un-filed — only edited or closed afterwards.
        </p>

        <div className="space-y-1.5">
          <Label htmlFor="issue-review-title">Title</Label>
          <Input
            id="issue-review-title"
            value={title}
            onChange={(event) => setTitle(event.target.value)}
          />
        </div>

        <div className="space-y-1.5">
          <Label htmlFor="issue-review-body">Body</Label>
          <textarea
            id="issue-review-body"
            value={body}
            onChange={(event) => setBody(event.target.value)}
            spellCheck={false}
            rows={12}
            className="w-full resize-y rounded-md border bg-transparent px-2.5 py-1.5 font-mono text-xs outline-none focus-visible:border-ring"
          />
        </div>

        <p className="text-[11px] text-muted-foreground">
          The screenshot is not part of this text. GitHub documents no API for
          attaching an image to an issue, so DevOS puts it on your clipboard
          after the issue exists and you paste it in.
        </p>

        {create.isError && <CreateFailure error={create.error} target={target} />}

        <DialogFooter>
          <Button
            variant="ghost"
            disabled={create.isPending}
            onClick={() => setStep("compose")}
          >
            Back
          </Button>
          <Button
            disabled={create.isPending || !target || normalizeTitle(title) === ""}
            onClick={() => {
              if (!target) return;
              create.mutate(
                {
                  owner: target.owner,
                  name: target.name,
                  title: normalizeTitle(title),
                  body,
                },
                {
                  onSuccess: (issue) => {
                    setCreated(issue);
                    setStep("done");
                  },
                },
              );
            }}
          >
            {create.isPending ? "Creating…" : "Create issue"}
          </Button>
        </DialogFooter>
      </>
    );
  }

  if (step === "compose") {
    return (
      <>
        <DialogHeader>
          <DialogTitle>Describe the bug</DialogTitle>
          <DialogDescription>
            The system block is filled in for you and is yours to edit — no
            terminal output or file contents are collected.
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-1.5">
          <Label htmlFor="issue-title">Title</Label>
          <Input
            id="issue-title"
            value={title}
            onChange={(event) => setTitle(event.target.value)}
            placeholder="e.g. Terminal loses focus after switching workspace"
            autoFocus
          />
        </div>

        <div className="space-y-1.5">
          <Label htmlFor="issue-body">Body</Label>
          <textarea
            id="issue-body"
            value={body}
            onChange={(event) => setBody(event.target.value)}
            spellCheck={false}
            rows={10}
            className="w-full resize-y rounded-md border bg-transparent px-2.5 py-1.5 font-mono text-xs outline-none focus-visible:border-ring"
          />
        </div>

        <TargetPicker
          projectName={project?.name ?? null}
          loading={targets.isLoading}
          error={targets.error}
          choices={choices}
          selected={targetKey}
          onSelect={setTargetKey}
        />

        <DialogFooter>
          <Button variant="ghost" onClick={() => setStep("annotate")}>
            Back
          </Button>
          <Button
            disabled={!target || normalizeTitle(title) === ""}
            onClick={() => {
              // Tidied once, here — after this the review pane and the request
              // body are the same string.
              setBody(finalBody(body));
              setStep("review");
            }}
          >
            Review
          </Button>
        </DialogFooter>
      </>
    );
  }

  return (
    <>
      <DialogHeader>
        <DialogTitle>Cover anything private, then continue</DialogTitle>
        <DialogDescription>
          A screenshot of a developer machine usually has more in it than the
          bug — an .env file, a token in a terminal, someone else's window.
          Redaction is painted into the image itself, so what is covered is
          gone from every copy that leaves DevOS.
        </DialogDescription>
      </DialogHeader>

      <div className="flex flex-wrap items-center gap-1.5">
        <ToolButton
          active={tool === "redact"}
          onClick={() => setTool("redact")}
          title="Drag a rectangle. It is filled solid — not blurred, not pixelated — and the pixels underneath are destroyed."
        >
          <Eraser className="size-3" />
          Redact
        </ToolButton>
        <ToolButton
          active={tool === "highlight"}
          onClick={() => setTool("highlight")}
          title="Drag a rectangle to point at the bug. Adds an outline; changes nothing underneath."
        >
          <Highlighter className="size-3" />
          Highlight
        </ToolButton>

        <Button
          variant="ghost"
          size="sm"
          className="h-7"
          disabled={annotations.length === 0}
          onClick={() => setAnnotations((current) => current.slice(0, -1))}
          title="Undo the last shape (Ctrl+Z)"
        >
          <Undo2 className="size-3" />
          Undo
        </Button>

        <span className="ml-auto text-[11px] text-muted-foreground">
          {redactions === 0
            ? "Nothing redacted yet"
            : `${redactions} redacted area${redactions === 1 ? "" : "s"}`}
        </span>
      </div>

      <ShotAnnotator
        shot={shot}
        tool={tool}
        annotations={annotations}
        canvasRef={canvasRef}
        onCommit={(annotation) =>
          setAnnotations((current) => [...current, annotation])
        }
        onReady={setShotReady}
        onLoadError={setLoadError}
      />

      {(loadError || exportError) && (
        <p className="flex items-start gap-1.5 rounded-md border border-destructive/40 bg-destructive/10 px-2.5 py-1.5 text-[11px] text-red-300">
          <TriangleAlert className="mt-px size-3 shrink-0" />
          <span className="min-w-0 break-words">{loadError ?? exportError}</span>
        </p>
      )}

      <DialogFooter>
        <Button variant="ghost" onClick={onClose}>
          Cancel
        </Button>
        <Button
          disabled={!shotReady || loadError !== null}
          onClick={() => void flattenAndContinue()}
        >
          Continue
        </Button>
      </DialogFooter>
    </>
  );
}

function ToolButton({
  active,
  title,
  onClick,
  children,
}: {
  active: boolean;
  title: string;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      aria-pressed={active}
      title={title}
      className={cn(
        "inline-flex items-center gap-1.5 rounded-full border px-3 py-1 text-xs transition-colors",
        active
          ? "border-primary/40 bg-primary/10 text-foreground"
          : "border-border text-muted-foreground hover:text-foreground",
      )}
    >
      {children}
    </button>
  );
}

/**
 * Which repository the issue goes to. DevOS never picks one when git offers
 * none — filing a bug into whatever repository happened to be nearby is worse
 * than not filing it.
 */
function TargetPicker({
  projectName,
  loading,
  error,
  choices,
  selected,
  onSelect,
}: {
  projectName: string | null;
  loading: boolean;
  error: unknown;
  choices: TargetChoice[];
  selected: string | null;
  onSelect: (key: string) => void;
}) {
  if (projectName === null) {
    return (
      <FieldNote>
        No project is selected, so DevOS has no git remotes to read. Add a
        project and reopen this dialog to file against its repository.
      </FieldNote>
    );
  }

  if (loading) {
    return <FieldNote>Reading the git remotes in {projectName}…</FieldNote>;
  }

  if (error) {
    const failure = classifyIssueError(error);
    if (failure === "unconfigured" || failure === "auth") {
      // Handled at the top of the flow and on submit; here it is just a note.
      return (
        <FieldNote tone="bad">
          {failure === "auth"
            ? `GitHub rejected the stored ${GITHUB_TOKEN_SECRET}. Replace it in Settings → Secrets.`
            : `No ${GITHUB_TOKEN_SECRET} is stored. Add one in Settings → Secrets.`}
        </FieldNote>
      );
    }
    return <FieldNote tone="bad">{String(error)}</FieldNote>;
  }

  if (choices.length === 0) {
    return (
      <FieldNote tone="bad">
        No GitHub remote found in {projectName}. DevOS will not guess a
        repository, so there is nothing to file against — add a GitHub remote,
        or open a project that has one.
      </FieldNote>
    );
  }

  const only = choices.length === 1 ? choices[0] : undefined;
  if (only) {
    return (
      <div className="space-y-1.5">
        <Label>Repository</Label>
        <p className="rounded-md border px-2.5 py-1.5 font-mono text-xs">
          {only.label}
          {remotesLabel(only) && (
            <span className="pl-2 font-sans text-[11px] text-muted-foreground">
              {remotesLabel(only)}
            </span>
          )}
        </p>
      </div>
    );
  }

  return (
    <div className="space-y-1.5">
      <Label>Repository</Label>
      <div className="flex flex-wrap gap-1.5">
        {choices.map((choice) => (
          <button
            key={choice.key}
            type="button"
            aria-pressed={choice.key === selected}
            onClick={() => onSelect(choice.key)}
            className={cn(
              "rounded-full border px-3 py-1 text-left text-xs transition-colors",
              choice.key === selected
                ? "border-primary/40 bg-primary/10 text-foreground"
                : "border-border text-muted-foreground hover:text-foreground",
            )}
          >
            <span className="font-mono">{choice.label}</span>
            {remotesLabel(choice) && (
              <span className="pl-2 text-[10px] text-muted-foreground">
                {remotesLabel(choice)}
              </span>
            )}
          </button>
        ))}
      </div>
      <p className="text-[11px] text-muted-foreground">
        This project has more than one GitHub remote. Pick the one whose issue
        tracker you mean — a fork's upstream belongs to somebody else.
      </p>
    </div>
  );
}

/**
 * Three failures that arrive as one string and need three different actions.
 * A 404 in particular is almost never "the repository is gone".
 */
function CreateFailure({
  error,
  target,
}: {
  error: unknown;
  target: TargetChoice | null;
}) {
  const failure = classifyIssueError(error);

  if (failure === "unconfigured") {
    return (
      <FieldNote tone="bad" icon={<KeyRound className="size-3" />}>
        No <code className="font-mono">{GITHUB_TOKEN_SECRET}</code> is stored.
        Add one in Settings → Secrets and the issue can be filed — nothing was
        sent.
      </FieldNote>
    );
  }

  if (failure === "auth") {
    return (
      <FieldNote tone="bad" icon={<ShieldAlert className="size-3" />}>
        A <code className="font-mono">{GITHUB_TOKEN_SECRET}</code> is stored and
        GitHub refused it — revoked, expired, or without permission to open
        issues. Save a replacement under the same name in Settings → Secrets.
      </FieldNote>
    );
  }

  if (failure === "notFound") {
    return (
      <FieldNote tone="bad" icon={<FileQuestion className="size-3" />}>
        GitHub answered 404 for{" "}
        <code className="font-mono">{target?.label ?? "that repository"}</code>.
        That usually means the stored token cannot see it rather than that it is
        missing: the API returns 404 instead of 403 for a private repository, so
        it does not confirm the repository exists. Check the token's repository
        access — and that issues are enabled there.
      </FieldNote>
    );
  }

  return <FieldNote tone="bad">{String(error)}</FieldNote>;
}

function Created({
  issue,
  annotated,
  onCopy,
  onClose,
}: {
  issue: CreatedIssue;
  annotated: boolean;
  onCopy: () => Promise<void>;
  onClose: () => void;
}) {
  return (
    <>
      <DialogHeader>
        <DialogTitle>Issue #{issue.number} created</DialogTitle>
        <DialogDescription>{issue.title}</DialogDescription>
      </DialogHeader>

      <div className="space-y-3">
        <Button
          variant="outline"
          size="sm"
          className="w-full justify-start"
          onClick={() =>
            void openUrl(issue.url).catch((failure) =>
              toast.error(`Could not open your browser: ${failure}`),
            )
          }
        >
          <ExternalLink className="size-3.5" />
          <span className="truncate font-mono text-xs">{issue.url}</span>
        </Button>

        <div className="rounded-md border p-3">
          <Button size="sm" onClick={() => void onCopy()}>
            <ClipboardCopy className="size-3.5" />
            Copy image
          </Button>
          <p className="pt-2 text-[11px] leading-relaxed text-muted-foreground">
            GitHub has no API for attaching an image to an issue, so this is the
            last step you do by hand: copy the{" "}
            {annotated ? "annotated " : ""}screenshot, open the issue above, and
            paste it into a comment. Nothing failed — that upload is the one
            part GitHub does not expose.
          </p>
        </div>
      </div>

      <DialogFooter>
        <Button onClick={onClose}>Done</Button>
      </DialogFooter>
    </>
  );
}

/**
 * No token has ever been stored. Deliberately a different screen from the
 * rejected-token and 404 messages: this one asks for a credential, the others
 * ask for a *different* credential and for different repository access.
 */
function NotConfigured({ onNavigate }: { onNavigate: () => void }) {
  const navigate = useNavigate();

  return (
    <Notice
      icon={<KeyRound className="size-5 text-muted-foreground" />}
      title="Connect GitHub to file issues"
    >
      <>
        DevOS opens issues through the GitHub API, so it needs a personal access
        token with permission to create them. Store one in Settings → Secrets
        under the name{" "}
        <code className="font-mono text-foreground">{GITHUB_TOKEN_SECRET}</code>{" "}
        and this dialog takes the screenshot instead.
        <span className="mt-3 block">
          <Button
            size="sm"
            onClick={() => {
              onNavigate();
              void navigate({ to: "/settings" });
            }}
          >
            Open Settings
          </Button>
        </span>
      </>
    </Notice>
  );
}

function FieldNote({
  children,
  tone = "neutral",
  icon,
}: {
  children: React.ReactNode;
  tone?: "neutral" | "bad";
  icon?: React.ReactNode;
}) {
  return (
    <p
      className={cn(
        "flex items-start gap-1.5 rounded-md border px-2.5 py-1.5 text-[11px]",
        tone === "bad"
          ? "border-destructive/40 bg-destructive/10 text-red-300"
          : "border-border text-muted-foreground",
      )}
    >
      {icon && <span className="mt-px shrink-0">{icon}</span>}
      <span className="min-w-0 break-words">{children}</span>
    </p>
  );
}

function Notice({
  title,
  icon,
  children,
}: {
  title: string;
  icon?: React.ReactNode;
  children?: React.ReactNode;
}) {
  return (
    <>
      <DialogHeader>
        <DialogTitle className="flex items-center gap-2">
          {icon ?? <Bug className="size-5 text-muted-foreground" />}
          {title}
        </DialogTitle>
      </DialogHeader>
      {children && (
        <div className="text-sm text-muted-foreground">{children}</div>
      )}
    </>
  );
}

/**
 * Whether this webview can put an image on the clipboard itself. WebView2 and
 * WKWebView both can; checking rather than assuming is what keeps the
 * fallback in {@link copyPlan} from being dead code with a live consequence.
 */
function webviewCanWriteImages(): boolean {
  return (
    typeof ClipboardItem !== "undefined" &&
    typeof navigator !== "undefined" &&
    typeof navigator.clipboard?.write === "function"
  );
}
