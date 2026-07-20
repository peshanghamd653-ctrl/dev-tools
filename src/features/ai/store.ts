import { create } from "zustand";
import { persist } from "zustand/middleware";

interface AiState {
  activeConversationId: string | null;
  /** Provider/model used when creating the next conversation. */
  provider: "claude" | "ollama";
  model: string;
  /** Inject the active project's git summary into the system prompt. */
  attachProject: boolean;
  /**
   * The tools grant: lets the model read files in the attached project
   * (read-only). Off by default — turning it on is the approval act.
   */
  toolsEnabled: boolean;
  setActiveConversation: (id: string | null) => void;
  setProvider: (provider: "claude" | "ollama", model: string) => void;
  setModel: (model: string) => void;
  setAttachProject: (attach: boolean) => void;
  setToolsEnabled: (enabled: boolean) => void;
}

export const useAiStore = create<AiState>()(
  persist(
    (set) => ({
      activeConversationId: null,
      provider: "claude",
      model: "claude-sonnet-5",
      attachProject: true,
      toolsEnabled: false,
      setActiveConversation: (id) => set({ activeConversationId: id }),
      setProvider: (provider, model) => set({ provider, model }),
      setModel: (model) => set({ model }),
      setAttachProject: (attach) => set({ attachProject: attach }),
      setToolsEnabled: (enabled) => set({ toolsEnabled: enabled }),
    }),
    { name: "devos-ai" },
  ),
);

export const CLAUDE_MODELS = [
  "claude-sonnet-5",
  "claude-opus-4-8",
  "claude-haiku-4-5",
];
