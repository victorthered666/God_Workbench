from __future__ import annotations

import json
import sqlite3
import threading
import uuid
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

from .config import DB_PATH

SCHEMA = """
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
"""


def now() -> str:
    return datetime.now(timezone.utc).isoformat()


class Repository:
    def __init__(self, path: Path = DB_PATH):
        self.path = Path(path)
        self.path.parent.mkdir(parents=True, exist_ok=True)
        self.lock = threading.RLock()
        with self.connect() as db:
            db.executescript(SCHEMA)
        self.recover_interrupted()

    @staticmethod
    def dumps(value: Any, **kwargs: Any) -> str:
        return json.dumps(value, ensure_ascii=False, **kwargs)

    def connect(self) -> sqlite3.Connection:
        db = sqlite3.connect(self.path, timeout=30)
        db.row_factory = sqlite3.Row
        db.execute("PRAGMA foreign_keys=ON")
        db.execute("PRAGMA journal_mode=WAL")
        return db

    @staticmethod
    def row(row: sqlite3.Row | None) -> dict[str, Any] | None:
        if row is None:
            return None
        data = dict(row)
        for key in ("artifacts", "snapshot", "payload", "metadata"):
            if key in data:
                data[key] = json.loads(data[key])
        return data

    def create_project(self, data: Any) -> dict[str, Any]:
        values = data.model_dump()
        values["reference_text"] = values.pop("references")
        project = {"id": uuid.uuid4().hex, **values, "status": "research_review",
                   "revision": 1, "review_round": 0, "artifacts": {},
                   "created_at": now(), "updated_at": now()}
        with self.connect() as db:
            db.execute("INSERT INTO projects VALUES (:id,:name,:purpose,:reference_text,:platform,:audience,:status,:revision,:review_round,:artifacts,NULL,:created_at,:updated_at)",
                       {**project, "artifacts": "{}"})
        return project

    def get(self, project_id: str) -> dict[str, Any] | None:
        with self.connect() as db:
            return self.row(db.execute("SELECT * FROM projects WHERE id=?", (project_id,)).fetchone())

    def list_projects(self) -> list[dict[str, Any]]:
        with self.connect() as db:
            rows = db.execute("SELECT id,name,purpose,reference_text,platform,audience,status,revision,review_round,error,created_at,updated_at FROM projects ORDER BY updated_at DESC LIMIT 200").fetchall()
        projects = [dict(row) for row in rows]
        for project in projects:
            with self.connect() as db:
                row = db.execute("SELECT content FROM versions WHERE project_id=? AND stage='review' ORDER BY created_at DESC LIMIT 1", (project["id"],)).fetchone()
            project["artifacts"] = {"review": {"content": json.loads(row["content"])} if row else {}}
        return projects

    def update(self, project_id: str, **fields: Any) -> None:
        if "artifacts" in fields:
            fields["artifacts"] = json.dumps(fields["artifacts"], ensure_ascii=False)
        fields["updated_at"] = now()
        with self.lock, self.connect() as db:
            db.execute(f"UPDATE projects SET {','.join(f'{k}=?' for k in fields)} WHERE id=?",
                       (*fields.values(), project_id))

    def save_artifact(self, project_id: str, stage: str, content: Any,
                      feedback: str = "") -> int:
        with self.lock, self.connect() as db:
            project = self.get(project_id)
            artifacts = project["artifacts"]
            version = (artifacts.get(stage, {}).get("version", 0)) + 1
            record = {"version": version, "content": content, "approved": False,
                      "feedback": feedback, "created_at": now()}
            artifacts[stage] = record
            db.execute("INSERT INTO versions VALUES (?,?,?,?,?,?,?)",
                       (uuid.uuid4().hex, project_id, stage, version,
                        json.dumps(content, ensure_ascii=False), feedback, now()))
            db.execute("UPDATE projects SET artifacts=?,updated_at=? WHERE id=?",
                       (json.dumps(artifacts, ensure_ascii=False), now(), project_id))
        return version

    def approve(self, project_id: str, stage: str) -> None:
        with self.lock:
            project = self.get(project_id)
            if not project or stage not in project["artifacts"]:
                raise ValueError("当前阶段尚无产物")
            artifacts = project["artifacts"]
            artifacts[stage]["approved"] = True
            self.update(project_id, artifacts=artifacts, error=None)

    def versions(self, project_id: str, limit: int = 50, offset: int = 0) -> list[dict[str, Any]]:
        with self.connect() as db:
            rows = db.execute("SELECT * FROM versions WHERE project_id=? ORDER BY created_at DESC LIMIT ? OFFSET ?", (project_id, limit, offset)).fetchall()
        return [{**dict(r), "content": json.loads(r["content"])} for r in reversed(rows)]

    def all_versions(self, project_id: str) -> list[dict[str, Any]]:
        with self.connect() as db:
            rows = db.execute("SELECT * FROM versions WHERE project_id=? ORDER BY created_at", (project_id,)).fetchall()
        return [{**dict(row), "content": json.loads(row["content"])} for row in rows]

    def create_run(self, project_id: str, stage: str, snapshot: Any) -> str:
        run_id = uuid.uuid4().hex
        with self.connect() as db:
            db.execute("INSERT INTO runs VALUES (?,?,?,'running',?,NULL,?,?)",
                       (run_id, project_id, stage, json.dumps(snapshot, ensure_ascii=False), now(), now()))
        return run_id

    def finish_run(self, run_id: str, error: str | None = None) -> None:
        with self.connect() as db:
            db.execute("UPDATE runs SET status=?,error=?,updated_at=? WHERE id=?",
                       ("failed" if error else "completed", error, now(), run_id))

    def recover_interrupted(self) -> None:
        with self.connect() as db:
            db.execute("UPDATE runs SET status='interrupted',error='服务重启；请检查日志后恢复，未自动重发外部请求。',updated_at=? WHERE status='running'", (now(),))
            db.execute("UPDATE projects SET status='interrupted',error='任务被中断；可从当前阶段恢复。',updated_at=? WHERE status LIKE 'running:%'", (now(),))

    def event(self, project_id: str, kind: str, message: str) -> None:
        with self.connect() as db:
            db.execute("INSERT INTO events(project_id,kind,message,created_at) VALUES (?,?,?,?)",
                       (project_id, kind, message, now()))

    def events(self, project_id: str) -> list[dict[str, Any]]:
        with self.connect() as db:
            return [dict(r) for r in db.execute("SELECT * FROM events WHERE project_id=? ORDER BY id DESC LIMIT 200", (project_id,))]

    def setting(self, key: str, default: Any = None) -> Any:
        with self.connect() as db:
            row = db.execute("SELECT value FROM settings WHERE key=?", (key,)).fetchone()
        return json.loads(row["value"]) if row else default

    def set_setting(self, key: str, value: Any) -> None:
        with self.connect() as db:
            db.execute("INSERT INTO settings VALUES (?,?) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                       (key, json.dumps(value, ensure_ascii=False)))

    def evidence(self, project_id: str, query: str, payload: Any) -> None:
        with self.connect() as db:
            db.execute("INSERT INTO evidence VALUES (?,?,?,?,?)", (uuid.uuid4().hex, project_id, query, json.dumps(payload, ensure_ascii=False), now()))

    def evidence_list(self, project_id: str, limit: int = 100, offset: int = 0) -> list[dict[str, Any]]:
        with self.connect() as db:
            return [self.row(r) for r in db.execute("SELECT * FROM evidence WHERE project_id=? ORDER BY created_at DESC LIMIT ? OFFSET ?", (project_id, limit, offset))]

    def asset(self, project_id: str, source_url: str, source_page: str,
              local_path: str, sha256: str, metadata: Any) -> None:
        with self.connect() as db:
            db.execute("INSERT INTO assets VALUES (?,?,?,?,?,?,?,?)", (uuid.uuid4().hex, project_id, source_url, source_page, local_path, sha256, json.dumps(metadata, ensure_ascii=False), now()))

    def assets(self, project_id: str) -> list[dict[str, Any]]:
        with self.connect() as db:
            return [self.row(r) for r in db.execute("SELECT * FROM assets WHERE project_id=? ORDER BY created_at", (project_id,))]
