import { useEffect, useState } from "react";
import { useNavigate } from "@tanstack/react-router";
import { FileCode2 } from "lucide-react";

import { useSymbolSearch } from "@/features/app/hooks";
import { useGitStore } from "@/features/git/store";
import { useProjects } from "@/features/projects/hooks";
import { useActiveWorkspace } from "@/features/workspaces/hooks";
import type { SymbolHit } from "@/shared/ipc/client";
import { useUiStore } from "@/shared/stores/ui";
import {
  Command,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
} from "@/shared/ui/command";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/shared/ui/dialog";

/** Debounced so a fast typist fires one query, not one per keystroke. */
const DEBOUNCE_MS = 150;

/**
 * "Go to symbol" (Ctrl+T): jump straight to a function, class, struct, or
 * type declaration by name, project-wide. Selecting a result hands the file
 * and line off to the File Explorer via `pendingFileOpen` — the same
 * queue-a-navigation pattern `AiPage` already uses for the terminal's
 * "diagnose this" handoff — and switches to it.
 */
export function SymbolSearchDialog() {
  const open = useUiStore((s) => s.symbolSearchOpen);
  const setOpen = useUiStore((s) => s.setSymbolSearchOpen);
  const setPendingFileOpen = useUiStore((s) => s.setPendingFileOpen);
  const navigate = useNavigate();

  const activeWorkspace = useActiveWorkspace();
  const { data: projects } = useProjects(activeWorkspace?.id);
  const selectedProjectId = useGitStore((s) => s.selectedProjectId);
  const project =
    projects?.find((p) => p.id === selectedProjectId) ?? projects?.[0] ?? null;

  const [query, setQuery] = useState("");
  const [debounced, setDebounced] = useState("");

  useEffect(() => {
    const id = setTimeout(() => setDebounced(query), DEBOUNCE_MS);
    return () => clearTimeout(id);
  }, [query]);

  // A dialog that reopens with the last search is disorienting for a "jump
  // somewhere fast" tool — every open starts from a clean slate.
  useEffect(() => {
    if (!open) {
      setQuery("");
      setDebounced("");
    }
  }, [open]);

  const { data: hits, isFetching } = useSymbolSearch(project?.path, debounced);

  function select(hit: SymbolHit) {
    if (!project) return;
    setOpen(false);
    setPendingFileOpen({
      projectPath: project.path,
      relative: hit.file,
      line: hit.startLine,
    });
    void navigate({ to: "/files" });
  }

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogHeader className="sr-only">
        <DialogTitle>Go to symbol</DialogTitle>
        <DialogDescription>
          Jump to a function, class, struct, or type declaration by name
        </DialogDescription>
      </DialogHeader>
      <DialogContent className="overflow-hidden p-0">
        <Command shouldFilter={false}>
          <CommandInput
            value={query}
            onValueChange={setQuery}
            placeholder={
              project
                ? `Go to symbol in ${project.name}…`
                : "Add a project first"
            }
          />
          <CommandList>
            {!project && (
              <CommandEmpty>Add a project to search its symbols.</CommandEmpty>
            )}
            {project && debounced.trim().length < 2 && (
              <CommandEmpty>Type at least 2 characters.</CommandEmpty>
            )}
            {project &&
              debounced.trim().length >= 2 &&
              !isFetching &&
              hits?.length === 0 && (
                <CommandEmpty>
                  No matching symbols. The project may need indexing.
                </CommandEmpty>
              )}
            {hits && hits.length > 0 && (
              <CommandGroup heading="Symbols">
                {hits.map((hit) => (
                  <CommandItem
                    key={`${hit.file}:${hit.startLine}:${hit.name}`}
                    value={`${hit.file}:${hit.startLine}:${hit.name}`}
                    onSelect={() => select(hit)}
                  >
                    <FileCode2 className="size-4" />
                    <span className="font-mono text-sm">{hit.name}</span>
                    <span className="text-[10px] uppercase text-muted-foreground">
                      {hit.kind}
                    </span>
                    <span className="ml-auto min-w-0 truncate text-xs text-muted-foreground">
                      {hit.file}:{hit.startLine}
                    </span>
                  </CommandItem>
                ))}
              </CommandGroup>
            )}
          </CommandList>
        </Command>
      </DialogContent>
    </Dialog>
  );
}
