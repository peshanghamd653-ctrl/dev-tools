import { useState } from "react";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import {
  Check,
  ChevronDown,
  ChevronRight,
  ChevronsUpDown,
  Copy,
  File as FileIcon,
  Folder,
  FolderOpen,
  FolderTree,
  Search,
  X,
} from "lucide-react";
import { toast } from "sonner";

import { useProjects } from "@/features/projects/hooks";
import { useActiveWorkspace } from "@/features/workspaces/hooks";
import { useGitStore } from "@/features/git/store";
import { isDesktopShell } from "@/shared/ipc/client";
import { formatBytes } from "@/shared/lib/format";
import { cn } from "@/shared/lib/utils";
import { Button } from "@/shared/ui/button";
import { Card, CardContent } from "@/shared/ui/card";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuTrigger,
} from "@/shared/ui/dropdown-menu";
import { Input } from "@/shared/ui/input";
import { useDirEntries, useFilePreview, useFileSearch } from "./hooks";

const MAX_RENDERED_LINES = 5000;

export function FileExplorerPage() {
  const activeWorkspace = useActiveWorkspace();
  const { data: projects } = useProjects(activeWorkspace?.id);
  const selectedProjectId = useGitStore((s) => s.selectedProjectId);
  const setSelectedProject = useGitStore((s) => s.setSelectedProject);
  const project =
    projects?.find((p) => p.id === selectedProjectId) ?? projects?.[0] ?? null;

  const [selected, setSelected] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  const [query, setQuery] = useState("");

  const search = useFileSearch(project?.path, query);

  if (!isDesktopShell()) {
    return (
      <Notice title="The file explorer needs the desktop shell" />
    );
  }
  if (!project) {
    return <Notice title="No project selected — add one first" />;
  }

  function openFile(relative: string) {
    setSelected(relative);
    setQuery("");
    setDraft("");
  }

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
            <DropdownMenuLabel>Project</DropdownMenuLabel>
            {projects?.map((p) => (
              <DropdownMenuItem
                key={p.id}
                onSelect={() => {
                  setSelectedProject(p.id);
                  setSelected(null);
                }}
              >
                {p.name}
                {p.id === project.id && <Check className="ml-auto size-4" />}
              </DropdownMenuItem>
            ))}
          </DropdownMenuContent>
        </DropdownMenu>

        <div className="relative max-w-md flex-1">
          <Search className="absolute left-2.5 top-1/2 size-3.5 -translate-y-1/2 text-muted-foreground" />
          <Input
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") setQuery(draft.trim());
              if (e.key === "Escape") {
                setDraft("");
                setQuery("");
              }
            }}
            placeholder="Search file names and content…  (Enter)"
            className="h-8 pl-8 pr-8 text-xs"
          />
          {(draft || query) && (
            <button
              type="button"
              aria-label="Clear search"
              className="absolute right-2 top-1/2 -translate-y-1/2 text-muted-foreground hover:text-foreground"
              onClick={() => {
                setDraft("");
                setQuery("");
              }}
            >
              <X className="size-3.5" />
            </button>
          )}
        </div>
      </header>

      <div className="grid min-h-0 flex-1 grid-cols-[300px_1fr]">
        <aside className="min-h-0 overflow-y-auto border-r p-1.5">
          <DirNode
            projectPath={project.path}
            relative=""
            depth={0}
            selected={selected}
            onSelect={openFile}
          />
        </aside>

        <section className="min-h-0">
          {search.active ? (
            <SearchResults
              names={search.names.data}
              content={search.content.data}
              query={query}
              onOpen={openFile}
            />
          ) : selected ? (
            <PreviewPane projectPath={project.path} relative={selected} />
          ) : (
            <div className="flex h-full flex-col items-center justify-center gap-2 text-muted-foreground">
              <FolderTree className="size-6" />
              <p className="text-sm">Select a file to preview it.</p>
            </div>
          )}
        </section>
      </div>
    </div>
  );
}

function DirNode({
  projectPath,
  relative,
  depth,
  selected,
  onSelect,
}: {
  projectPath: string;
  relative: string;
  depth: number;
  selected: string | null;
  onSelect: (relative: string) => void;
}) {
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const { data: entries } = useDirEntries(projectPath, relative);

  return (
    <ul>
      {entries?.map((entry) => {
        const childRel = relative ? `${relative}/${entry.name}` : entry.name;
        const isOpen = expanded.has(entry.name);
        return (
          <li key={entry.name}>
            <button
              type="button"
              onClick={() => {
                if (entry.isDir) {
                  setExpanded((prev) => {
                    const next = new Set(prev);
                    if (next.has(entry.name)) next.delete(entry.name);
                    else next.add(entry.name);
                    return next;
                  });
                } else {
                  onSelect(childRel);
                }
              }}
              className={cn(
                "flex w-full items-center gap-1.5 rounded px-1.5 py-0.5 text-left text-xs",
                selected === childRel
                  ? "bg-accent text-foreground"
                  : "text-foreground/80 hover:bg-accent/60",
              )}
              style={{ paddingLeft: `${depth * 14 + 6}px` }}
            >
              {entry.isDir ? (
                <>
                  {isOpen ? (
                    <ChevronDown className="size-3 shrink-0 text-muted-foreground" />
                  ) : (
                    <ChevronRight className="size-3 shrink-0 text-muted-foreground" />
                  )}
                  {isOpen ? (
                    <FolderOpen className="size-3.5 shrink-0 text-muted-foreground" />
                  ) : (
                    <Folder className="size-3.5 shrink-0 text-muted-foreground" />
                  )}
                </>
              ) : (
                <FileIcon className="ml-4 size-3.5 shrink-0 text-muted-foreground/70" />
              )}
              <span className="truncate">{entry.name}</span>
            </button>
            {entry.isDir && isOpen && (
              <DirNode
                projectPath={projectPath}
                relative={childRel}
                depth={depth + 1}
                selected={selected}
                onSelect={onSelect}
              />
            )}
          </li>
        );
      })}
    </ul>
  );
}

