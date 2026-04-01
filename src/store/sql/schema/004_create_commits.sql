CREATE TABLE IF NOT EXISTS commits (
    id INTEGER PRIMARY KEY,
    artifact_id INTEGER NOT NULL,
    sha TEXT NOT NULL,
    message TEXT NOT NULL,
    authored_at TEXT,
    committed_at TEXT,
    FOREIGN KEY (artifact_id) REFERENCES artifacts(id) ON DELETE CASCADE,
    UNIQUE (artifact_id, sha)
)
