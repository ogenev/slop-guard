pub(crate) mod queries;
mod schema;
#[cfg(test)]
pub(crate) mod test_queries;

use std::path::Path;

use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use sqlx::{
    Row, SqlitePool,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};

const PULL_REQUEST_ARTIFACT_KIND: &str = "pull_request";

/// Thin SQLite-backed persistence layer for accounts, artifacts, commits, and sync runs.
#[derive(Clone)]
pub struct Store {
    pool: SqlitePool,
}

/// Minimal account row returned by lookup helpers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountRecord {
    pub id: i64,
    pub username: String,
}

/// Input payload for inserting or updating one normalized artifact row.
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

/// Input payload for inserting or updating one commit row.
#[derive(Clone, Debug)]
pub struct CommitUpsert<'a> {
    pub artifact_id: i64,
    pub sha: &'a str,
    pub message: &'a str,
    pub authored_at: Option<&'a str>,
    pub committed_at: Option<&'a str>,
}

/// Typed pull-request shape loaded back from SQLite for analysis and scoring.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PullRequestReadModel {
    pub artifact_id: i64,
    pub account_id: i64,
    pub username: String,
    pub repository_owner: String,
    pub repository_name: String,
    pub repository_full_name: String,
    pub external_id: String,
    pub number: i64,
    pub title: String,
    pub body: Option<String>,
    pub state: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub additions: i64,
    pub deletions: i64,
    pub changed_files: i64,
    pub base_branch: Option<String>,
    pub head_branch: Option<String>,
    pub commits: Vec<PullRequestCommitReadModel>,
}

/// Typed commit shape attached to one stored pull-request read model.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PullRequestCommitReadModel {
    pub sha: String,
    pub message: String,
    pub authored_at: Option<DateTime<Utc>>,
    pub committed_at: Option<DateTime<Utc>>,
}

/// Lifecycle state persisted for a sync run.
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

fn parse_timestamp(value: &str) -> Result<DateTime<Utc>> {
    let parsed = DateTime::parse_from_rfc3339(value)
        .with_context(|| format!("invalid RFC3339 timestamp: {value}"))?;

    Ok(parsed.with_timezone(&Utc))
}

fn parse_optional_timestamp(value: Option<String>) -> Result<Option<DateTime<Utc>>> {
    value.map(|value| parse_timestamp(&value)).transpose()
}

impl Store {
    /// Opens the SQLite database, creating parent directories and bootstrapping the schema.
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

    /// Exposes the underlying pool for integration tests and focused queries.
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    /// Applies the current bootstrap schema to a newly opened database.
    async fn init_schema(&self) -> Result<()> {
        schema::apply(&self.pool)
            .await
            .context("failed to initialize sqlite schema")
    }

    /// Inserts or refreshes an account row and returns its stable identifier.
    pub async fn upsert_account(&self, username: &str) -> Result<i64> {
        let id = sqlx::query_scalar::<_, i64>(queries::UPSERT_ACCOUNT_QUERY)
            .bind(username)
            .fetch_one(&self.pool)
            .await
            .with_context(|| format!("failed to upsert account with username {username}"))?;

        Ok(id)
    }

    /// Looks up an account by username if it has already been seen locally.
    pub async fn find_account_by_username(&self, username: &str) -> Result<Option<AccountRecord>> {
        let row = sqlx::query(queries::FIND_ACCOUNT_BY_USERNAME_QUERY)
            .bind(username)
            .fetch_optional(&self.pool)
            .await
            .with_context(|| format!("failed to lookup account with username {username}"))?;

        Ok(row.map(|row| AccountRecord {
            id: row.get("id"),
            username: row.get("username"),
        }))
    }

    /// Inserts or refreshes a repository row and returns its stable identifier.
    pub async fn upsert_repository(&self, owner: &str, name: &str) -> Result<i64> {
        let full_name = format!("{owner}/{name}");

        let id = sqlx::query_scalar::<_, i64>(queries::UPSERT_REPOSITORY_QUERY)
            .bind(owner)
            .bind(name)
            .bind(full_name)
            .fetch_one(&self.pool)
            .await
            .with_context(|| format!("failed to upsert repository {owner}/{name}"))?;

        Ok(id)
    }

