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

/**
 * Streaming send. `streamText` grows as deltas arrive; once the backend
 * confirms persistence the messages query is refreshed and the stream resets.
 */
export function useSendMessage(
  conversationId: string | null,
  projectPath: string | null,
) {
  const queryClient = useQueryClient();
  const [streamText, setStreamText] = useState<string | null>(null);
  const streamRef = useRef("");

  const mutation = useMutation({
    mutationFn: async (content: string) => {
      if (!conversationId) throw new Error("no conversation selected");
      streamRef.current = "";
      setStreamText("");
      const channel = new Channel<AiDelta>();
      channel.onmessage = (delta) => {
        if (delta.kind === "text") {
          streamRef.current += delta.data.text;
          setStreamText(streamRef.current);
        }
      };
      return ipc.aiSend(conversationId, content, projectPath, channel);
    },
    onSettled: () => {
      if (conversationId) {
        void queryClient
          .invalidateQueries({ queryKey: aiKeys.messages(conversationId) })
          .then(() => setStreamText(null));
      } else {
        setStreamText(null);
      }
      void queryClient.invalidateQueries({ queryKey: aiKeys.conversations });
    },
  });

  const reset = useCallback(() => setStreamText(null), []);
  return { ...mutation, streamText, resetStream: reset };
}
