CREATE TABLE IF NOT EXISTS file_nodes (
    id TEXT PRIMARY KEY,
    remote_id TEXT,
    path TEXT UNIQUE NOT NULL,
    state TEXT NOT NULL,
    size INTEGER NOT NULL DEFAULT 0,
    modified TEXT NOT NULL,
    is_dir INTEGER NOT NULL DEFAULT 0,
    parent_id TEXT,
    FOREIGN KEY (parent_id) REFERENCES file_nodes(id)
);

CREATE INDEX IF NOT EXISTS idx_path ON file_nodes(path);
CREATE INDEX IF NOT EXISTS idx_state ON file_nodes(state);
