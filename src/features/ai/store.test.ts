/**
 * The two tool grants are the standing half of the AI capability model
 * (ADR-0005): the grant decides which tools exist for the model, per-call
 * approval decides whether an individual call runs. `docs/security.md` says
 * both are "off by default" — these tests are written so they fail if that
 * degrades into "off on first run only" (SEC-009).
 */
import { beforeEach, describe, expect, it } from "vitest";

import {
  AI_STORAGE_KEY,
  readPersistedAiSelection,
  useAiModelStore,
} from "@/shared/stores/ai-model";
import { useAiStore } from "./store";

const STORAGE_KEY = "devos-ai";
const initialState = useAiStore.getState();

function storedState(): Record<string, unknown> {
  const raw = localStorage.getItem(STORAGE_KEY);
  expect(raw, "nothing was persisted").not.toBeNull();
  return (JSON.parse(raw as string) as { state: Record<string, unknown> }).state;
}

describe("ai tool grants", () => {
  beforeEach(() => {
    useAiStore.setState(initialState, true);
    localStorage.clear();
  });

  it("starts with both grants off", () => {
    expect(useAiStore.getState().toolsEnabled).toBe(false);
    expect(useAiStore.getState().writeToolsEnabled).toBe(false);
  });

  it("revoking the read grant takes the write grant with it", () => {
    const { setToolsEnabled, setWriteToolsEnabled } = useAiStore.getState();
    setToolsEnabled(true);
    setWriteToolsEnabled(true);
    expect(useAiStore.getState().writeToolsEnabled).toBe(true);

    setToolsEnabled(false);
    expect(useAiStore.getState().writeToolsEnabled).toBe(false);
  });

  /**
   * SEC-009. A standing grant to propose edits and shell commands must not
   * outlive the session that granted it, so it never reaches storage at all.
   * The read grant does persist — it is side-effect-free and the chip shows
   * it on screen — which is why this asserts both halves.
   */
  it("persists the read grant but never the write/execute grant", () => {
    useAiStore.getState().setToolsEnabled(true);
    useAiStore.getState().setWriteToolsEnabled(true);

    const stored = storedState();
    expect(stored.toolsEnabled).toBe(true);
    expect(stored).not.toHaveProperty("writeToolsEnabled");
  });

  /**
   * Dropping the key from `partialize` is not enough on its own: rehydration
   * merges whatever is on disk over the initial state, so an install that
   * already wrote the grant would keep restoring it forever.
   */
  it("ignores a write grant an earlier build left on disk", async () => {
    localStorage.setItem(
      STORAGE_KEY,
      JSON.stringify({
        version: 0,
        state: {
          provider: "claude",
          model: "claude-sonnet-5",
          attachProject: true,
          toolsEnabled: true,
          writeToolsEnabled: true,
        },
      }),
    );

    await useAiStore.persist.rehydrate();

    // The read grant proves rehydration actually ran...
    expect(useAiStore.getState().toolsEnabled).toBe(true);
    // ...so this is a refusal, not a store that never loaded.
    expect(useAiStore.getState().writeToolsEnabled).toBe(false);
  });
});

/**
 * The git page reads the published copy of the selection instead of this
 * store — reading it from here is what closed the `ai` ⇄ `git` cycle in
 * `docs/architecture.md`. This store stays the owner, so if publication stops
 * the git page silently generates commit messages on the wrong model.
 */
describe("ai model publication", () => {
  beforeEach(() => {
    useAiStore.setState(initialState, true);
    localStorage.clear();
  });

  it("publishes the selection to the shared store", () => {
    useAiStore.getState().setProvider("gemini", "gemini-3.6-flash");
    expect(useAiModelStore.getState()).toMatchObject({
      provider: "gemini",
      model: "gemini-3.6-flash",
    });

    useAiStore.getState().setModel("gemini-2.5-flash");
    expect(useAiModelStore.getState().model).toBe("gemini-2.5-flash");
  });

  it("publishes what rehydration restored", async () => {
    localStorage.setItem(
      STORAGE_KEY,
      JSON.stringify({
        version: 0,
        state: { provider: "ollama", model: "qwen3:8b" },
      }),
    );

    await useAiStore.persist.rehydrate();

    expect(useAiModelStore.getState()).toMatchObject({
      provider: "ollama",
      model: "qwen3:8b",
    });
  });

  /**
   * A cold start into the git page never evaluates this module — route
   * components are code split — so the shared store reads this store's blob
   * itself. That only works while both agree on the key and the field names.
   */
  it("writes a blob the shared store can read back", () => {
    expect(STORAGE_KEY).toBe(AI_STORAGE_KEY);

    useAiStore.getState().setProvider("gemini", "gemini-3.5-flash-lite");

    expect(readPersistedAiSelection()).toEqual({
      provider: "gemini",
      model: "gemini-3.5-flash-lite",
    });
  });
});
