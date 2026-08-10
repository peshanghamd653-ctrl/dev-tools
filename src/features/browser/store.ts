import { create } from "zustand";

interface BrowserState {
  /**
   * The last URL navigated to, or `null` if the pane has never been
   * opened this session. Read on mount so returning to the Browser page
   * re-shows the same page (the Rust side keeps the child webview alive,
   * hidden, across visits) instead of prompting for a URL again.
   */
  currentUrl: string | null;
  setCurrentUrl: (url: string) => void;
}

export const useBrowserStore = create<BrowserState>()((set) => ({
  currentUrl: null,
  setCurrentUrl: (url) => set({ currentUrl: url }),
}));
