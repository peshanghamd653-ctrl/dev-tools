/**
 * The selected project moved out of `features/git/store` so that the AI page
 * could stop importing it — the `ai` ⇄ `git` cycle in `docs/architecture.md`.
 * Moving state between stores normally moves its storage key with it, which
 * would silently reset everyone's selected project on upgrade, so these tests
 * pin the key and the stored shape as much as the behaviour.
 */
import { beforeEach, describe, expect, it } from "vitest";

import { useProjectStore } from "./project";

const STORAGE_KEY = "devos-git";
const initialState = useProjectStore.getState();

describe("project selection store", () => {
  beforeEach(() => {
    useProjectStore.setState(initialState, true);
    localStorage.clear();
  });

  it("starts with nothing selected", () => {
    expect(useProjectStore.getState().selectedProjectId).toBeNull();
  });

  it("tracks the selected project", () => {
    useProjectStore.getState().setSelectedProject("project-1");
    expect(useProjectStore.getState().selectedProjectId).toBe("project-1");
  });

  it("still persists under the key the git store used", () => {
    useProjectStore.getState().setSelectedProject("project-2");

    const raw = localStorage.getItem(STORAGE_KEY);
    expect(raw, `nothing was persisted under ${STORAGE_KEY}`).not.toBeNull();
    expect(
      (JSON.parse(raw as string) as { state: Record<string, unknown> }).state,
    ).toMatchObject({ selectedProjectId: "project-2" });
  });

  it("rehydrates a selection written by the old git store", async () => {
    localStorage.setItem(
      STORAGE_KEY,
      JSON.stringify({ version: 0, state: { selectedProjectId: "upgraded" } }),
    );

    await useProjectStore.persist.rehydrate();

    expect(useProjectStore.getState().selectedProjectId).toBe("upgraded");
  });
});
