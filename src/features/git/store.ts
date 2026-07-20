import { create } from "zustand";
import { persist } from "zustand/middleware";

interface GitState {
  /** Project the git page is pointed at; falls back to the first project. */
  selectedProjectId: string | null;
  setSelectedProject: (id: string) => void;
}

export const useGitStore = create<GitState>()(
  persist(
    (set) => ({
      selectedProjectId: null,
      setSelectedProject: (id) => set({ selectedProjectId: id }),
    }),
    { name: "devos-git" },
  ),
);