    /// Inserts or refreshes a normalized artifact and returns its row identifier.
    pub async fn upsert_artifact(&self, input: &ArtifactUpsert<'_>) -> Result<i64> {
        let id = sqlx::query_scalar::<_, i64>(queries::UPSERT_ARTIFACT_QUERY)
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

    /// Inserts or refreshes a commit row for an artifact and returns its row identifier.
    pub async fn upsert_commit(&self, input: &CommitUpsert<'_>) -> Result<i64> {
        let id = sqlx::query_scalar::<_, i64>(queries::UPSERT_COMMIT_QUERY)
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

    /// Deletes all commits currently associated with an artifact.
    pub async fn delete_commits_for_artifact(&self, artifact_id: i64) -> Result<()> {
        sqlx::query(queries::DELETE_COMMITS_FOR_ARTIFACT_QUERY)
            .bind(artifact_id)
            .execute(&self.pool)
            .await
            .with_context(|| format!("failed to delete commits for artifact {artifact_id}"))?;

        Ok(())
    }

    /// Loads stored pull requests for one account inside the requested trailing window.
    ///
    /// This is the read-model entrypoint for the Phase 3 analyze-on-read flow. It
    /// returns fully hydrated pull requests, including their linked commits, so the
    /// scoring path can derive features without persisting analyzer output.
    pub async fn load_pull_requests_for_account_window(
        &self,
        username: &str,
        window_days: u16,
    ) -> Result<Vec<PullRequestReadModel>> {
        let Some(account) = self.find_account_by_username(username).await? else {
            return Ok(Vec::new());
        };

        let cutoff = (Utc::now() - Duration::days(i64::from(window_days))).to_rfc3339();
        let rows = sqlx::query(queries::LOAD_PULL_REQUESTS_FOR_ACCOUNT_WINDOW_QUERY)
            .bind(account.id)
            .bind(PULL_REQUEST_ARTIFACT_KIND)
            .bind(cutoff)
            .fetch_all(&self.pool)
            .await
            .with_context(|| {
                format!(
                    "failed to load pull requests for username {} in {} day window",
                    username, window_days
                )
            })?;

        let mut pull_requests = Vec::with_capacity(rows.len());

        for row in rows {
            let artifact_id = row.get::<i64, _>("artifact_id");
            let repository_owner =
                row.get::<Option<String>, _>("repository_owner")
                    .context(format!(
                        "pull request artifact {artifact_id} is missing repository owner"
                    ))?;
            let repository_name =
                row.get::<Option<String>, _>("repository_name")
                    .context(format!(
                        "pull request artifact {artifact_id} is missing repository name"
                    ))?;
            let repository_full_name = row
                .get::<Option<String>, _>("repository_full_name")
                .context(format!(
                    "pull request artifact {artifact_id} is missing repository full name"
                ))?;
            let number = row.get::<Option<i64>, _>("pr_number").context(format!(
                "pull request artifact {artifact_id} is missing pr_number"
            ))?;
            let title = row.get::<Option<String>, _>("title").context(format!(
                "pull request artifact {artifact_id} is missing title"
            ))?;
            let state = row.get::<Option<String>, _>("state").context(format!(
                "pull request artifact {artifact_id} is missing state"
            ))?;
            let created_at_raw = row.get::<String, _>("created_at");
            let updated_at_raw = row.get::<String, _>("updated_at");
            let commits = self.load_commits_for_artifact(artifact_id).await?;

            pull_requests.push(PullRequestReadModel {
                artifact_id,
                account_id: row.get("account_id"),
                username: row.get("username"),
                repository_owner,
                repository_name,
                repository_full_name,
                external_id: row.get("external_id"),
                number,
                title,
                body: row.get("body"),
                state,
                created_at: parse_timestamp(&created_at_raw).with_context(|| {
                    format!("failed to parse created_at for pull request artifact {artifact_id}")
                })?,
                updated_at: parse_timestamp(&updated_at_raw).with_context(|| {
                    format!("failed to parse updated_at for pull request artifact {artifact_id}")
                })?,
                additions: row.get("additions"),
                deletions: row.get("deletions"),
                changed_files: row.get("changed_files"),
                base_branch: row.get("base_branch"),
                head_branch: row.get("head_branch"),
                commits,
            });
        }

        Ok(pull_requests)
    }

    /// Loads the stored commit set for one artifact in a stable analyzer-friendly order.
    async fn load_commits_for_artifact(
        &self,
        artifact_id: i64,
    ) -> Result<Vec<PullRequestCommitReadModel>> {
        let rows = sqlx::query(queries::LOAD_COMMITS_FOR_ARTIFACT_QUERY)
            .bind(artifact_id)
            .fetch_all(&self.pool)
            .await
            .with_context(|| format!("failed to load commits for artifact {artifact_id}"))?;

        let mut commits = Vec::with_capacity(rows.len());

        for row in rows {
            let sha = row.get::<String, _>("sha");
            let authored_at_raw = row.get::<Option<String>, _>("authored_at");
            let committed_at_raw = row.get::<Option<String>, _>("committed_at");

            commits.push(PullRequestCommitReadModel {
                sha: sha.clone(),
                message: row.get("message"),
                authored_at: parse_optional_timestamp(authored_at_raw).with_context(|| {
                    format!(
                        "failed to parse authored_at for commit {sha} on artifact {artifact_id}"
                    )
                })?,
                committed_at: parse_optional_timestamp(committed_at_raw).with_context(|| {
                    format!(
                        "failed to parse committed_at for commit {sha} on artifact {artifact_id}"
                    )
                })?,
            });
        }

        Ok(commits)
    }

    /// Creates a new sync-run record before remote ingestion starts.
    pub async fn start_sync_run(&self, account_id: i64, window_days: u16) -> Result<i64> {
        let started_at = Utc::now().to_rfc3339();

        let run_id = sqlx::query_scalar::<_, i64>(queries::START_SYNC_RUN_QUERY)
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

    /// Marks a sync run as finished and records aggregate outcome counters.
    pub async fn finish_sync_run(
        &self,
        run_id: i64,
        status: SyncRunStatus,
        artifacts_discovered: i64,
        artifacts_stored: i64,
        error_message: Option<&str>,
    ) -> Result<()> {
        let finished_at = Utc::now().to_rfc3339();

        sqlx::query(queries::FINISH_SYNC_RUN_QUERY)
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
    use chrono::{Duration, Utc};
    use tempfile::TempDir;

    use super::{
        ArtifactUpsert, CommitUpsert, PullRequestReadModel, Store, SyncRunStatus, test_queries,
    };

    fn relative_timestamp(days_from_now: i64) -> String {
        (Utc::now() + Duration::days(days_from_now)).to_rfc3339()
    }

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

        let table_count: i64 = sqlx::query_scalar(test_queries::COUNT_SCHEMA_TABLES_QUERY)
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
            .find_account_by_username("ogi")
            .await
            .expect("account lookup should work")
            .expect("account should exist");

        assert_eq!(account.id, first_id);
        assert_eq!(account.username, "ogi");
    }

    #[tokio::test]
    async fn artifact_commit_sets_can_be_replaced() {
        let temp_dir = TempDir::new().expect("temp dir should be created");
        let db_path = temp_dir.path().join("aislop.db");
        let store = Store::connect(&db_path)
            .await
            .expect("store should connect for test");

        let account_id = store
            .upsert_account("ogi")
            .await
            .expect("account upsert should work");
        let repository_id = store
            .upsert_repository("rust-lang", "cargo")
            .await
            .expect("repository upsert should work");
        let artifact_id = store
            .upsert_artifact(&ArtifactUpsert {
                account_id,
                repository_id: Some(repository_id),
                kind: "pull_request",
                external_id: "9001",
                pr_number: Some(42),
                title: Some("Improve parser coverage"),
                body: Some("Adds regression tests and cleanup."),
                state: Some("open"),
                created_at: "2026-03-01T10:00:00Z",
                updated_at: "2026-03-01T11:00:00Z",
                additions: 17,
                deletions: 4,
                changed_files: 3,
                base_branch: Some("main"),
                head_branch: Some("topic/coverage"),
            })
            .await
            .expect("artifact upsert should work");

        store
            .upsert_commit(&CommitUpsert {
                artifact_id,
                sha: "abc123",
                message: "test: add parser regression",
                authored_at: Some("2026-03-01T09:00:00Z"),
                committed_at: Some("2026-03-01T09:05:00Z"),
            })
            .await
            .expect("first commit upsert should work");
        store
            .upsert_commit(&CommitUpsert {
                artifact_id,
                sha: "def456",
                message: "refactor: simplify fixtures",
                authored_at: Some("2026-03-01T09:10:00Z"),
                committed_at: Some("2026-03-01T09:15:00Z"),
            })
            .await
            .expect("second commit upsert should work");

        store
            .delete_commits_for_artifact(artifact_id)
            .await
            .expect("commit set deletion should work");
        store
            .upsert_commit(&CommitUpsert {
                artifact_id,
                sha: "fedcba",
                message: "fix: keep only latest commit set",
                authored_at: Some("2026-03-01T09:20:00Z"),
                committed_at: Some("2026-03-01T09:25:00Z"),
            })
            .await
            .expect("replacement commit upsert should work");

        let commit_count: i64 = sqlx::query_scalar(test_queries::COUNT_COMMITS_FOR_ARTIFACT_QUERY)
            .bind(artifact_id)
            .fetch_one(store.pool())
            .await
            .expect("commit count should be queryable");
        let remaining_sha: String =
            sqlx::query_scalar(test_queries::SELECT_COMMIT_SHA_FOR_ARTIFACT_QUERY)
                .bind(artifact_id)
                .fetch_one(store.pool())
                .await
                .expect("remaining commit should be queryable");

        assert_eq!(commit_count, 1);
        assert_eq!(remaining_sha, "fedcba");
    }

    #[tokio::test]
    async fn load_pull_requests_for_account_window_hydrates_commits() {
        let temp_dir = TempDir::new().expect("temp dir should be created");
        let db_path = temp_dir.path().join("aislop.db");
        let store = Store::connect(&db_path)
            .await
            .expect("store should connect for test");

        let account_id = store
            .upsert_account("ogi")
            .await
            .expect("account upsert should work");
        let other_account_id = store
            .upsert_account("someone-else")
            .await
            .expect("other account upsert should work");
        let repository_id = store
            .upsert_repository("rust-lang", "cargo")
            .await
            .expect("repository upsert should work");
        let other_repository_id = store
            .upsert_repository("rust-lang", "rust")
            .await
            .expect("other repository upsert should work");

        let current_artifact_id = store
            .upsert_artifact(&ArtifactUpsert {
                account_id,
                repository_id: Some(repository_id),
                kind: "pull_request",
                external_id: "9001",
                pr_number: Some(42),
                title: Some("Improve parser coverage"),
                body: Some("Adds regression tests and cleanup."),
                state: Some("open"),
                created_at: &relative_timestamp(-5),
                updated_at: &relative_timestamp(-4),
                additions: 17,
                deletions: 4,
                changed_files: 3,
                base_branch: Some("main"),
                head_branch: Some("topic/coverage"),
            })
            .await
            .expect("current artifact upsert should work");
        let _older_artifact_id = store
            .upsert_artifact(&ArtifactUpsert {
                account_id,
                repository_id: Some(repository_id),
                kind: "pull_request",
                external_id: "9002",
                pr_number: Some(43),
                title: Some("Old pull request"),
                body: Some("Outside the current score window."),
                state: Some("closed"),
                created_at: &relative_timestamp(-120),
                updated_at: &relative_timestamp(-119),
                additions: 3,
                deletions: 1,
                changed_files: 1,
                base_branch: Some("main"),
                head_branch: Some("topic/old"),
            })
            .await
            .expect("older artifact upsert should work");
        let _issue_artifact_id = store
            .upsert_artifact(&ArtifactUpsert {
                account_id,
                repository_id: Some(repository_id),
                kind: "issue",
                external_id: "issue-1",
                pr_number: None,
                title: Some("Not a pull request"),
                body: Some("Should not show up in the pull-request read model."),
                state: Some("open"),
                created_at: &relative_timestamp(-2),
                updated_at: &relative_timestamp(-2),
                additions: 0,
                deletions: 0,
                changed_files: 0,
                base_branch: None,
                head_branch: None,
            })
            .await
            .expect("issue artifact upsert should work");
        let _other_account_artifact_id = store
            .upsert_artifact(&ArtifactUpsert {
                account_id: other_account_id,
                repository_id: Some(other_repository_id),
                kind: "pull_request",
                external_id: "9003",
                pr_number: Some(44),
                title: Some("Other account pull request"),
                body: Some("Should not show up for the requested account."),
                state: Some("open"),
                created_at: &relative_timestamp(-1),
                updated_at: &relative_timestamp(-1),
                additions: 9,
                deletions: 2,
                changed_files: 2,
                base_branch: Some("main"),
                head_branch: Some("topic/other"),
            })
            .await
            .expect("other account artifact upsert should work");

        store
            .upsert_commit(&CommitUpsert {
                artifact_id: current_artifact_id,
                sha: "abc123",
                message: "test: add parser regression",
                authored_at: Some(&relative_timestamp(-6)),
                committed_at: Some(&relative_timestamp(-6)),
            })
            .await
            .expect("first commit upsert should work");
        store
            .upsert_commit(&CommitUpsert {
                artifact_id: current_artifact_id,
                sha: "def456",
                message: "refactor: simplify fixtures",
                authored_at: Some(&relative_timestamp(-5)),
                committed_at: Some(&relative_timestamp(-5)),
            })
            .await
            .expect("second commit upsert should work");

        let pull_requests = store
            .load_pull_requests_for_account_window("ogi", 90)
            .await
            .expect("pull requests should load");

        assert_eq!(pull_requests.len(), 1);

        let pull_request: &PullRequestReadModel = &pull_requests[0];
        assert_eq!(pull_request.artifact_id, current_artifact_id);
        assert_eq!(pull_request.username, "ogi");
        assert_eq!(pull_request.repository_owner, "rust-lang");
        assert_eq!(pull_request.repository_name, "cargo");
        assert_eq!(pull_request.repository_full_name, "rust-lang/cargo");
        assert_eq!(pull_request.number, 42);
        assert_eq!(pull_request.title, "Improve parser coverage");
        assert_eq!(
            pull_request.body.as_deref(),
            Some("Adds regression tests and cleanup.")
        );
        assert_eq!(pull_request.state, "open");
        assert_eq!(pull_request.additions, 17);
        assert_eq!(pull_request.deletions, 4);
        assert_eq!(pull_request.changed_files, 3);
        assert_eq!(pull_request.base_branch.as_deref(), Some("main"));
        assert_eq!(pull_request.head_branch.as_deref(), Some("topic/coverage"));
        assert_eq!(pull_request.commits.len(), 2);
        assert_eq!(pull_request.commits[0].sha, "abc123");
        assert_eq!(pull_request.commits[1].sha, "def456");
    }

    #[tokio::test]
    async fn load_pull_requests_for_unknown_account_returns_empty() {
        let temp_dir = TempDir::new().expect("temp dir should be created");
        let db_path = temp_dir.path().join("aislop.db");
        let store = Store::connect(&db_path)
            .await
            .expect("store should connect for test");

        let pull_requests = store
            .load_pull_requests_for_account_window("missing", 90)
            .await
            .expect("unknown account lookup should not fail");

        assert!(pull_requests.is_empty());
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

        let status: String = sqlx::query_scalar(test_queries::SELECT_SYNC_RUN_STATUS_BY_ID_QUERY)
            .bind(run_id)
            .fetch_one(store.pool())
            .await
            .expect("sync run status should be queryable");

        let finished_at: Option<String> =
            sqlx::query_scalar(test_queries::SELECT_SYNC_RUN_FINISHED_AT_BY_ID_QUERY)
                .bind(run_id)
                .fetch_one(store.pool())
                .await
                .expect("sync run completion time should be queryable");

        assert_eq!(status, "success");
        assert!(finished_at.is_some());
    }
}
