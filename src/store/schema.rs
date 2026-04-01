use sqlx::SqlitePool;

/// Bootstrap schema for the current prototype database.
///
/// This is intentionally simple today and can later be replaced by versioned
/// SQL migrations once we need to preserve existing databases across schema
/// changes.
const STATEMENTS: [&str; 10] = [
    include_str!("sql/schema/001_create_accounts.sql"),
    include_str!("sql/schema/002_create_repositories.sql"),
    include_str!("sql/schema/003_create_artifacts.sql"),
    include_str!("sql/schema/004_create_commits.sql"),
    include_str!("sql/schema/005_create_sync_runs.sql"),
    include_str!("sql/schema/006_idx_accounts_username.sql"),
    include_str!("sql/schema/007_idx_repositories_full_name.sql"),
    include_str!("sql/schema/008_idx_artifacts_account_created_at.sql"),
    include_str!("sql/schema/009_idx_commits_artifact_sha.sql"),
    include_str!("sql/schema/010_idx_sync_runs_account_started_at.sql"),
];

/// Applies all bootstrap DDL statements to the target SQLite database.
pub async fn apply(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    for statement in STATEMENTS {
        sqlx::query(statement).execute(pool).await?;
    }

    Ok(())
}
