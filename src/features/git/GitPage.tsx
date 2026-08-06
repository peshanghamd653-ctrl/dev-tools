import { useState } from "react";
import {
  ArrowDownToLine,
  ArrowUpFromLine,
  Check,
  ChevronsUpDown,
  GitBranch as GitBranchIcon,
  History,
  Loader2,
  Minus,
  Plus,
  RotateCcw,
  Sparkles,
} from "lucide-react";
import { toast } from "sonner";

import { useAiStore } from "@/features/ai/store";

import { useProjects } from "@/features/projects/hooks";
import { useActiveWorkspace } from "@/features/workspaces/hooks";
import { isDesktopShell, ipc, type GitFileEntry } from "@/shared/ipc/client";
import { cn } from "@/shared/lib/utils";
import { Badge } from "@/shared/ui/badge";
import { Button } from "@/shared/ui/button";
import { Card, CardContent } from "@/shared/ui/card";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/shared/ui/dropdown-menu";
import { Input } from "@/shared/ui/input";
import { DiffViewer } from "./DiffViewer";
import {
  useGitBranches,
  useGitDiff,
  useGitLog,
  useGitMutations,
  useGitStatus,
} from "./hooks";
import { useGitStore } from "./store";

interface SelectedFile {
  path: string;
  staged: boolean;
  untracked: boolean;
}

