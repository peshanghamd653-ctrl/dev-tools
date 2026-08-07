/**
 * Typed IPC client. All webview -> kernel calls go through here; no feature
 * imports `invoke` directly. Payload types are generated from Rust by ts-rs
 * (`pnpm gen:types`) into `./bindings`.
 */
import { invoke, type Channel } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import type { AiDelta } from "./bindings/AiDelta";
import type { ApiHeader } from "./bindings/ApiHeader";
import type { ApiHistoryEntry } from "./bindings/ApiHistoryEntry";
import type { ApiRequestSpec } from "./bindings/ApiRequestSpec";
import type { ApiResponse } from "./bindings/ApiResponse";
import type { AppInfo } from "./bindings/AppInfo";
import type { AuditEntry } from "./bindings/AuditEntry";
import type { AuditLog } from "./bindings/AuditLog";
import type { BackupEntry } from "./bindings/BackupEntry";
import type { BackupKind } from "./bindings/BackupKind";
import type { BackupListing } from "./bindings/BackupListing";
import type { PendingRestore } from "./bindings/PendingRestore";
import type { CapturedShot } from "./bindings/CapturedShot";
import type { ChatMessage } from "./bindings/ChatMessage";
import type { CreatedIssue } from "./bindings/CreatedIssue";
import type { IssueTarget } from "./bindings/IssueTarget";
import type { CommandDescriptor } from "./bindings/CommandDescriptor";
import type { Conversation } from "./bindings/Conversation";
import type { DbColumn } from "./bindings/DbColumn";
import type { DbConnection } from "./bindings/DbConnection";
import type { DbSchema } from "./bindings/DbSchema";
import type { DbTable } from "./bindings/DbTable";
import type { DeployProject } from "./bindings/DeployProject";
import type { Deployment } from "./bindings/Deployment";
import type { DockerContainer } from "./bindings/DockerContainer";
import type { DockerImage } from "./bindings/DockerImage";
import type { GitBranch } from "./bindings/GitBranch";
import type { GitCommit } from "./bindings/GitCommit";
import type { GitFileEntry } from "./bindings/GitFileEntry";
import type { GitRepoInfo } from "./bindings/GitRepoInfo";
import type { FilePreview } from "./bindings/FilePreview";
import type { FsEntry } from "./bindings/FsEntry";
import type { GitStatus } from "./bindings/GitStatus";
import type { IndexHit } from "./bindings/IndexHit";
import type { IndexStats } from "./bindings/IndexStats";
import type { JobInfo } from "./bindings/JobInfo";
import type { KernelEvent } from "./bindings/KernelEvent";
import type { MemoryEntry } from "./bindings/MemoryEntry";
import type { Monitor } from "./bindings/Monitor";
import type { MonitorCheck } from "./bindings/MonitorCheck";
import type { MonitorStatus } from "./bindings/MonitorStatus";
import type { NotificationDto } from "./bindings/NotificationDto";
import type { Project } from "./bindings/Project";
import type { QueryResult } from "./bindings/QueryResult";
import type { DiskInfo } from "./bindings/DiskInfo";
import type { ProcessInfo } from "./bindings/ProcessInfo";
import type { SystemSnapshot } from "./bindings/SystemSnapshot";
import type { Snippet } from "./bindings/Snippet";
import type { SnippetDraft } from "./bindings/SnippetDraft";
import type { TermEvent } from "./bindings/TermEvent";
import type { SavedRequest } from "./bindings/SavedRequest";
import type { TermSessionInfo } from "./bindings/TermSessionInfo";
import type { Workspace } from "./bindings/Workspace";

