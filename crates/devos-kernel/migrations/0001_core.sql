-- DevOS core schema (M0). Module-owned tables arrive with their modules.

CREATE TABLE workspaces (
    id         TEXT PRIMARY KEY,
    name       TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE projects (
    id           TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    name         TEXT NOT NULL,
    path         TEXT NOT NULL,
    created_at   INTEGER NOT NULL,
    updated_at   INTEGER NOT NULL,
    UNIQUE (workspace_id, path)
);
CREATE INDEX idx_projects_workspace ON projects (workspace_id);

CREATE TABLE settings (
    key        TEXT PRIMARY KEY,
    value      TEXT NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE jobs (
    id          TEXT PRIMARY KEY,
    module      TEXT NOT NULL,
    kind        TEXT NOT NULL,
    status      TEXT NOT NULL,
    payload     TEXT,
    result      TEXT,
    error       TEXT,
    created_at  INTEGER NOT NULL,
    started_at  INTEGER,
    finished_at INTEGER
);
CREATE INDEX idx_jobs_status ON jobs (status);

CREATE TABLE notifications (
    id         TEXT PRIMARY KEY,
    module     TEXT NOT NULL,
    level      TEXT NOT NULL,
    title      TEXT NOT NULL,
    body       TEXT,
    read       INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL
);
CREATE INDEX idx_notifications_read ON notifications (read);

CREATE TABLE audit_log (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    actor      TEXT NOT NULL,
    action     TEXT NOT NULL,
    detail     TEXT,
    created_at INTEGER NOT NULL
);