export function GitPage() {
  const activeWorkspace = useActiveWorkspace();
  const { data: projects } = useProjects(activeWorkspace?.id);
  const selectedProjectId = useGitStore((s) => s.selectedProjectId);
  const setSelectedProject = useGitStore((s) => s.setSelectedProject);

  const project =
    projects?.find((p) => p.id === selectedProjectId) ?? projects?.[0] ?? null;
  const repoPath = project?.path;

  const { data: status } = useGitStatus(repoPath);
  const { data: branches } = useGitBranches(repoPath);
  const { data: commits } = useGitLog(repoPath);
  const mutations = useGitMutations(repoPath);

  const [tab, setTab] = useState<"changes" | "history">("changes");
  const [selected, setSelected] = useState<SelectedFile | null>(null);
  const [message, setMessage] = useState("");
  const [newBranch, setNewBranch] = useState<string | null>(null);
  const [generating, setGenerating] = useState(false);
  const aiProvider = useAiStore((s) => s.provider);
  const aiModel = useAiStore((s) => s.model);

  function generateMessage() {
    if (!repoPath || generating) return;
    setGenerating(true);
    ipc
      .aiCommitMessage(repoPath, aiProvider, aiModel)
      .then((text) => setMessage(text))
      .catch((error) => toast.error(String(error)))
      .finally(() => setGenerating(false));
  }

  const { data: diff } = useGitDiff(
    repoPath,
    selected?.path ?? null,
    selected?.staged ?? false,
    selected?.untracked ?? false,
  );

  if (!isDesktopShell()) {
    return (
      <Notice title="Git needs the desktop shell">
        This page is running in a plain browser tab.
      </Notice>
    );
  }
  if (!project) {
    return (
      <Notice title="No project selected">
        Add a project first — git operates on a project folder.
      </Notice>
    );
  }

  const entries = status?.entries ?? [];
  const stagedFiles = entries.filter(entryHasStaged);
  const unstagedFiles = entries.filter(entryHasUnstaged);
  const info = status?.info;

  return (
    <div className="flex h-full flex-col">
      <header className="flex h-11 shrink-0 items-center gap-2 border-b px-3">
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button variant="ghost" size="sm" className="gap-1.5">
              {project.name}
              <ChevronsUpDown className="size-3.5 text-muted-foreground" />
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="start">
            <DropdownMenuLabel>Repository</DropdownMenuLabel>
            {projects?.map((p) => (
              <DropdownMenuItem key={p.id} onSelect={() => setSelectedProject(p.id)}>
                {p.name}
                {p.id === project.id && <Check className="ml-auto size-4" />}
              </DropdownMenuItem>
            ))}
          </DropdownMenuContent>
        </DropdownMenu>

        {info?.isRepo && (
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button variant="outline" size="sm" className="gap-1.5">
                <GitBranchIcon className="size-3.5" />
                {info.branch ?? "detached"}
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="start">
              <DropdownMenuLabel>Branches</DropdownMenuLabel>
              {branches?.map((b) => (
                <DropdownMenuItem
                  key={b.name}
                  onSelect={() =>
                    mutations.switchBranch.mutate(
                      { branch: b.name, create: false },
                      {
                        onSuccess: () => toast.success(`Switched to ${b.name}`),
                        onError: (e) => toast.error(String(e)),
                      },
                    )
                  }
                >
                  {b.name}
                  {b.current && <Check className="ml-auto size-4" />}
                </DropdownMenuItem>
              ))}
              <DropdownMenuSeparator />
              <DropdownMenuItem onSelect={() => setNewBranch("")}>
                <Plus className="size-4" />
                New branch…
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        )}

        {newBranch !== null && (
          <Input
            value={newBranch}
            onChange={(e) => setNewBranch(e.target.value)}
            placeholder="new-branch-name"
            className="h-7 w-44 font-mono text-xs"
            autoFocus
            onKeyDown={(e) => {
              if (e.key === "Escape") setNewBranch(null);
              if (e.key === "Enter" && newBranch.trim()) {
                mutations.switchBranch.mutate(
                  { branch: newBranch.trim(), create: true },
                  {
                    onSuccess: () => {
                      toast.success(`Created ${newBranch.trim()}`);
                      setNewBranch(null);
                    },
                    onError: (err) => toast.error(String(err)),
                  },
                );
              }
            }}
          />
        )}

        {info?.isRepo && (info.ahead > 0 || info.behind > 0) && (
          <Badge variant="secondary" className="text-[10px]">
            {info.ahead > 0 && `↑${info.ahead}`} {info.behind > 0 && `↓${info.behind}`}
          </Badge>
        )}

        <div className="flex-1" />

        {info?.isRepo && (
          <>
            <Button
              variant="ghost"
              size="sm"
              disabled={mutations.pull.isPending}
              onClick={() =>
                mutations.pull.mutate(undefined, {
                  onSuccess: () => toast.success("Pulled"),
                  onError: (e) => toast.error(String(e)),
                })
              }
            >
              <ArrowDownToLine className="size-4" />
              Pull
            </Button>
            <Button
              variant="ghost"
              size="sm"
              disabled={mutations.push.isPending}
              onClick={() =>
                mutations.push.mutate(undefined, {
                  onSuccess: () => toast.success("Pushed"),
                  onError: (e) => toast.error(String(e)),
                })
              }
            >
              <ArrowUpFromLine className="size-4" />
              Push
            </Button>
          </>
        )}
      </header>

      {info && !info.isRepo ? (
        <Notice title="Not a git repository">
          {project.path} has no .git directory. Initialize it from the terminal
          with <code className="font-mono text-xs">git init</code>.
        </Notice>
      ) : (
        <div className="grid min-h-0 flex-1 grid-cols-[340px_1fr]">
          <aside className="flex min-h-0 flex-col border-r">
            <div className="flex h-9 shrink-0 items-center gap-1 border-b px-2">
              <TabButton
                active={tab === "changes"}
                onClick={() => setTab("changes")}
                label={`Changes${entries.length ? ` (${entries.length})` : ""}`}
              />
              <TabButton
                active={tab === "history"}
                onClick={() => setTab("history")}
                label="History"
                icon={<History className="size-3.5" />}
              />
            </div>

            {tab === "changes" ? (
              <>
                <div className="min-h-0 flex-1 overflow-y-auto p-2">
                  <FileSection
                    title="Staged"
                    files={stagedFiles}
                    staged
                    selected={selected}
                    onSelect={(f) =>
                      setSelected({ path: f.path, staged: true, untracked: false })
                    }
                    action={(f) => (
                      <RowAction
                        label="Unstage"
                        icon={<Minus className="size-3.5" />}
                        onClick={() => mutations.unstage.mutate([f.path])}
                      />
                    )}
                  />
                  <FileSection
                    title="Changes"
                    files={unstagedFiles}
                    staged={false}
                    selected={selected}
                    onSelect={(f) =>
                      setSelected({
                        path: f.path,
                        staged: false,
                        untracked: f.untracked,
                      })
                    }
                    action={(f) => (
                      <span className="flex">
                        <RowAction
                          label="Discard changes"
                          icon={<RotateCcw className="size-3.5" />}
                          onClick={() =>
                            mutations.discard.mutate(
                              { file: f.path, untracked: f.untracked },
                              {
                                onSuccess: () =>
                                  toast.success(`Discarded ${f.path}`),
                                onError: (e) => toast.error(String(e)),
                              },
                            )
                          }
                        />
                        <RowAction
                          label="Stage"
                          icon={<Plus className="size-3.5" />}
                          onClick={() => mutations.stage.mutate([f.path])}
                        />
                      </span>
                    )}
                  />
                  {entries.length === 0 && (
                    <p className="px-2 py-6 text-center text-xs text-muted-foreground">
                      Working tree clean.
                    </p>
                  )}
                </div>

                <div className="shrink-0 space-y-2 border-t p-2">
                  <div className="relative">
                    <textarea
                      value={message}
                      onChange={(e) => setMessage(e.target.value)}
                      placeholder="Commit message"
                      rows={3}
                      className="w-full resize-none rounded-md border bg-transparent px-2.5 py-1.5 pr-9 text-sm outline-none placeholder:text-muted-foreground focus-visible:border-ring"
                    />
                    <Button
                      variant="ghost"
                      size="icon"
                      className="absolute right-1.5 top-1.5 size-6 text-muted-foreground hover:text-primary"
                      aria-label="Generate commit message with AI"
                      title={`Generate with ${aiProvider} · ${aiModel}`}
                      disabled={stagedFiles.length === 0 || generating}
                      onClick={generateMessage}
                    >
                      {generating ? (
                        <Loader2 className="size-3.5 animate-spin" />
                      ) : (
                        <Sparkles className="size-3.5" />
                      )}
                    </Button>
                  </div>
                  <Button
                    className="w-full"
                    size="sm"
                    disabled={
                      stagedFiles.length === 0 ||
                      !message.trim() ||
                      mutations.commit.isPending
                    }
                    onClick={() =>
                      mutations.commit.mutate(message, {
                        onSuccess: (id) => {
                          toast.success(`Committed ${id.slice(0, 7)}`);
                          setMessage("");
                        },
                        onError: (e) => toast.error(String(e)),
                      })
                    }
                  >
                    Commit to {info?.branch ?? "…"}
                  </Button>
                </div>
              </>
            ) : (
              <div className="min-h-0 flex-1 overflow-y-auto p-2">
                {commits?.length === 0 && (
                  <p className="px-2 py-6 text-center text-xs text-muted-foreground">
                    No commits yet.
                  </p>
                )}
                <ul className="space-y-0.5">
                  {commits?.map((c) => (
                    <li
                      key={c.id}
                      className="rounded-md px-2 py-1.5 hover:bg-accent"
                    >
                      <p className="truncate text-sm">{c.summary}</p>
                      <p className="text-[11px] text-muted-foreground">
                        <span className="font-mono">{c.shortId}</span> ·{" "}
                        {c.author} · {timeAgo(c.time * 1000)}
                      </p>
                    </li>
                  ))}
                </ul>
              </div>
            )}
          </aside>

          <section className="min-h-0">
            {selected ? (
              <div className="flex h-full flex-col">
                <div className="flex h-9 shrink-0 items-center gap-2 border-b px-3">
                  <span className="truncate font-mono text-xs">
                    {selected.path}
                  </span>
                  <Badge variant="outline" className="text-[10px]">
                    {selected.staged ? "staged" : "working tree"}
                  </Badge>
                </div>
                <div className="min-h-0 flex-1">
                  <DiffViewer diff={diff ?? ""} />
                </div>
              </div>
            ) : (
              <div className="flex h-full items-center justify-center text-sm text-muted-foreground">
                Select a file to view its diff.
              </div>
            )}
          </section>
        </div>
      )}
    </div>
  );
}