export type {
  AiDelta,
  ApiHeader,
  ApiHistoryEntry,
  ApiRequestSpec,
  ApiResponse,
  AppInfo,
  AuditEntry,
  AuditLog,
  BackupEntry,
  BackupKind,
  BackupListing,
  PendingRestore,
  SavedRequest,
  CapturedShot,
  ChatMessage,
  CommandDescriptor,
  Conversation,
  CreatedIssue,
  DbColumn,
  DbConnection,
  DbSchema,
  DbTable,
  DeployProject,
  Deployment,
  DockerContainer,
  DockerImage,
  GitBranch,
  GitCommit,
  GitFileEntry,
  GitRepoInfo,
  FilePreview,
  FsEntry,
  GitStatus,
  IndexHit,
  IndexStats,
  IssueTarget,
  JobInfo,
  KernelEvent,
  MemoryEntry,
  Monitor,
  MonitorCheck,
  MonitorStatus,
  NotificationDto,
  Project,
  QueryResult,
  DiskInfo,
  ProcessInfo,
  Snippet,
  SnippetDraft,
  SystemSnapshot,
  TermEvent,
  TermSessionInfo,
  Workspace,
};

/**
 * True when running inside the Tauri shell (vs. a plain browser tab).
 *
 * Evaluated on every call rather than once at import: `__TAURI_INTERNALS__` is
 * injected by the shell, and any module that happens to be imported before
 * that lands would otherwise snapshot `false` for the life of the process. It
 * also means a test can flip the answer without a module mock.
 */
export function isDesktopShell(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (!isDesktopShell()) {
    return Promise.reject(
      new Error("DevOS IPC is only available inside the desktop shell"),
    );
  }
  return invoke<T>(command, args);
}

