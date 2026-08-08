import { create } from "zustand";

/**
 * The storage key `features/ai/store` persists under. It is declared here,
 * and imported back by that store, so the key this module reads and the key
 * that store writes cannot drift — `features/ai/store.test.ts` pins the round
 * trip.
 */
export const AI_STORAGE_KEY = "devos-ai";

/** What an AI call runs on before anyone has chosen anything. */
export const DEFAULT_AI_SELECTION = {
  provider: "claude",
  model: "claude-sonnet-5",
} as const;

export interface AiSelection {
  /** An `AiProviderId`; kept as a plain string so `shared` stays feature-free. */
  provider: string;
  model: string;
}

interface AiModelState extends AiSelection {
  /** Called by `features/ai/store` whenever the selection changes. */
  publish: (provider: string, model: string) => void;
}

/**
 * Recovers the selection from the AI store's persisted blob.
 *
 * Defensive on purpose: the blob may be absent, from an older build, or hand
 * edited, and a bad value here would be handed straight to the backend.
 */
export function readPersistedAiSelection(): AiSelection {
  try {
    const raw = globalThis.localStorage?.getItem(AI_STORAGE_KEY);
    if (!raw) return { ...DEFAULT_AI_SELECTION };
    const stored = (JSON.parse(raw) as { state?: Partial<AiSelection> }).state;
    return {
      provider: nonEmpty(stored?.provider) ?? DEFAULT_AI_SELECTION.provider,
      model: nonEmpty(stored?.model) ?? DEFAULT_AI_SELECTION.model,
    };
  } catch {
    return { ...DEFAULT_AI_SELECTION };
  }
}

function nonEmpty(value: unknown): string | null {
  return typeof value === "string" && value.trim() !== "" ? value : null;
}

/**
 * The provider/model the user picked in the AI page, readable by any feature
 * that fires an AI call of its own — git's "generate a commit message" is the
 * one today — without importing `features/ai`. That import was the other half
 * of the `ai` ⇄ `git` cycle in `docs/architecture.md`.
 *
 * `features/ai/store` stays the owner: it is the only writer, and the only
 * thing that persists the choice. This store deliberately persists nothing of
 * its own — a second writer on `devos-ai` would clobber the rest of the AI
 * state, and a second key would silently reset the model choice for every
 * install that upgrades. It instead seeds itself from the owner's blob and is
 * refreshed by `publish()` on every change, because route components are code
 * split (`autoCodeSplitting` in `vite.config.ts`): a cold start straight into
 * the git page never evaluates the AI store, so "ask the owner" is not an
 * option and "wait to be told" would fall back to the wrong model.
 */
export const useAiModelStore = create<AiModelState>()((set) => ({
  ...readPersistedAiSelection(),
  publish: (provider, model) =>
    set((s) =>
      s.provider === provider && s.model === model ? s : { provider, model },
    ),
}));
