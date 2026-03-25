mod schema;

use std::path::Path;

use anyhow::{Context, Result};
use chrono::Utc;
use sqlx::{
    Row, SqlitePool,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};

#[derive(Clone)]
pub struct Store {
    pool: SqlitePool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountRecord {
    pub id: i64,
    pub login: String,
}

#[derive(Clone, Debug)]
pub struct ArtifactUpsert<'a> {
    pub account_id: i64,
    pub repository_id: Option<i64>,
    pub kind: &'a str,
    pub external_id: &'a str,
    pub pr_number: Option<i64>,
    pub title: Option<&'a str>,
    pub body: Option<&'a str>,
    pub state: Option<&'a str>,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub additions: i64,
    pub deletions: i64,
    pub changed_files: i64,
    pub base_branch: Option<&'a str>,
    pub head_branch: Option<&'a str>,
}

#[derive(Clone, Debug)]
pub struct CommitUpsert<'a> {
    pub artifact_id: i64,
    pub sha: &'a str,
    pub message: &'a str,
    pub authored_at: Option<&'a str>,
    pub committed_at: Option<&'a str>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyncRunStatus {
    Running,
    Success,
    Failed,
}

impl SyncRunStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Success => "success",
            Self::Failed => "failed",
        }
    }
}

