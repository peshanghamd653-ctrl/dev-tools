/**
 * The two tool grants are the standing half of the AI capability model
 * (ADR-0005): the grant decides which tools exist for the model, per-call
 * approval decides whether an individual call runs. `docs/security.md` says
 * both are "off by default" — these tests are written so they fail if that
 * degrades into "off on first run only" (SEC-009).
 */
import { beforeEach, describe, expect, it } from "vitest";

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
