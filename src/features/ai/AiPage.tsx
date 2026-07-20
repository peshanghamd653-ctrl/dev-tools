import { useEffect, useRef, useState } from "react";
import { Check, ChevronsUpDown, KeyRound, Plus, Send, Sparkles, Trash2 } from "lucide-react";
import Markdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { toast } from "sonner";

import { useGitStore } from "@/features/git/store";
import { useProjects } from "@/features/projects/hooks";
import { useActiveWorkspace } from "@/features/workspaces/hooks";
import { inDesktopShell, ipc } from "@/shared/ipc/client";
import { cn } from "@/shared/lib/utils";
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
import {
  aiKeys,
  useConversations,
  useMessages,
  useOllamaModels,
  useSecretNames,
  useSendMessage,
} from "./hooks";
import { CLAUDE_MODELS, useAiStore } from "./store";
import { useQueryClient } from "@tanstack/react-query";

export function AiPage() {
  const { data: conversations } = useConversations();
  const activeId = useAiStore((s) => s.activeConversationId);
  const setActiveConversation = useAiStore((s) => s.setActiveConversation);
  const provider = useAiStore((s) => s.provider);
  const model = useAiStore((s) => s.model);
  const setProvider = useAiStore((s) => s.setProvider);
  const setModel = useAiStore((s) => s.setModel);

  const queryClient = useQueryClient();
  const { data: secretNames } = useSecretNames();
  const hasClaudeKey = secretNames?.includes("anthropic-api-key") ?? false;
  const { data: ollamaModels } = useOllamaModels(provider === "ollama");

  const attachProject = useAiStore((s) => s.attachProject);
  const setAttachProject = useAiStore((s) => s.setAttachProject);
  const activeWorkspace = useActiveWorkspace();
  const { data: projects } = useProjects(activeWorkspace?.id);
  const selectedProjectId = useGitStore((s) => s.selectedProjectId);
  const project =
    projects?.find((p) => p.id === selectedProjectId) ?? projects?.[0] ?? null;

  const active =
    conversations?.find((c) => c.id === activeId) ?? conversations?.[0] ?? null;
  const { data: messages } = useMessages(active?.id ?? null);
  const send = useSendMessage(
    active?.id ?? null,
    attachProject && project ? project.path : null,
  );

  const [draft, setDraft] = useState("");
  const scrollRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    scrollRef.current?.scrollTo({ top: scrollRef.current.scrollHeight });
  }, [messages, send.streamText]);

  if (!inDesktopShell) {
    return (
      <div className="mx-auto max-w-3xl p-6">
        <Card>
          <CardContent className="flex flex-col items-center gap-2 py-12 text-center">
            <Sparkles className="size-6 text-muted-foreground" />
            <p className="font-medium">The AI assistant needs the desktop shell</p>
          </CardContent>
        </Card>
      </div>
    );
  }

  async function newConversation() {
    try {
      const conversation = await ipc.aiConversationCreate(provider, model);
      await queryClient.invalidateQueries({ queryKey: aiKeys.conversations });
      setActiveConversation(conversation.id);
    } catch (error) {
      toast.error(String(error));
    }
  }

  function submit() {
    const content = draft.trim();
    if (!content || send.isPending || !active) return;
    setDraft("");
    send.mutate(content, {
      onError: (error) => toast.error(String(error)),
    });
  }

  return (
    <div className="grid h-full grid-cols-[280px_1fr]">
      <aside className="flex min-h-0 flex-col border-r">
        <div className="flex h-11 shrink-0 items-center gap-1 border-b px-2">
          <Button size="sm" className="flex-1 gap-1.5" onClick={newConversation}>
            <Plus className="size-4" />
            New chat
          </Button>
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button variant="outline" size="sm" className="gap-1 px-2 text-xs">
                {provider === "claude" ? "Claude" : "Ollama"}
                <ChevronsUpDown className="size-3 text-muted-foreground" />
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end" className="w-56">
              <DropdownMenuLabel>Claude</DropdownMenuLabel>
              {CLAUDE_MODELS.map((m) => (
                <DropdownMenuItem
                  key={m}
                  onSelect={() => setProvider("claude", m)}
                >
                  <span className="font-mono text-xs">{m}</span>
                  {provider === "claude" && model === m && (
                    <Check className="ml-auto size-4" />
                  )}
                </DropdownMenuItem>
              ))}
              <DropdownMenuSeparator />
              <DropdownMenuLabel>Ollama (local)</DropdownMenuLabel>
              {ollamaModels?.length ? (
                ollamaModels.map((m) => (
                  <DropdownMenuItem
                    key={m}
                    onSelect={() => setProvider("ollama", m)}
                  >
                    <span className="font-mono text-xs">{m}</span>
                    {provider === "ollama" && model === m && (
                      <Check className="ml-auto size-4" />
                    )}
                  </DropdownMenuItem>
                ))
              ) : (
                <DropdownMenuItem
                  disabled
                  onSelect={() => setModel(model)}
                >
                  <span className="text-xs text-muted-foreground">
                    {provider === "ollama"
                      ? "No local models found"
                      : "Select to load local models"}
                  </span>
                </DropdownMenuItem>
              )}
            </DropdownMenuContent>
          </DropdownMenu>
        </div>

        <div className="min-h-0 flex-1 overflow-y-auto p-2">
          {conversations?.length === 0 && (
            <p className="px-2 py-6 text-center text-xs text-muted-foreground">
              No conversations yet.
            </p>
          )}
          <ul className="space-y-0.5">
            {conversations?.map((c) => (
              <li
                key={c.id}
                className={cn(
                  "group flex cursor-pointer items-center gap-2 rounded-md px-2 py-1.5",
                  c.id === active?.id ? "bg-accent" : "hover:bg-accent/60",
                )}
                onClick={() => setActiveConversation(c.id)}
              >
                <div className="min-w-0 flex-1">
                  <p className="truncate text-xs">{c.title}</p>
                  <p className="text-[10px] text-muted-foreground">
                    {c.provider} · {c.model}
                  </p>
                </div>
                <Button
                  variant="ghost"
                  size="icon"
                  className="size-5 opacity-0 group-hover:opacity-70 hover:!opacity-100"
                  aria-label={`Delete ${c.title}`}
                  onClick={(e) => {
                    e.stopPropagation();
                    void ipc
                      .aiConversationDelete(c.id)
                      .then(() =>
                        queryClient.invalidateQueries({
                          queryKey: aiKeys.conversations,
                        }),
                      )
                      .then(() => {
                        if (c.id === active?.id) setActiveConversation(null);
                      })
                      .catch((error) => toast.error(String(error)));
                  }}
                >
                  <Trash2 className="size-3" />
                </Button>
              </li>
            ))}
          </ul>
        </div>
      </aside>

      <section className="flex min-h-0 flex-col">
        {active?.provider === "claude" && !hasClaudeKey ? (
          <ClaudeKeySetup />
        ) : !active ? (
          <div className="flex flex-1 flex-col items-center justify-center gap-3 text-center">
            <Sparkles className="size-6 text-muted-foreground" />
            <p className="text-sm text-muted-foreground">
              Start a conversation — it runs on{" "}
              <span className="text-foreground">
                {provider === "claude" ? "Claude" : "Ollama"} · {model}
              </span>
            </p>
            <Button size="sm" onClick={newConversation}>
              <Plus className="size-4" />
              New chat
            </Button>
          </div>
        ) : (
          <>
            <div
              ref={scrollRef}
              className="min-h-0 flex-1 overflow-y-auto px-6 py-4"
            >
              <div className="mx-auto max-w-3xl space-y-4">
                {messages?.map((m) => (
                  <MessageBubble key={m.id} role={m.role} content={m.content} />
                ))}
                {send.streamText !== null && (
                  <MessageBubble
                    role="assistant"
                    content={send.streamText || "…"}
                    streaming
                  />
                )}
              </div>
            </div>

            <div className="shrink-0 border-t p-3">
              {project && (
                <div className="mx-auto max-w-3xl pb-2">
                  <button
                    type="button"
                    onClick={() => setAttachProject(!attachProject)}
                    className={cn(
                      "inline-flex items-center gap-1.5 rounded-full border px-2.5 py-0.5 text-[11px] transition-colors",
                      attachProject
                        ? "border-primary/40 bg-primary/10 text-foreground"
                        : "border-border text-muted-foreground line-through",
                    )}
                    title={
                      attachProject
                        ? "Project context attached — click to detach"
                        : "Project context detached — click to attach"
                    }
                  >
                    <span
                      className={cn(
                        "size-1.5 rounded-full",
                        attachProject ? "bg-primary" : "bg-muted-foreground",
                      )}
                    />
                    {project.name}
                  </button>
                </div>
              )}
              <div className="mx-auto flex max-w-3xl items-end gap-2">
                <textarea
                  value={draft}
                  onChange={(e) => setDraft(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter" && !e.shiftKey) {
                      e.preventDefault();
                      submit();
                    }
                  }}
                  placeholder={`Message ${active.model}…  (Enter to send)`}
                  rows={2}
                  className="flex-1 resize-none rounded-lg border bg-transparent px-3 py-2 text-sm outline-none placeholder:text-muted-foreground focus-visible:border-ring"
                />
                <Button
                  size="icon"
                  disabled={!draft.trim() || send.isPending}
                  onClick={submit}
                  aria-label="Send message"
                >
                  <Send className="size-4" />
                </Button>
              </div>
            </div>
          </>
        )}
      </section>
    </div>
  );
}

