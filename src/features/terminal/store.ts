import { create } from "zustand";

import type { TermSessionInfo } from "@/shared/ipc/client";

export type TermStatus = "live" | "exited" | "disconnected";

export interface TermSession {
  info: TermSessionInfo;
  status: TermStatus;
}

interface TerminalState {
  sessions: TermSession[];
  activeId: string | null;
  /** Second visible pane when split view is on. */
  splitId: string | null;
  addLive: (info: TermSessionInfo) => void;
  addDisconnected: (info: TermSessionInfo) => void;
  setExited: (id: string) => void;
  remove: (id: string) => void;
  setActive: (id: string) => void;
  toggleSplit: () => void;
}

export const useTerminalStore = create<TerminalState>()((set) => ({
  sessions: [],
  activeId: null,
  splitId: null,
  addLive: (info) =>
    set((s) => ({
      sessions: [...s.sessions, { info, status: "live" as const }],
      activeId: info.id,
    })),
  addDisconnected: (info) =>
    set((s) =>
      s.sessions.some((x) => x.info.id === info.id)
        ? s
        : {
            sessions: [...s.sessions, { info, status: "disconnected" as const }],
          },
    ),
  setExited: (id) =>
    set((s) => ({
      sessions: s.sessions.map((x) =>
        x.info.id === id ? { ...x, status: "exited" as const } : x,
      ),
    })),
  remove: (id) =>
    set((s) => {
      const sessions = s.sessions.filter((x) => x.info.id !== id);
      return {
        sessions,
        activeId:
          s.activeId === id
            ? (sessions[sessions.length - 1]?.info.id ?? null)
            : s.activeId,
        splitId: s.splitId === id ? null : s.splitId,
      };
    }),
  setActive: (id) => set({ activeId: id }),
  toggleSplit: () =>
    set((s) => {
      if (s.splitId) return { splitId: null };
      const other = s.sessions.find(
        (x) => x.info.id !== s.activeId && x.status === "live",
      );
      return { splitId: other?.info.id ?? null };
    }),
}));
