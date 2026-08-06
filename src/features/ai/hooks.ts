import { useCallback, useRef, useState } from "react";
import {
  useMutation,
  useQuery,
  useQueryClient,
} from "@tanstack/react-query";
import { Channel } from "@tauri-apps/api/core";

import { isDesktopShell, ipc, type AiDelta } from "@/shared/ipc/client";

export const aiKeys = {
  conversations: ["ai", "conversations"] as const,
  messages: (id: string) => ["ai", "messages", id] as const,
  secrets: ["secrets"] as const,
  ollamaModels: ["ai", "ollama-models"] as const,
  memory: (projectPath: string) => ["ai", "memory", projectPath] as const,
};

export function useConversations() {
  return useQuery({
    queryKey: aiKeys.conversations,
    queryFn: ipc.aiConversationsList,
    enabled: isDesktopShell(),
  });
}

export function useMessages(conversationId: string | null) {
  return useQuery({
    queryKey: aiKeys.messages(conversationId ?? "none"),
    queryFn: () => ipc.aiMessages(conversationId ?? ""),
    enabled: isDesktopShell() && Boolean(conversationId),
  });
}

export function useSecretNames() {
  return useQuery({
    queryKey: aiKeys.secrets,
    queryFn: ipc.secretList,
    enabled: isDesktopShell(),
  });
}

export function useMemoryEntries(projectPath: string | null) {
  return useQuery({
    queryKey: aiKeys.memory(projectPath ?? "none"),
    queryFn: () => ipc.aiMemoryList(projectPath ?? ""),
    enabled: isDesktopShell() && Boolean(projectPath),
  });
}

export function useOllamaModels(enabled: boolean) {
  return useQuery({
    queryKey: aiKeys.ollamaModels,
    queryFn: ipc.aiOllamaModels,
    enabled: isDesktopShell() && enabled,
    staleTime: 30_000,
    retry: false,
  });
}

export interface ToolActivity {
  id: string;
  name: string;
  input: string;
  /** undefined while running */
  ok?: boolean;
  summary?: string;
}

/**
 * Streaming send. `streamText` grows as deltas arrive and `toolEvents`
 * records live tool activity; once the backend confirms persistence the
 * messages query is refreshed and the stream resets.
 */
export interface PendingApproval {
  id: string;
  name: string;
  input: string;
}

export function useSendMessage(
  conversationId: string | null,
  projectPath: string | null,
  toolsEnabled: boolean,
  writeToolsEnabled: boolean,
) {
  const queryClient = useQueryClient();
  const [streamText, setStreamText] = useState<string | null>(null);
  const [toolEvents, setToolEvents] = useState<ToolActivity[]>([]);
  const [pendingApproval, setPendingApproval] =
    useState<PendingApproval | null>(null);
  const streamRef = useRef("");

  const mutation = useMutation({
    mutationFn: async (content: string) => {
      if (!conversationId) throw new Error("no conversation selected");
      streamRef.current = "";
      setStreamText("");
      setToolEvents([]);
      setPendingApproval(null);
      const channel = new Channel<AiDelta>();
      channel.onmessage = (delta) => {
        if (delta.kind === "text") {
          streamRef.current += delta.data.text;
          setStreamText(streamRef.current);
        } else if (delta.kind === "toolCall") {
          const { id, name, input } = delta.data;
          setToolEvents((events) => [...events, { id, name, input }]);
        } else if (delta.kind === "approvalRequest") {
          setPendingApproval(delta.data);
        } else if (delta.kind === "toolResult") {
          const { id, ok, summary } = delta.data;
          setPendingApproval(null);
          setToolEvents((events) =>
            events.map((e) => (e.id === id ? { ...e, ok, summary } : e)),
          );
        }
      };
      return ipc.aiSend(
        conversationId,
        content,
        projectPath,
        toolsEnabled,
        writeToolsEnabled,
        channel,
      );
    },
    onSettled: () => {
      setPendingApproval(null);
      if (conversationId) {
        void queryClient
          .invalidateQueries({ queryKey: aiKeys.messages(conversationId) })
          .then(() => {
            setStreamText(null);
            setToolEvents([]);
          });
      } else {
        setStreamText(null);
        setToolEvents([]);
      }
      void queryClient.invalidateQueries({ queryKey: aiKeys.conversations });
    },
  });

  const respondToApproval = useCallback((id: string, approved: boolean) => {
    setPendingApproval((current) => (current?.id === id ? null : current));
    void ipc.aiToolRespond(id, approved);
  }, []);

  const reset = useCallback(() => {
    setStreamText(null);
    setToolEvents([]);
    setPendingApproval(null);
  }, []);
  return {
    ...mutation,
    streamText,
    toolEvents,
    pendingApproval,
    respondToApproval,
    resetStream: reset,
  };
}