function PreviewPane({
  projectPath,
  relative,
}: {
  projectPath: string;
  relative: string;
}) {
  const { data: preview, error } = useFilePreview(projectPath, relative);

  return (
    <div className="flex h-full flex-col">
      <div className="flex h-9 shrink-0 items-center gap-2 border-b px-3">
        <span className="min-w-0 flex-1 truncate font-mono text-xs">
          {relative}
        </span>
        {preview && (
          <span className="text-[10px] text-muted-foreground">
            {formatBytes(preview.size)}
          </span>
        )}
        <Button
          variant="ghost"
          size="icon"
          className="size-6"
          aria-label="Copy relative path"
          onClick={() =>
            void navigator.clipboard
              .writeText(relative)
              .then(() => toast.success("Path copied"))
          }
        >
          <Copy className="size-3.5" />
        </Button>
        <Button
          variant="ghost"
          size="icon"
          className="size-6"
          aria-label="Reveal in Explorer"
          onClick={() =>
            revealItemInDir(`${projectPath}\\${relative.replaceAll("/", "\\")}`)
              .catch((e) => toast.error(String(e)))
          }
        >
          <FolderOpen className="size-3.5" />
        </Button>
      </div>

      <div className="min-h-0 flex-1 overflow-auto">
        {error && (
          <p className="p-4 text-sm text-destructive">{String(error)}</p>
        )}
        {preview?.binary && (
          <p className="p-4 text-sm text-muted-foreground">
            Binary file ({formatBytes(preview.size)}) — no preview.
          </p>
        )}
        {preview && !preview.binary && (
          <>
            <table className="w-full border-collapse font-mono text-xs leading-5">
              <tbody>
                {preview.content
                  .split("\n")
                  .slice(0, MAX_RENDERED_LINES)
                  .map((line, i) => (
                    <tr key={i} className="hover:bg-accent/40">
                      <td className="w-10 select-none pr-3 text-right align-top text-muted-foreground/40">
                        {i + 1}
                      </td>
                      <td className="whitespace-pre-wrap break-all">{line}</td>
                    </tr>
                  ))}
              </tbody>
            </table>
            {(preview.truncated ||
              preview.content.split("\n").length > MAX_RENDERED_LINES) && (
              <p className="p-3 text-center text-xs text-muted-foreground">
                … preview truncated …
              </p>
            )}
          </>
        )}
      </div>
    </div>
  );
}

function SearchResults({
  names,
  content,
  query,
  onOpen,
}: {
  names: string[] | undefined;
  content:
    | { file: string; startLine: number; snippet: string }[]
    | undefined;
  query: string;
  onOpen: (relative: string) => void;
}) {
  return (
    <div className="h-full space-y-4 overflow-y-auto p-4">
      <section>
        <p className="pb-1 text-[10px] font-medium uppercase tracking-wider text-muted-foreground">
          Files ({names?.length ?? 0})
        </p>
        {names?.length === 0 && (
          <p className="text-xs text-muted-foreground">
            No file names match “{query}”.
          </p>
        )}
        <ul className="space-y-0.5">
          {names?.map((path) => (
            <li key={path}>
              <button
                type="button"
                onClick={() => onOpen(path)}
                className="w-full truncate rounded px-2 py-1 text-left font-mono text-xs hover:bg-accent"
              >
                {path}
              </button>
            </li>
          ))}
        </ul>
      </section>

      <section>
        <p className="pb-1 text-[10px] font-medium uppercase tracking-wider text-muted-foreground">
          Content ({content?.length ?? 0})
        </p>
        {content?.length === 0 && (
          <p className="text-xs text-muted-foreground">
            No indexed content matches — if this project isn’t indexed yet,
            run “Index for AI search” from the Projects page.
          </p>
        )}
        <ul className="space-y-1">
          {content?.map((hit, i) => (
            <li key={`${hit.file}-${hit.startLine}-${i}`}>
              <button
                type="button"
                onClick={() => onOpen(hit.file)}
                className="w-full rounded border border-border/60 px-2.5 py-1.5 text-left hover:bg-accent/60"
              >
                <span className="block font-mono text-xs">
                  {hit.file}:{hit.startLine}
                </span>
                <span className="line-clamp-2 block text-xs text-muted-foreground">
                  {hit.snippet.replaceAll("\n", " ")}
                </span>
              </button>
            </li>
          ))}
        </ul>
      </section>
    </div>
  );
}

function Notice({ title }: { title: string }) {
  return (
    <div className="mx-auto max-w-3xl p-6">
      <Card>
        <CardContent className="flex flex-col items-center gap-2 py-12 text-center">
          <FolderTree className="size-6 text-muted-foreground" />
          <p className="font-medium">{title}</p>
        </CardContent>
      </Card>
    </div>
  );
}