function entryHasStaged(e: GitFileEntry): boolean {
  return !e.untracked && !e.conflicted && e.staged !== ".";
}

function entryHasUnstaged(e: GitFileEntry): boolean {
  return e.untracked || e.conflicted || e.unstaged !== ".";
}

const STATUS_COLORS: Record<string, string> = {
  M: "text-yellow-400",
  A: "text-emerald-400",
  D: "text-red-400",
  R: "text-sky-400",
  C: "text-sky-400",
  U: "text-emerald-400",
  "!": "text-orange-400",
};

function FileSection({
  title,
  files,
  staged,
  selected,
  onSelect,
  action,
}: {
  title: string;
  files: GitFileEntry[];
  staged: boolean;
  selected: SelectedFile | null;
  onSelect: (f: GitFileEntry) => void;
  action: (f: GitFileEntry) => React.ReactNode;
}) {
  if (files.length === 0) return null;
  return (
    <div className="mb-3">
      <p className="px-2 pb-1 text-[10px] font-medium uppercase tracking-wider text-muted-foreground">
        {title} ({files.length})
      </p>
      <ul className="space-y-0.5">
        {files.map((f) => {
          const letter = f.conflicted
            ? "!"
            : f.untracked
              ? "U"
              : staged
                ? f.staged
                : f.unstaged;
          const isSelected =
            selected?.path === f.path && selected?.staged === staged;
          return (
            <li
              key={`${f.path}-${staged}`}
              className={cn(
                "group flex cursor-pointer items-center gap-2 rounded-md px-2 py-1",
                isSelected ? "bg-accent" : "hover:bg-accent/60",
              )}
              onClick={() => onSelect(f)}
            >
              <span
                className={cn(
                  "w-3 shrink-0 text-center font-mono text-xs font-semibold",
                  STATUS_COLORS[letter] ?? "text-muted-foreground",
                )}
              >
                {letter}
              </span>
              <span className="min-w-0 flex-1 truncate text-xs">{f.path}</span>
              <span className="opacity-0 transition-opacity group-hover:opacity-100">
                {action(f)}
              </span>
            </li>
          );
        })}
      </ul>
    </div>
  );
}

