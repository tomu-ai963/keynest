-- 初期スキーマ。キー本体の値はここには保存しない(OSのセキュアストレージが担当)。

CREATE TABLE services (
    id   INTEGER PRIMARY KEY,
    name TEXT NOT NULL UNIQUE
);

CREATE TABLE keys (
    id                INTEGER PRIMARY KEY,
    service_id        INTEGER NOT NULL REFERENCES services(id),
    name              TEXT NOT NULL,
    env_var_name      TEXT NOT NULL,
    created_at        TEXT NOT NULL DEFAULT (datetime('now')),
    expires_at        TEXT,
    last_validated_at TEXT,
    note              TEXT,
    meta              TEXT,
    UNIQUE (service_id, name)
);

CREATE TABLE projects (
    id   INTEGER PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    path TEXT
);

CREATE TABLE key_usages (
    key_id     INTEGER NOT NULL REFERENCES keys(id) ON DELETE CASCADE,
    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    PRIMARY KEY (key_id, project_id)
);
