import { useEffect, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import {
  Camera,
  FileCode2,
  FolderPlus,
  MonitorCog,
  Plus,
  Sparkles,
} from "lucide-react";
import { toast } from "sonner";

import { primaryNav } from "@/app/nav";
import { useAiStore } from "@/features/ai/store";
import { useModuleCommands } from "@/features/app/hooks";
import { useGitStore } from "@/features/git/store";
import { useProjects } from "@/features/projects/hooks";
import { useActiveWorkspace } from "@/features/workspaces/hooks";
import { ipc } from "@/shared/ipc/client";
import { useDialogStore } from "@/shared/stores/dialogs";
import { useUiStore } from "@/shared/stores/ui";
import {
  CommandDialog,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
  CommandSeparator,
  CommandShortcut,
} from "@/shared/ui/command";

/**
 * Presets that turn one palette entry into a ready-made prompt for the AI
 * assistant, rather than a raw pass-through of whatever the user typed.
 * "Deploy production" is deliberately absent — the AI has no deploy tool
 * (ADR-0009: deployments are read-only), and offering a shortcut for
 * something it cannot do would be a false affordance.
 */
const AI_PRESETS: { label: string; keywords: string[]; prompt: string }[] = [
  {
    label: "Run tests",
    keywords: ["test", "run", "suite", "pytest", "jest", "cargo"],
    prompt: "Run the test suite and report the results.",
  },
  {
    label: "Run tests and fix failures",
    keywords: ["test", "fix", "failing", "repair"],
    prompt:
      "Run the test suite. If anything fails, find the cause, fix it, and re-run until it passes.",
  },
  {
    label: "Find TODO comments",
    keywords: ["todo", "fixme", "find"],
    prompt:
      "Find every TODO comment in this project and list them with their file and line number.",
  },
  {
    label: "Explain the last error",
    keywords: ["error", "explain", "debug", "diagnose"],
    prompt:
      "Look at the most recent error or failing output you can find in this project (build, test, or lint) and explain what's wrong and how to fix it.",
  },
];

/** Module command ids the shell knows how to execute today. */
const moduleCommandHandlers: Record<
  string,
  | "createWorkspace"
  | "addProject"
  | "newTerminal"
  | "openGit"
  | "openAi"
  | "indexProject"
  | "openDocker"
  | "openApi"
  | "openDatabase"
  | "openMonitors"
  | "openDeploy"
  | "openSnippets"
  | "openMcp"
  | "openSecurity"
  | "captureIssue"
> = {
  "core.workspace.create": "createWorkspace",
  "core.project.add": "addProject",
  "terminal.new": "newTerminal",
  "git.open": "openGit",
  "ai.open": "openAi",
  "index.project": "indexProject",
  "docker.open": "openDocker",
  "api.open": "openApi",
  "db.open": "openDatabase",
  "monitor.open": "openMonitors",
  "deploy.open": "openDeploy",
  "snippets.open": "openSnippets",
  "mcp.open": "openMcp",
  "security.open": "openSecurity",
  "issue.capture": "captureIssue",
};

export function CommandPalette() {
  const open = useUiStore((s) => s.paletteOpen);
  const setOpen = useUiStore((s) => s.setPaletteOpen);
  const setSymbolSearchOpen = useUiStore((s) => s.setSymbolSearchOpen);
  const setCreateWorkspaceOpen = useDialogStore(
    (s) => s.setCreateWorkspaceOpen,
  );
  const setAddProjectOpen = useDialogStore((s) => s.setAddProjectOpen);
  const setCaptureIssueOpen = useDialogStore((s) => s.setCaptureIssueOpen);
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const { data: moduleCommands } = useModuleCommands();
  const activeWorkspace = useActiveWorkspace();
  const { data: projects } = useProjects(activeWorkspace?.id);
  const selectedProjectId = useGitStore((s) => s.selectedProjectId);
  const paletteProject =
    projects?.find((p) => p.id === selectedProjectId) ?? projects?.[0] ?? null;
  const [query, setQuery] = useState("");

  // Covers every way the palette closes, not just the ones that go through
  // this component: `runAndClose` below calls the store setter directly,
  // and Ctrl+K itself toggles `paletteOpen` from the *global* hotkey handler
  // (`useGlobalHotkeys`), entirely outside `CommandDialog`'s own
  // `onOpenChange` (which only fires for Radix's own dismiss triggers —
  // Escape, a backdrop click). Watching `open` itself, rather than trying to
  // hook every closer, is what actually catches all three — a reopened
  // palette never starts mid-query or showing a stale "Ask AI: …" item for
  // a question already asked.
  useEffect(() => {
    if (!open) setQuery("");
  }, [open]);

  function runAndClose(action: () => void) {
    setOpen(false);
    action();
  }

  /**
   * The "natural language command" half of the brief: a query with no
   * matching static command still does something, because it becomes a
   * prompt for the AI assistant — which has real read/write tools
   * (`run_command`, `search_code`, `run_tests`, git operations) and can
   * often just do what was asked, rather than only searching for it. Same
   * queue-a-conversation flow `TerminalPage`'s "diagnose this" already
   * uses: create a conversation, stash the prompt, navigate, and `AiPage`
   * sends it once that conversation is actually active.
   */
  async function askAi(prompt: string) {
    try {
      const { provider, model, setActiveConversation, setPendingPrompt } =
        useAiStore.getState();
      const conversation = await ipc.aiConversationCreate(provider, model);
      await queryClient.invalidateQueries({
        queryKey: ["ai", "conversations"],
      });
      setActiveConversation(conversation.id);
      setPendingPrompt(prompt);
      void navigate({ to: "/ai" });
    } catch (error) {
      toast.error(String(error));
    }
  }

  function runModuleCommand(id: string) {
    const handler = moduleCommandHandlers[id];
    if (handler === "createWorkspace") setCreateWorkspaceOpen(true);
    else if (handler === "addProject") setAddProjectOpen(true);
    else if (handler === "openGit") void navigate({ to: "/git" });
    else if (handler === "openAi") void navigate({ to: "/ai" });
    else if (handler === "openDocker") void navigate({ to: "/docker" });
    else if (handler === "openApi") void navigate({ to: "/api" });
    else if (handler === "openDatabase") void navigate({ to: "/database" });
    else if (handler === "openMonitors") void navigate({ to: "/monitors" });
    else if (handler === "openDeploy") void navigate({ to: "/deploy" });
    else if (handler === "openSnippets") void navigate({ to: "/snippets" });
    else if (handler === "openMcp") void navigate({ to: "/mcp" });
    else if (handler === "openSecurity") void navigate({ to: "/security" });
    else if (handler === "captureIssue") setCaptureIssueOpen(true);
    else if (handler === "indexProject") {
      if (!paletteProject) {
        toast.error("Add a project first");
      } else {
        void ipc
          .indexProject(paletteProject.path)
          .then(() => toast.info(`Indexing "${paletteProject.name}"…`))
          .catch((error) => toast.error(String(error)));
      }
    } else if (handler === "newTerminal") {
      void navigate({ to: "/terminal" });
      void import("@/features/terminal/session").then((m) =>
        m
          .createTerminalSession()
          .catch((error) => toast.error(`Could not start terminal: ${error}`)),
      );
    } else toast.info(`Command "${id}" is not wired up yet`);
  }

  return (
    <CommandDialog
      open={open}
      onOpenChange={setOpen}
      title="Command palette"
      description="Search pages and run commands, or ask in plain language"
    >
      <CommandInput
        value={query}
        onValueChange={setQuery}
        placeholder="Search, or ask in plain language…"
      />
      <CommandList>
        <CommandEmpty>No results found.</CommandEmpty>

        <CommandGroup heading="Navigation">
          {primaryNav.map((item) => (
            <CommandItem
              key={item.to}
              onSelect={() => runAndClose(() => void navigate({ to: item.to }))}
            >
              <item.icon className="size-4" />
              {item.label}
              {item.shortcut && (
                <CommandShortcut>{item.shortcut}</CommandShortcut>
              )}
            </CommandItem>
          ))}
        </CommandGroup>

        <CommandSeparator />

        <CommandGroup heading="Actions">
          <CommandItem
            keywords={["workspace", "new"]}
            onSelect={() => runAndClose(() => setCreateWorkspaceOpen(true))}
          >
            <Plus className="size-4" />
            Create workspace
          </CommandItem>
          <CommandItem
            keywords={["project", "add", "open"]}
            onSelect={() => runAndClose(() => setAddProjectOpen(true))}
          >
            <FolderPlus className="size-4" />
            Add project
          </CommandItem>
          <CommandItem
            keywords={["issue", "bug", "screenshot", "capture", "github"]}
            onSelect={() => runAndClose(() => setCaptureIssueOpen(true))}
          >
            <Camera className="size-4" />
            Report a bug with a screenshot
            <CommandShortcut>Ctrl+Shift+S</CommandShortcut>
          </CommandItem>
          <CommandItem
            keywords={["symbol", "function", "class", "definition", "goto"]}
            onSelect={() => runAndClose(() => setSymbolSearchOpen(true))}
          >
            <FileCode2 className="size-4" />
            Go to symbol
            <CommandShortcut>Ctrl+T</CommandShortcut>
          </CommandItem>
        </CommandGroup>

        <CommandSeparator />

        <CommandGroup heading="Ask AI">
          {AI_PRESETS.map((preset) => (
            <CommandItem
              key={preset.label}
              keywords={preset.keywords}
              onSelect={() => runAndClose(() => void askAi(preset.prompt))}
            >
              <Sparkles className="size-4" />
              {preset.label}
            </CommandItem>
          ))}
          {query.trim() && (
            <CommandItem
              value={query}
              onSelect={() => runAndClose(() => void askAi(query.trim()))}
            >
              <Sparkles className="size-4" />
              Ask AI: &ldquo;{query.trim()}&rdquo;
            </CommandItem>
          )}
        </CommandGroup>

        {moduleCommands && moduleCommands.length > 0 && (
          <>
            <CommandSeparator />
            <CommandGroup heading="Modules">
              {moduleCommands.map((command) => (
                <CommandItem
                  key={command.id}
                  keywords={command.keywords}
                  onSelect={() =>
                    runAndClose(() => runModuleCommand(command.id))
                  }
                >
                  <MonitorCog className="size-4" />
                  {command.title}
                  <span className="ml-auto text-[10px] text-muted-foreground uppercase">
                    {command.module}
                  </span>
                </CommandItem>
              ))}
            </CommandGroup>
          </>
        )}
      </CommandList>
    </CommandDialog>
  );
}
