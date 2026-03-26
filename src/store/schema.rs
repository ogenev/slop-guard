use sqlx::SqlitePool;

/// Bootstrap schema for the current prototype database.
///
/// This is intentionally simple today and can later be replaced by versioned
/// SQL migrations once we need to preserve existing databases across schema
/// changes.
const STATEMENTS: [&str; 10] = [
    r#"
    CREATE TABLE IF NOT EXISTS accounts (
        id INTEGER PRIMARY KEY,
        username TEXT NOT NULL UNIQUE,
        created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
        updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
    )
    "#,
    r#"
    CREATE TABLE IF NOT EXISTS repositories (
        id INTEGER PRIMARY KEY,
        owner TEXT NOT NULL,
        name TEXT NOT NULL,
        full_name TEXT NOT NULL UNIQUE,
        created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
        updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
    )
    "#,
    r#"
    CREATE TABLE IF NOT EXISTS artifacts (
        id INTEGER PRIMARY KEY,
        account_id INTEGER NOT NULL,
        repository_id INTEGER,
        kind TEXT NOT NULL,
        external_id TEXT NOT NULL,
        pr_number INTEGER,
        title TEXT,
        body TEXT,
        state TEXT,
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL,
        additions INTEGER NOT NULL DEFAULT 0,
        deletions INTEGER NOT NULL DEFAULT 0,
        changed_files INTEGER NOT NULL DEFAULT 0,
        base_branch TEXT,
        head_branch TEXT,
        FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE,
        FOREIGN KEY (repository_id) REFERENCES repositories(id) ON DELETE SET NULL,
        UNIQUE (kind, external_id)
    )
    "#,
    r#"
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
    "#,
    r#"
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
    "#,
    r#"
    CREATE INDEX IF NOT EXISTS idx_accounts_username ON accounts(username)
    "#,
    r#"
    CREATE INDEX IF NOT EXISTS idx_repositories_full_name ON repositories(full_name)
    "#,
    r#"
    CREATE INDEX IF NOT EXISTS idx_artifacts_account_created_at ON artifacts(account_id, created_at DESC)
    "#,
    r#"
    CREATE INDEX IF NOT EXISTS idx_commits_artifact_sha ON commits(artifact_id, sha)
    "#,
    r#"
    CREATE INDEX IF NOT EXISTS idx_sync_runs_account_started_at ON sync_runs(account_id, started_at DESC)
    "#,
];

/// Applies all bootstrap DDL statements to the target SQLite database.
pub async fn apply(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    for statement in STATEMENTS {
        sqlx::query(statement).execute(pool).await?;
    }

    Ok(())
}
