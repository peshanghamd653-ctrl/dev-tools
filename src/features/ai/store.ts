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
  /**
   * Second grant level: edit_file/write_file/run_command exist for the
   * model. Every individual call still requires explicit approval in the
   * chat (ADR-0005). Off by default.
   */
  writeToolsEnabled: boolean;
  setActiveConversation: (id: string | null) => void;
  setProvider: (provider: "claude" | "ollama", model: string) => void;
  setModel: (model: string) => void;
  setAttachProject: (attach: boolean) => void;
  setToolsEnabled: (enabled: boolean) => void;
  setWriteToolsEnabled: (enabled: boolean) => void;
}

export const useAiStore = create<AiState>()(
  persist(
    (set) => ({
      activeConversationId: null,
      provider: "claude",
      model: "claude-sonnet-5",
      attachProject: true,
      toolsEnabled: false,
      writeToolsEnabled: false,
      setActiveConversation: (id) => set({ activeConversationId: id }),
      setProvider: (provider, model) => set({ provider, model }),
      setModel: (model) => set({ model }),
      setAttachProject: (attach) => set({ attachProject: attach }),
      setToolsEnabled: (enabled) =>
        set((s) => ({
          toolsEnabled: enabled,
          // Revoking read access also revokes the write level.
          writeToolsEnabled: enabled ? s.writeToolsEnabled : false,
        })),
      setWriteToolsEnabled: (enabled) => set({ writeToolsEnabled: enabled }),
    }),
    { name: "devos-ai" },
  ),
);

export const CLAUDE_MODELS = [
  "claude-sonnet-5",
  "claude-opus-4-8",
  "claude-haiku-4-5",
];
