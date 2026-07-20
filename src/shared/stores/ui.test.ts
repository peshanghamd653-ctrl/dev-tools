import { beforeEach, describe, expect, it } from "vitest";

import { useUiStore } from "./ui";

const initialState = useUiStore.getState();

describe("ui store", () => {
  beforeEach(() => {
    useUiStore.setState(initialState, true);
  });

  it("toggles the command palette", () => {
    expect(useUiStore.getState().paletteOpen).toBe(false);
    useUiStore.getState().togglePalette();
    expect(useUiStore.getState().paletteOpen).toBe(true);
    useUiStore.getState().setPaletteOpen(false);
    expect(useUiStore.getState().paletteOpen).toBe(false);
  });

  it("opens tabs without duplicating them", () => {
    const { openTab } = useUiStore.getState();
    openTab({ path: "/projects", title: "Projects" });
    openTab({ path: "/projects", title: "Projects" });
    expect(useUiStore.getState().tabs).toHaveLength(1);
  });

  it("closes tabs by path", () => {
    const { openTab, closeTab } = useUiStore.getState();
    openTab({ path: "/", title: "Dashboard" });
    openTab({ path: "/projects", title: "Projects" });
    closeTab("/");
    expect(useUiStore.getState().tabs).toEqual([
      { path: "/projects", title: "Projects" },
    ]);
  });

  it("tracks the active workspace", () => {
    useUiStore.getState().setActiveWorkspace("ws-1");
    expect(useUiStore.getState().activeWorkspaceId).toBe("ws-1");
  });
});
