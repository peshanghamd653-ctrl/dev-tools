import { create } from "zustand";
import { persist } from "zustand/middleware";

export interface Tab {
  path: string;
  title: string;
}

interface UiState {
  paletteOpen: boolean;
  sidebarCollapsed: boolean;
  activeWorkspaceId: string | null;
  tabs: Tab[];
  setPaletteOpen: (open: boolean) => void;
  togglePalette: () => void;
  toggleSidebar: () => void;
  setActiveWorkspace: (id: string) => void;
  openTab: (tab: Tab) => void;
  closeTab: (path: string) => void;
}

export const useUiStore = create<UiState>()(
  persist(
    (set) => ({
      paletteOpen: false,
      sidebarCollapsed: false,
      activeWorkspaceId: null,
      tabs: [],
      setPaletteOpen: (open) => set({ paletteOpen: open }),
      togglePalette: () => set((s) => ({ paletteOpen: !s.paletteOpen })),
      toggleSidebar: () =>
        set((s) => ({ sidebarCollapsed: !s.sidebarCollapsed })),
      setActiveWorkspace: (id) => set({ activeWorkspaceId: id }),
      openTab: (tab) =>
        set((s) =>
          s.tabs.some((t) => t.path === tab.path)
            ? s
            : { tabs: [...s.tabs, tab] },
        ),
      closeTab: (path) =>
        set((s) => ({ tabs: s.tabs.filter((t) => t.path !== path) })),
    }),
    {
      name: "devos-ui",
      partialize: (s) => ({
        sidebarCollapsed: s.sidebarCollapsed,
        activeWorkspaceId: s.activeWorkspaceId,
        tabs: s.tabs,
      }),
    },
  ),
);