impl Store {
    pub async fn connect(database_path: &Path) -> Result<Self> {
        if let Some(parent) = database_path
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent).with_context(|| {
                format!(
                    "failed to create database parent directory at {}",
                    parent.display()
                )
            })?;
        }

        let options = SqliteConnectOptions::new()
            .filename(database_path)
            .create_if_missing(true)
            .foreign_keys(true);

        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .with_context(|| {
                format!(
                    "failed to connect sqlite database at {}",
                    database_path.display()
                )
            })?;

        let store = Self { pool };
        store.init_schema().await?;

        Ok(store)
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    async fn init_schema(&self) -> Result<()> {
        schema::apply(&self.pool)
            .await
            .context("failed to initialize sqlite schema")
    }

    pub async fn upsert_account(&self, login: &str) -> Result<i64> {
        let id = sqlx::query_scalar::<_, i64>(
            r#"
            INSERT INTO accounts (login)
            VALUES (?)
            ON CONFLICT(login) DO UPDATE SET
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
            RETURNING id
            "#,
        )
        .bind(login)
        .fetch_one(&self.pool)
        .await
        .with_context(|| format!("failed to upsert account with login {login}"))?;

        Ok(id)
    }

    pub async fn find_account_by_login(&self, login: &str) -> Result<Option<AccountRecord>> {
        let row = sqlx::query(
            r#"
            SELECT id, login
            FROM accounts
            WHERE login = ?
            "#,
        )
        .bind(login)
        .fetch_optional(&self.pool)
        .await
        .with_context(|| format!("failed to lookup account with login {login}"))?;

        Ok(row.map(|row| AccountRecord {
            id: row.get("id"),
            login: row.get("login"),
        }))
    }

    pub async fn upsert_repository(&self, owner: &str, name: &str) -> Result<i64> {
        let full_name = format!("{owner}/{name}");

        let id = sqlx::query_scalar::<_, i64>(
            r#"
            INSERT INTO repositories (owner, name, full_name)
            VALUES (?, ?, ?)
            ON CONFLICT(full_name) DO UPDATE SET
                owner = excluded.owner,
                name = excluded.name,
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
            RETURNING id
            "#,
        )
        .bind(owner)
        .bind(name)
        .bind(full_name)
        .fetch_one(&self.pool)
        .await
        .with_context(|| format!("failed to upsert repository {owner}/{name}"))?;

        Ok(id)
    }

    pub async fn upsert_artifact(&self, input: &ArtifactUpsert<'_>) -> Result<i64> {
        let id = sqlx::query_scalar::<_, i64>(
            r#"
            INSERT INTO artifacts (
                account_id,
                repository_id,
                kind,
                external_id,
                pr_number,
                title,
                body,
                state,
                created_at,
                updated_at,
                additions,
                deletions,
                changed_files,
                base_branch,
                head_branch
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(kind, external_id) DO UPDATE SET
                account_id = excluded.account_id,
                repository_id = excluded.repository_id,
                pr_number = excluded.pr_number,
                title = excluded.title,
                body = excluded.body,
                state = excluded.state,
                created_at = excluded.created_at,
                updated_at = excluded.updated_at,
                additions = excluded.additions,
                deletions = excluded.deletions,
                changed_files = excluded.changed_files,
                base_branch = excluded.base_branch,
                head_branch = excluded.head_branch
            RETURNING id
            "#,
        )
        .bind(input.account_id)
        .bind(input.repository_id)
        .bind(input.kind)
        .bind(input.external_id)
        .bind(input.pr_number)
        .bind(input.title)
        .bind(input.body)
        .bind(input.state)
        .bind(input.created_at)
        .bind(input.updated_at)
        .bind(input.additions)
        .bind(input.deletions)
        .bind(input.changed_files)
        .bind(input.base_branch)
        .bind(input.head_branch)
        .fetch_one(&self.pool)
        .await
        .with_context(|| {
            format!(
                "failed to upsert artifact kind={} external_id={}",
                input.kind, input.external_id
            )
        })?;

        Ok(id)
    }

    pub async fn upsert_commit(&self, input: &CommitUpsert<'_>) -> Result<i64> {
        let id = sqlx::query_scalar::<_, i64>(
            r#"
            INSERT INTO commits (
                artifact_id,
                sha,
                message,
                authored_at,
                committed_at
            )
            VALUES (?, ?, ?, ?, ?)
            ON CONFLICT(artifact_id, sha) DO UPDATE SET
                message = excluded.message,
                authored_at = excluded.authored_at,
                committed_at = excluded.committed_at
            RETURNING id
            "#,
        )
        .bind(input.artifact_id)
        .bind(input.sha)
        .bind(input.message)
        .bind(input.authored_at)
        .bind(input.committed_at)
        .fetch_one(&self.pool)
        .await
        .with_context(|| format!("failed to upsert commit {}", input.sha))?;

        Ok(id)
    }

    pub async fn start_sync_run(&self, account_id: i64, window_days: u16) -> Result<i64> {
        let started_at = Utc::now().to_rfc3339();

        let run_id = sqlx::query_scalar::<_, i64>(
            r#"
            INSERT INTO sync_runs (
                account_id,
                window_days,
                status,
                started_at
            )
            VALUES (?, ?, ?, ?)
            RETURNING id
            "#,
        )
        .bind(account_id)
        .bind(i64::from(window_days))
        .bind(SyncRunStatus::Running.as_str())
        .bind(started_at)
        .fetch_one(&self.pool)
        .await
        .with_context(|| {
            format!(
                "failed to create sync run for account_id={} window_days={}",
                account_id, window_days
            )
        })?;

        Ok(run_id)
    }

    pub async fn finish_sync_run(
        &self,
        run_id: i64,
        status: SyncRunStatus,
        artifacts_discovered: i64,
        artifacts_stored: i64,
        error_message: Option<&str>,
    ) -> Result<()> {
        let finished_at = Utc::now().to_rfc3339();

        sqlx::query(
            r#"
            UPDATE sync_runs
            SET status = ?,
                finished_at = ?,
                artifacts_discovered = ?,
                artifacts_stored = ?,
                error_message = ?
            WHERE id = ?
            "#,
        )
        .bind(status.as_str())
        .bind(finished_at)
        .bind(artifacts_discovered)
        .bind(artifacts_stored)
        .bind(error_message)
        .bind(run_id)
        .execute(&self.pool)
        .await
        .with_context(|| format!("failed to finish sync run {run_id}"))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::{Store, SyncRunStatus};

    #[tokio::test]
    async fn schema_init_is_idempotent() {
        let temp_dir = TempDir::new().expect("temp dir should be created");
        let db_path = temp_dir.path().join("aislop.db");

        let first = Store::connect(&db_path)
            .await
            .expect("first schema init should work");
        let _second = Store::connect(&db_path)
            .await
            .expect("second schema init should be idempotent");

        let table_count: i64 = sqlx::query_scalar(
            "
            SELECT COUNT(*)
            FROM sqlite_master
            WHERE type = 'table'
              AND name IN ('accounts', 'repositories', 'artifacts', 'commits', 'sync_runs')
            ",
        )
        .fetch_one(first.pool())
        .await
        .expect("schema tables should be queryable");

        assert_eq!(table_count, 5);
    }

    #[tokio::test]
    async fn account_upsert_returns_stable_identifier() {
        let temp_dir = TempDir::new().expect("temp dir should be created");
        let db_path = temp_dir.path().join("aislop.db");
        let store = Store::connect(&db_path)
            .await
            .expect("store should connect for test");

        let first_id = store
            .upsert_account("ogi")
            .await
            .expect("first account upsert should work");
        let second_id = store
            .upsert_account("ogi")
            .await
            .expect("second account upsert should work");

        assert_eq!(first_id, second_id);

        let account = store
            .find_account_by_login("ogi")
            .await
            .expect("account lookup should work")
            .expect("account should exist");

        assert_eq!(account.id, first_id);
        assert_eq!(account.login, "ogi");
    }

    #[tokio::test]
    async fn sync_run_lifecycle_is_recorded() {
        let temp_dir = TempDir::new().expect("temp dir should be created");
        let db_path = temp_dir.path().join("aislop.db");
        let store = Store::connect(&db_path)
            .await
            .expect("store should connect for test");

        let account_id = store
            .upsert_account("ogi")
            .await
            .expect("account upsert should work");
        let run_id = store
            .start_sync_run(account_id, 90)
            .await
            .expect("sync run should start");

        store
            .finish_sync_run(run_id, SyncRunStatus::Success, 0, 0, None)
            .await
            .expect("sync run should finish");

        let status: String = sqlx::query_scalar("SELECT status FROM sync_runs WHERE id = ?")
            .bind(run_id)
            .fetch_one(store.pool())
            .await
            .expect("sync run status should be queryable");

        let finished_at: Option<String> =
            sqlx::query_scalar("SELECT finished_at FROM sync_runs WHERE id = ?")
                .bind(run_id)
                .fetch_one(store.pool())
                .await
                .expect("sync run completion time should be queryable");

        assert_eq!(status, "success");
        assert!(finished_at.is_some());
    }
}
