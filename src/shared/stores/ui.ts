import { create } from "zustand";
import { persist } from "zustand/middleware";

export interface Tab {
  path: string;
  title: string;
}

/** A file the symbol picker asked the File Explorer to open, and land on. */
export interface PendingFileOpen {
  projectPath: string;
  relative: string;
  line: number;
}

interface UiState {
  paletteOpen: boolean;
  symbolSearchOpen: boolean;
  sidebarCollapsed: boolean;
  activeWorkspaceId: string | null;
  tabs: Tab[];
  pendingFileOpen: PendingFileOpen | null;
  setPaletteOpen: (open: boolean) => void;
  togglePalette: () => void;
  setSymbolSearchOpen: (open: boolean) => void;
  toggleSymbolSearch: () => void;
  toggleSidebar: () => void;
  setActiveWorkspace: (id: string) => void;
  openTab: (tab: Tab) => void;
  closeTab: (path: string) => void;
  setPendingFileOpen: (target: PendingFileOpen | null) => void;
}

export const useUiStore = create<UiState>()(
  persist(
    (set) => ({
      paletteOpen: false,
      symbolSearchOpen: false,
      sidebarCollapsed: false,
      activeWorkspaceId: null,
      tabs: [],
      pendingFileOpen: null,
      setPaletteOpen: (open) => set({ paletteOpen: open }),
      togglePalette: () => set((s) => ({ paletteOpen: !s.paletteOpen })),
      setSymbolSearchOpen: (open) => set({ symbolSearchOpen: open }),
      toggleSymbolSearch: () =>
        set((s) => ({ symbolSearchOpen: !s.symbolSearchOpen })),
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
      setPendingFileOpen: (target) => set({ pendingFileOpen: target }),
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
