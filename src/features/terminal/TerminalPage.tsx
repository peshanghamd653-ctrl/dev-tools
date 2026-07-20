import { useEffect, useRef } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import { Columns2, Plus, Sparkles, Terminal, Unplug, X } from "lucide-react";
import { toast } from "sonner";

import { useAiStore } from "@/features/ai/store";
import { inDesktopShell, ipc } from "@/shared/ipc/client";
import { cn } from "@/shared/lib/utils";
import { Button } from "@/shared/ui/button";
import { Card, CardContent } from "@/shared/ui/card";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/shared/ui/tooltip";
import {
  closeTerminalSession,
  createTerminalSession,
  reconcileSessions,
} from "./session";
import { useTerminalStore } from "./store";
import { TerminalView } from "./TerminalView";

export function TerminalPage() {
  const sessions = useTerminalStore((s) => s.sessions);
  const activeId = useTerminalStore((s) => s.activeId);
  const splitId = useTerminalStore((s) => s.splitId);
  const setActive = useTerminalStore((s) => s.setActive);
  const toggleSplit = useTerminalStore((s) => s.toggleSplit);
  const bootstrapped = useRef(false);
  const navigate = useNavigate();
  const queryClient = useQueryClient();

  async function diagnoseActive() {
    if (!activeId) return;
    try {
      const tail = await ipc.termTail(activeId);
      if (!tail.trim()) {
        toast.info("No terminal output to diagnose yet");
        return;
      }
      const { provider, model, setActiveConversation, setPendingPrompt } =
        useAiStore.getState();
      const conversation = await ipc.aiConversationCreate(provider, model);
      await queryClient.invalidateQueries({
        queryKey: ["ai", "conversations"],
      });
      setActiveConversation(conversation.id);
      setPendingPrompt(
        "Diagnose the problem in this terminal output and propose a concrete fix:\n\n```\n" +
          tail.slice(-6000) +
          "\n```",
      );
      void navigate({ to: "/ai" });
    } catch (error) {
      toast.error(String(error));
    }
  }

  useEffect(() => {
    if (!inDesktopShell || bootstrapped.current) return;
    bootstrapped.current = true;
    void (async () => {
      await reconcileSessions();
      const { sessions } = useTerminalStore.getState();
      if (!sessions.some((s) => s.status === "live")) {
        await createTerminalSession().catch((error) =>
          toast.error(`Could not start terminal: ${error}`),
        );
      }
    })();
  }, []);

  if (!inDesktopShell) {
    return (
      <div className="mx-auto max-w-3xl p-6">
        <Card>
          <CardContent className="flex flex-col items-center gap-2 py-12 text-center">
            <Terminal className="size-6 text-muted-foreground" />
            <p className="font-medium">Terminals need the desktop shell</p>
            <p className="text-sm text-muted-foreground">
              This page is running in a plain browser tab, so no pty backend is
              available.
            </p>
          </CardContent>
        </Card>
      </div>
    );
  }

  const active = sessions.find((s) => s.info.id === activeId);
  const split = sessions.find((s) => s.info.id === splitId);

  return (
    <div className="flex h-full flex-col">
      <div className="flex h-9 shrink-0 items-center gap-1 border-b px-2">
        {sessions.map((session) => (
          <button
            key={session.info.id}
            type="button"
            onClick={() => setActive(session.info.id)}
            className={cn(
              "group flex h-6 items-center gap-1.5 rounded px-2 text-xs",
              session.info.id === activeId
                ? "bg-accent text-foreground"
                : "text-muted-foreground hover:bg-accent/60",
            )}
          >
            {session.status === "disconnected" ? (
              <Unplug className="size-3 text-yellow-500/80" />
            ) : (
              <span
                className={cn(
                  "size-1.5 rounded-full",
                  session.status === "live" ? "bg-emerald-500" : "bg-zinc-500",
                )}
              />
            )}
            {session.info.title}
            <X
              className="size-3 opacity-0 transition-opacity group-hover:opacity-70 hover:!opacity-100"
              onClick={(e) => {
                e.stopPropagation();
                void closeTerminalSession(session.info.id);
              }}
            />
          </button>
        ))}

        <Tooltip>
          <TooltipTrigger asChild>
            <Button
              variant="ghost"
              size="icon"
              className="size-6"
              aria-label="New terminal"
              onClick={() =>
                void createTerminalSession().catch((error) =>
                  toast.error(`Could not start terminal: ${error}`),
                )
              }
            >
              <Plus className="size-3.5" />
            </Button>
          </TooltipTrigger>
          <TooltipContent>New terminal</TooltipContent>
        </Tooltip>

        <div className="flex-1" />

        <Tooltip>
          <TooltipTrigger asChild>
            <Button
              variant="ghost"
              size="icon"
              className="size-6"
              aria-label="Diagnose with AI"
              disabled={!active || active.status === "disconnected"}
              onClick={() => void diagnoseActive()}
            >
              <Sparkles className="size-3.5" />
            </Button>
          </TooltipTrigger>
          <TooltipContent>
            Diagnose recent output with AI
          </TooltipContent>
        </Tooltip>

        <Tooltip>
          <TooltipTrigger asChild>
            <Button
              variant="ghost"
              size="icon"
              className={cn("size-6", splitId && "text-primary")}
              aria-label="Toggle split view"
              disabled={sessions.filter((s) => s.status === "live").length < 2}
              onClick={toggleSplit}
            >
              <Columns2 className="size-3.5" />
            </Button>
          </TooltipTrigger>
          <TooltipContent>Split view (needs 2 terminals)</TooltipContent>
        </Tooltip>
      </div>

      <div
        className={cn(
          "grid min-h-0 flex-1",
          split ? "grid-cols-2 divide-x divide-border" : "grid-cols-1",
        )}
      >
        {active ? (
          <SessionPane
            sessionId={active.info.id}
            disconnected={active.status === "disconnected"}
          />
        ) : (
          <div className="flex items-center justify-center text-sm text-muted-foreground">
            No terminal — press
            <kbd className="mx-1.5 rounded bg-muted px-1.5 py-0.5 text-[10px]">
              +
            </kbd>
            to start one.
          </div>
        )}
        {split && (
          <SessionPane
            sessionId={split.info.id}
            disconnected={split.status === "disconnected"}
          />
        )}
      </div>
    </div>
  );
}

function SessionPane({
  sessionId,
  disconnected,
}: {
  sessionId: string;
  disconnected: boolean;
}) {
  if (disconnected) {
    return (
      <div className="flex flex-col items-center justify-center gap-2 text-center">
        <Unplug className="size-5 text-yellow-500/80" />
        <p className="text-sm">This session predates the current window.</p>
        <p className="text-xs text-muted-foreground">
          Its output stream was attached to a previous page load.
        </p>
        <Button
          variant="outline"
          size="sm"
          onClick={() => void closeTerminalSession(sessionId)}
        >
          Close session
        </Button>
      </div>
    );
  }
  return <TerminalView sessionId={sessionId} />;
}
