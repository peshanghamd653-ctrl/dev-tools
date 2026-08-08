import { create } from "zustand";
import { persist } from "zustand/middleware";

interface ProjectState {
  /** The project every project-scoped page is pointed at. */
  selectedProjectId: string | null;
  setSelectedProject: (id: string) => void;
}

/**
 * Which project the workbench is pointed at. Git picks it, but the files
 * page, the issue hooks, the command palette and the AI page's attached
 * context all read the same value — so it is app state that happened to be
 * declared inside a feature, and the AI page reaching into `features/git` for
 * it is half of the `ai` ⇄ `git` cycle `docs/architecture.md` calls out.
 *
 * Persisted under `devos-git` deliberately: that is the key every existing
 * install already holds this value under, and the stored shape
 * (`{ state: { selectedProjectId } }`) is unchanged, so an entry written by
 * the old `features/git/store` rehydrates here untouched. Renaming the key
 * would silently reset everyone's selected project on upgrade.
 * `features/git/store` still exports `useGitStore` as an alias of this store
 * for the callers that import it from there.
 */
export const useProjectStore = create<ProjectState>()(
  persist(
    (set) => ({
      selectedProjectId: null,
      setSelectedProject: (id) => set({ selectedProjectId: id }),
    }),
    { name: "devos-git" },
  ),
);