export const ipc = {
  appInfo: () => call<AppInfo>("app_info"),

  workspacesList: () => call<Workspace[]>("workspaces_list"),
  workspaceCreate: (name: string) =>
    call<Workspace>("workspace_create", { name }),
  workspaceRename: (id: string, name: string) =>
    call<Workspace>("workspace_rename", { id, name }),
  workspaceDelete: (id: string) => call<void>("workspace_delete", { id }),

  projectsList: (workspaceId: string) =>
    call<Project[]>("projects_list", { workspaceId }),
  projectAdd: (workspaceId: string, name: string, path: string) =>
    call<Project>("project_add", { workspaceId, name, path }),
  projectRemove: (id: string, workspaceId: string) =>
    call<void>("project_remove", { id, workspaceId }),

  settingsGet: (key: string) => call<string | null>("settings_get", { key }),
  settingsSet: (key: string, value: string) =>
    call<void>("settings_set", { key, value }),

  commandsList: () => call<CommandDescriptor[]>("commands_list"),
  jobsRecent: () => call<JobInfo[]>("jobs_recent"),

  notificationsList: () => call<NotificationDto[]>("notifications_list"),
  notificationsUnreadCount: () => call<number>("notifications_unread_count"),
  notificationMarkRead: (id: string) =>
    call<void>("notification_mark_read", { id }),
  notificationsMarkAllRead: () => call<void>("notifications_mark_all_read"),

  /**
   * The newest audit entries plus the shape of the whole table.
   *
   * Read-only, and there is no sibling: the audit log has no write, edit or
   * delete command by design. Rows are appended by the code paths that perform
   * the audited actions, and removed only by the kernel's age-based prune at
   * boot — an IPC surface that could touch them would be a surface for writing
   * one's own alibi.
   */
  auditLog: (limit: number) => call<AuditLog>("audit_log", { limit }),

  backupsList: () => call<BackupListing>("backups_list"),
  /**
   * Stage a restore. Nothing about the live database changes until the app
   * restarts — `backupRestart` is what makes it happen, and
   * `backupRestoreCancel` un-makes it until then.
   */
  backupRestoreStage: (name: string) =>
    call<PendingRestore>("backup_restore_stage", { name }),
  backupRestoreCancel: () => call<void>("backup_restore_cancel"),
  /**
   * Ask the shell to exit and come back up, which is what applies a staged
   * restore. Resolving means the request was accepted, not that the restart
   * finished — the window is closing, so there is no "after" to observe.
   */
  backupRestart: () => call<void>("backup_restart"),

  termCreate: (
    opts: { shell?: string; cwd?: string; cols: number; rows: number },
    onOutput: Channel<TermEvent>,
  ) => call<TermSessionInfo>("term_create", { ...opts, onOutput }),
  termWrite: (id: string, data: string) => call<void>("term_write", { id, data }),
  termResize: (id: string, cols: number, rows: number) =>
    call<void>("term_resize", { id, cols, rows }),
  termKill: (id: string) => call<void>("term_kill", { id }),
  termList: () => call<TermSessionInfo[]>("term_list"),
  termTail: (id: string) => call<string>("term_tail", { id }),

  gitStatus: (path: string) => call<GitStatus>("git_status", { path }),
  gitStage: (path: string, files: string[]) =>
    call<void>("git_stage", { path, files }),
  gitUnstage: (path: string, files: string[]) =>
    call<void>("git_unstage", { path, files }),
  gitDiscard: (path: string, file: string, untracked: boolean) =>
    call<void>("git_discard", { path, file, untracked }),
  gitCommit: (path: string, message: string) =>
    call<string>("git_commit", { path, message }),
  gitLog: (path: string, limit: number) =>
    call<GitCommit[]>("git_log", { path, limit }),
  gitBranches: (path: string) => call<GitBranch[]>("git_branches", { path }),
  gitSwitch: (path: string, branch: string, create: boolean) =>
    call<void>("git_switch", { path, branch, create }),
  gitDiff: (path: string, file: string, staged: boolean, untracked: boolean) =>
    call<string>("git_diff", { path, file, staged, untracked }),
  gitPush: (path: string) => call<string>("git_push", { path }),
  gitPull: (path: string) => call<string>("git_pull", { path }),

  secretSet: (name: string, value: string) =>
    call<void>("secret_set", { name, value }),
  secretList: () => call<string[]>("secret_list"),
  secretDelete: (name: string) => call<void>("secret_delete", { name }),

  aiConversationsList: () => call<Conversation[]>("ai_conversations_list"),
  aiConversationCreate: (provider: string, model: string) =>
    call<Conversation>("ai_conversation_create", { provider, model }),
  aiConversationDelete: (id: string) =>
    call<void>("ai_conversation_delete", { id }),
  aiMessages: (conversationId: string) =>
    call<ChatMessage[]>("ai_messages", { conversationId }),
  aiOllamaModels: () => call<string[]>("ai_ollama_models"),
  aiSend: (
    conversationId: string,
    content: string,
    projectPath: string | null,
    toolsEnabled: boolean,
    writeToolsEnabled: boolean,
    onDelta: Channel<AiDelta>,
  ) =>
    call<ChatMessage>("ai_send", {
      conversationId,
      content,
      projectPath,
      toolsEnabled,
      writeToolsEnabled,
      onDelta,
    }),
  aiToolRespond: (id: string, approved: boolean) =>
    call<boolean>("ai_tool_respond", { id, approved }),

  indexProject: (path: string) => call<string>("index_project", { path }),
  indexStats: (path: string) => call<IndexStats>("index_stats", { path }),
  indexSearch: (path: string, query: string) =>
    call<IndexHit[]>("index_search", { path, query }),

  fsListDir: (projectPath: string, relative: string) =>
    call<FsEntry[]>("fs_list_dir", { projectPath, relative }),
  fsReadFile: (projectPath: string, relative: string) =>
    call<FilePreview>("fs_read_file", { projectPath, relative }),
  fsFind: (projectPath: string, query: string) =>
    call<string[]>("fs_find", { projectPath, query }),

  dockerPing: () => call<string>("docker_ping"),
  dockerContainers: () => call<DockerContainer[]>("docker_containers"),
  dockerImages: () => call<DockerImage[]>("docker_images"),
  dockerStart: (id: string) => call<void>("docker_start", { id }),
  dockerStop: (id: string) => call<void>("docker_stop", { id }),
  dockerRestart: (id: string) => call<void>("docker_restart", { id }),
  dockerLogs: (id: string) => call<string>("docker_logs", { id }),

  apiSend: (spec: ApiRequestSpec) => call<ApiResponse>("api_send", { spec }),
  apiSave: (name: string, collection: string, spec: ApiRequestSpec) =>
    call<SavedRequest>("api_save", { name, collection, spec }),
  apiRequests: () => call<SavedRequest[]>("api_requests"),
  apiRequestDelete: (id: string) => call<void>("api_request_delete", { id }),
  apiHistory: () => call<ApiHistoryEntry[]>("api_history"),

  dbConnections: () => call<DbConnection[]>("db_connections"),
  dbConnect: (name: string, path: string) =>
    call<DbConnection>("db_connect", { name, path }),
  dbConnectionDelete: (id: string) => call<void>("db_connection_delete", { id }),
  dbSchema: (id: string) => call<DbSchema>("db_schema", { id }),
  dbQuery: (id: string, sql: string, allowWrite: boolean) =>
    call<QueryResult>("db_query", { id, sql, allowWrite }),
  dbTableRows: (id: string, table: string, limit: number) =>
    call<QueryResult>("db_table_rows", { id, table, limit }),

  aiMemoryList: (projectPath: string) =>
    call<MemoryEntry[]>("ai_memory_list", { projectPath }),
  aiMemoryAdd: (projectPath: string, content: string) =>
    call<MemoryEntry>("ai_memory_add", { projectPath, content }),
  aiMemoryDelete: (id: string) => call<void>("ai_memory_delete", { id }),
  aiCommitMessage: (path: string, provider: string, model: string) =>
    call<string>("ai_commit_message", { path, provider, model }),

  systemSnapshot: () => call<SystemSnapshot>("system_snapshot"),

  monitorsList: () => call<MonitorStatus[]>("monitors_list"),
  monitorCreate: (name: string, url: string, intervalSecs: number) =>
    call<Monitor>("monitor_create", { name, url, intervalSecs }),
  monitorDelete: (id: string) => call<void>("monitor_delete", { id }),
  monitorToggle: (id: string, enabled: boolean) =>
    call<Monitor>("monitor_toggle", { id, enabled }),
  monitorCheckNow: (id: string) => call<MonitorCheck>("monitor_check_now", { id }),

  deployConfigured: () => call<boolean>("deploy_configured"),
  deployProjects: () => call<DeployProject[]>("deploy_projects"),
  deployList: (projectId: string) =>
    call<Deployment[]>("deploy_list", { projectId }),

  issueConfigured: () => call<boolean>("issue_configured"),
  issueCapture: () => call<CapturedShot>("issue_capture"),
  issueTargets: (projectPath: string) =>
    call<IssueTarget[]>("issue_targets", { projectPath }),
  issueCreate: (owner: string, name: string, title: string, body: string) =>
    call<CreatedIssue>("issue_create", { owner, name, title, body }),
  issueCopyImage: (path: string) => call<void>("issue_copy_image", { path }),

  snippetsList: () => call<Snippet[]>("snippets_list"),
  // A blank query is not a filter — the backend answers it with the full
  // list, so the page has one query for both.
  snippetsSearch: (query: string) =>
    call<Snippet[]>("snippets_search", { query }),
  snippetSave: (draft: SnippetDraft) => call<Snippet>("snippet_save", { draft }),
  snippetDelete: (id: string) => call<void>("snippet_delete", { id }),
};

/** Subscribe to the kernel event stream. Returns an unsubscribe function. */
export function onKernelEvent(
  handler: (event: KernelEvent) => void,
): () => void {
  if (!isDesktopShell()) return () => {};
  let unlisten: UnlistenFn | undefined;
  let cancelled = false;
  void listen<KernelEvent>("devos://event", (e) => handler(e.payload)).then(
    (fn) => {
      if (cancelled) fn();
      else unlisten = fn;
    },
  );
  return () => {
    cancelled = true;
    unlisten?.();
  };
}
