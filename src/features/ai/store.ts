import { create } from "zustand";
import { persist } from "zustand/middleware";

/** Providers the backend can resolve — see `AiRegistry::provider`. */
export type AiProviderId = "claude" | "gemini" | "ollama";

interface AiState {
  activeConversationId: string | null;
  /** Provider/model used when creating the next conversation. */
  provider: AiProviderId;
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
   * chat (ADR-0005). Off by default — and, unlike every other field here,
   * **never restored from disk**: see the persist config below.
   */
  writeToolsEnabled: boolean;
  /**
   * A message queued by another page (e.g. terminal "Diagnose with AI");
   * the AI page consumes and sends it exactly once. Never persisted.
   */
  pendingPrompt: string | null;
  setActiveConversation: (id: string | null) => void;
  setProvider: (provider: AiProviderId, model: string) => void;
  setModel: (model: string) => void;
  setAttachProject: (attach: boolean) => void;
  setToolsEnabled: (enabled: boolean) => void;
  setWriteToolsEnabled: (enabled: boolean) => void;
  setPendingPrompt: (prompt: string | null) => void;
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
      pendingPrompt: null,
      setWriteToolsEnabled: (enabled) => set({ writeToolsEnabled: enabled }),
      setPendingPrompt: (prompt) => set({ pendingPrompt: prompt }),
    }),
    {
      name: "devos-ai",
      /**
       * `writeToolsEnabled` is deliberately absent (SEC-009). "Off by
       * default" has to mean off at every launch, not off on first run: a
       * standing grant that lets the model *propose* edits and shell
       * commands should not survive a restart, because the user who granted
       * it three weeks ago is not the user opening the app today. Per-call
       * approval still guards each action, but the grant is what puts those
       * tools in the model's tool list at all — and a tool that is not
       * offered is the one thing a prompt injection cannot reach for.
       *
       * The read grant does persist: read-only tools are side-effect-free by
       * construction (ADR-0005), the chip states the grant on screen the
       * whole time, and `save_memory` — the one mutating tool in that tier —
       * now goes through per-call approval (SEC-002).
       */
      partialize: (s) => ({
        activeConversationId: s.activeConversationId,
        provider: s.provider,
        model: s.model,
        attachProject: s.attachProject,
        toolsEnabled: s.toolsEnabled,
      }),
      /**
       * Rehydration merges the stored object over the initial state, so
       * dropping the key above is not enough on its own: an install that
       * already wrote `writeToolsEnabled: true` would keep restoring it.
       * Forcing it here makes the guarantee independent of what is on disk.
       */
      merge: (persisted, current) => ({
        ...current,
        ...(persisted as Partial<AiState>),
        writeToolsEnabled: false,
      }),
    },
  ),
);

export const CLAUDE_MODELS = [
  "claude-sonnet-5",
  "claude-opus-4-8",
  "claude-haiku-4-5",
];

/**
 * Flash-class only, deliberately. The pro models are the ones a free key
 * cannot usefully drive, so listing them would mostly produce quota errors
 * from the provider people pick *because* it is free.
 *
 * Kept in step with `GEMINI_MODELS` in `crates/devos-ai/src/providers/gemini.rs`
 * — the Rust list is what a default lands on; this one is what the picker
 * offers.
 */
export const GEMINI_MODELS = [
  "gemini-3.6-flash",
  "gemini-3.5-flash",
  "gemini-3.5-flash-lite",
  "gemini-2.5-flash",
];

/** Display names, so a third provider doesn't mean a third nested ternary. */
export const PROVIDER_LABELS: Record<string, string> = {
  claude: "Claude",
  gemini: "Gemini",
  ollama: "Ollama",
};
