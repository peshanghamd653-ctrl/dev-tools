import { create } from "zustand";

/** Global dialogs that can be opened from anywhere (palette, pages, shortcuts). */
interface DialogState {
  createWorkspaceOpen: boolean;
  addProjectOpen: boolean;
  setCreateWorkspaceOpen: (open: boolean) => void;
  setAddProjectOpen: (open: boolean) => void;
}

export const useDialogStore = create<DialogState>()((set) => ({
  createWorkspaceOpen: false,
  addProjectOpen: false,
  setCreateWorkspaceOpen: (open) => set({ createWorkspaceOpen: open }),
  setAddProjectOpen: (open) => set({ addProjectOpen: open }),
}));
