/**
 * The published AI selection exists so `features/git` can read which model to
 * use without importing `features/ai` — the other half of the `ai` ⇄ `git`
 * cycle in `docs/architecture.md`. `features/ai/store` remains the owner and
 * the only thing that persists the choice, which is what these tests hold it
 * to: this store must read that preference correctly and store nothing.
 */
import { beforeEach, describe, expect, it } from "vitest";

import {
  AI_STORAGE_KEY,
  DEFAULT_AI_SELECTION,
  readPersistedAiSelection,
  useAiModelStore,
} from "./ai-model";

const initialState = useAiModelStore.getState();

function persistSelection(state: unknown) {
  localStorage.setItem(AI_STORAGE_KEY, JSON.stringify({ version: 0, state }));
}

describe("published ai selection", () => {
  beforeEach(() => {
    useAiModelStore.setState(initialState, true);
    localStorage.clear();
  });

  it("falls back to the default model when nothing is stored", () => {
    expect(readPersistedAiSelection()).toEqual({ ...DEFAULT_AI_SELECTION });
  });

  it("reads the selection the ai store persisted", () => {
    persistSelection({
      provider: "gemini",
      model: "gemini-3.6-flash",
      attachProject: true,
    });

    expect(readPersistedAiSelection()).toEqual({
      provider: "gemini",
      model: "gemini-3.6-flash",
    });
  });

  /**
   * A cold start into the git page hands these straight to the backend, so a
   * blob from an older build — or a hand-edited one — must degrade to the
   * default rather than to `undefined`.
   */
  it("falls back field by field on a malformed blob", () => {
    persistSelection({ provider: "ollama", model: "   " });
    expect(readPersistedAiSelection()).toEqual({
      provider: "ollama",
      model: DEFAULT_AI_SELECTION.model,
    });

    localStorage.setItem(AI_STORAGE_KEY, "not json");
    expect(readPersistedAiSelection()).toEqual({ ...DEFAULT_AI_SELECTION });
  });

  it("takes whatever the ai store publishes", () => {
    useAiModelStore.getState().publish("ollama", "qwen3:8b");
    expect(useAiModelStore.getState()).toMatchObject({
      provider: "ollama",
      model: "qwen3:8b",
    });
  });

  it("ignores a republish of the selection it already has", () => {
    let notifications = 0;
    const unsubscribe = useAiModelStore.subscribe(() => {
      notifications += 1;
    });

    useAiModelStore.getState().publish("gemini", "gemini-2.5-flash");
    useAiModelStore.getState().publish("gemini", "gemini-2.5-flash");
    unsubscribe();

    expect(notifications).toBe(1);
  });

  /**
   * Two writers on `devos-ai` would clobber each other's fields, and a second
   * key would silently reset the model choice on upgrade — so this store owns
   * no storage at all.
   */
  it("persists nothing of its own", () => {
    useAiModelStore.getState().publish("gemini", "gemini-3.5-flash");
    expect(localStorage.length).toBe(0);
  });
});
