CREATE TABLE IF NOT EXISTS projects (
    id TEXT PRIMARY KEY, name TEXT NOT NULL, purpose TEXT NOT NULL,
    reference_text TEXT NOT NULL, platform TEXT NOT NULL DEFAULT '',
    audience TEXT NOT NULL DEFAULT '', status TEXT NOT NULL,
    revision INTEGER NOT NULL DEFAULT 1, review_round INTEGER NOT NULL DEFAULT 0,
    artifacts TEXT NOT NULL DEFAULT '{}', error TEXT,
    created_at TEXT NOT NULL, updated_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS versions (
    id TEXT PRIMARY KEY, project_id TEXT NOT NULL, stage TEXT NOT NULL,
    version INTEGER NOT NULL, content TEXT NOT NULL, feedback TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS runs (
    id TEXT PRIMARY KEY, project_id TEXT NOT NULL, stage TEXT NOT NULL,
    status TEXT NOT NULL, snapshot TEXT NOT NULL DEFAULT '{}', error TEXT,
    created_at TEXT NOT NULL, updated_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS events (
    id INTEGER PRIMARY KEY AUTOINCREMENT, project_id TEXT NOT NULL,
    kind TEXT NOT NULL, message TEXT NOT NULL, created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS settings (
    key TEXT PRIMARY KEY, value TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS evidence (
    id TEXT PRIMARY KEY, project_id TEXT NOT NULL, query TEXT NOT NULL,
    payload TEXT NOT NULL, created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS assets (
    id TEXT PRIMARY KEY, project_id TEXT NOT NULL, source_url TEXT NOT NULL,
    source_page TEXT NOT NULL, local_path TEXT NOT NULL, sha256 TEXT NOT NULL,
    metadata TEXT NOT NULL DEFAULT '{}', created_at TEXT NOT NULL
);
