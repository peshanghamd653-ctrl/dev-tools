import { useCallback, useRef, useState } from "react";
import {
  useMutation,
  useQuery,
  useQueryClient,
} from "@tanstack/react-query";
import { Channel } from "@tauri-apps/api/core";

import { inDesktopShell, ipc, type AiDelta } from "@/shared/ipc/client";

export const aiKeys = {
  conversations: ["ai", "conversations"] as const,
  messages: (id: string) => ["ai", "messages", id] as const,
  secrets: ["secrets"] as const,
  ollamaModels: ["ai", "ollama-models"] as const,
};

export function useConversations() {
  return useQuery({
    queryKey: aiKeys.conversations,
    queryFn: ipc.aiConversationsList,
    enabled: inDesktopShell,
  });
}

export function useMessages(conversationId: string | null) {
  return useQuery({
    queryKey: aiKeys.messages(conversationId ?? "none"),
    queryFn: () => ipc.aiMessages(conversationId ?? ""),
    enabled: inDesktopShell && Boolean(conversationId),
  });
}

export function useSecretNames() {
  return useQuery({
    queryKey: aiKeys.secrets,
    queryFn: ipc.secretList,
    enabled: inDesktopShell,
  });
}

export function useOllamaModels(enabled: boolean) {
  return useQuery({
    queryKey: aiKeys.ollamaModels,
    queryFn: ipc.aiOllamaModels,
    enabled: inDesktopShell && enabled,
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
export function useSendMessage(
  conversationId: string | null,
  projectPath: string | null,
  toolsEnabled: boolean,
) {
  const queryClient = useQueryClient();
  const [streamText, setStreamText] = useState<string | null>(null);
  const [toolEvents, setToolEvents] = useState<ToolActivity[]>([]);
  const streamRef = useRef("");

  const mutation = useMutation({
    mutationFn: async (content: string) => {
      if (!conversationId) throw new Error("no conversation selected");
      streamRef.current = "";
      setStreamText("");
      setToolEvents([]);
      const channel = new Channel<AiDelta>();
      channel.onmessage = (delta) => {
        if (delta.kind === "text") {
          streamRef.current += delta.data.text;
          setStreamText(streamRef.current);
        } else if (delta.kind === "toolCall") {
          const { id, name, input } = delta.data;
          setToolEvents((events) => [...events, { id, name, input }]);
        } else if (delta.kind === "toolResult") {
          const { id, ok, summary } = delta.data;
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
        channel,
      );
    },
    onSettled: () => {
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

  const reset = useCallback(() => {
    setStreamText(null);
    setToolEvents([]);
  }, []);
  return { ...mutation, streamText, toolEvents, resetStream: reset };
}