function MessageBubble({
  role,
  content,
  streaming,
}: {
  role: string;
  content: string;
  streaming?: boolean;
}) {
  if (role === "user") {
    return (
      <div className="flex justify-end">
        <div className="max-w-[80%] whitespace-pre-wrap rounded-2xl rounded-br-sm bg-primary/15 px-4 py-2 text-sm">
          {content}
        </div>
      </div>
    );
  }
  return (
    <div className="flex">
      <div
        className={cn(
          "prose prose-sm prose-invert max-w-[85%] rounded-2xl rounded-bl-sm bg-card px-4 py-2",
          "prose-pre:bg-muted prose-pre:text-xs prose-code:before:content-none prose-code:after:content-none",
          streaming && "animate-pulse-subtle",
        )}
      >
        <Markdown remarkPlugins={[remarkGfm]}>{content}</Markdown>
      </div>
    </div>
  );
}

function ClaudeKeySetup() {
  const [key, setKey] = useState("");
  const queryClient = useQueryClient();

  return (
    <div className="flex flex-1 items-center justify-center p-6">
      <Card className="w-full max-w-md">
        <CardContent className="space-y-4 py-6">
          <div className="flex items-center gap-2">
            <KeyRound className="size-4 text-muted-foreground" />
            <p className="font-medium">Connect your Anthropic API key</p>
          </div>
          <p className="text-sm text-muted-foreground">
            The key is encrypted with a master key held in Windows Credential
            Manager and never leaves this machine except to call the API.
          </p>
          <Input
            type="password"
            value={key}
            onChange={(e) => setKey(e.target.value)}
            placeholder="sk-ant-…"
            className="font-mono text-xs"
          />
          <Button
            className="w-full"
            disabled={!key.trim()}
            onClick={() => {
              void ipc
                .secretSet("anthropic-api-key", key.trim())
                .then(() =>
                  queryClient.invalidateQueries({ queryKey: aiKeys.secrets }),
                )
                .then(() => toast.success("API key saved"))
                .catch((error) => toast.error(String(error)));
            }}
          >
            Save key
          </Button>
        </CardContent>
      </Card>
    </div>
  );
}