function RowAction({
  label,
  icon,
  onClick,
}: {
  label: string;
  icon: React.ReactNode;
  onClick: () => void;
}) {
  return (
    <Button
      variant="ghost"
      size="icon"
      className="size-5"
      aria-label={label}
      title={label}
      onClick={(e) => {
        e.stopPropagation();
        onClick();
      }}
    >
      {icon}
    </Button>
  );
}

function TabButton({
  active,
  onClick,
  label,
  icon,
}: {
  active: boolean;
  onClick: () => void;
  label: string;
  icon?: React.ReactNode;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={cn(
        "flex h-6 items-center gap-1 rounded px-2 text-xs",
        active
          ? "bg-accent text-foreground"
          : "text-muted-foreground hover:bg-accent/60",
      )}
    >
      {icon}
      {label}
    </button>
  );
}

function Notice({
  title,
  children,
}: {
  title: string;
  children: React.ReactNode;
}) {
  return (
    <div className="mx-auto max-w-3xl p-6">
      <Card>
        <CardContent className="flex flex-col items-center gap-2 py-12 text-center">
          <GitBranchIcon className="size-6 text-muted-foreground" />
          <p className="font-medium">{title}</p>
          <p className="text-sm text-muted-foreground">{children}</p>
        </CardContent>
      </Card>
    </div>
  );
}

function timeAgo(ms: number): string {
  const seconds = Math.floor((Date.now() - ms) / 1000);
  if (seconds < 60) return "just now";
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  const days = Math.floor(hours / 24);
  if (days < 30) return `${days}d ago`;
  return new Date(ms).toLocaleDateString();
}
