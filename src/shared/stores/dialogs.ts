import { create } from "zustand";

/** Global dialogs that can be opened from anywhere (palette, pages, shortcuts). */
interface DialogState {
  createWorkspaceOpen: boolean;
  addProjectOpen: boolean;
  /** Screenshot → GitHub issue (Ctrl+Shift+S, or `issue.capture`). */
  captureIssueOpen: boolean;
  setCreateWorkspaceOpen: (open: boolean) => void;
  setAddProjectOpen: (open: boolean) => void;
  setCaptureIssueOpen: (open: boolean) => void;
}

export const useDialogStore = create<DialogState>()((set) => ({
  createWorkspaceOpen: false,
  addProjectOpen: false,
  captureIssueOpen: false,
  setCreateWorkspaceOpen: (open) => set({ createWorkspaceOpen: open }),
  setAddProjectOpen: (open) => set({ addProjectOpen: open }),
  setCaptureIssueOpen: (open) => set({ captureIssueOpen: open }),
}));
