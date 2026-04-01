CREATE TABLE IF NOT EXISTS sync_runs (
    id INTEGER PRIMARY KEY,
    account_id INTEGER NOT NULL,
    window_days INTEGER NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('running', 'success', 'failed')),
    started_at TEXT NOT NULL,
    finished_at TEXT,
    artifacts_discovered INTEGER NOT NULL DEFAULT 0,
    artifacts_stored INTEGER NOT NULL DEFAULT 0,
    error_message TEXT,
    FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE
)
