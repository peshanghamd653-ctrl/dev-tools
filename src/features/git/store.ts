/**
 * `selectedProjectId` is not git state. The files page, the issue hooks, the
 * command palette and the AI page's attached context all read it, and the AI
 * page reading it *from here* is what made `ai` and `git` import each other
 * (`docs/architecture.md`). It now lives in `@/shared/stores/project`, under
 * the same `devos-git` storage key it has always used.
 *
 * This alias keeps the `useGitStore` name working for the callers that still
 * import it from here — it is the same store object, not a copy.
 */
export { useProjectStore as useGitStore } from "@/shared/stores/project";
