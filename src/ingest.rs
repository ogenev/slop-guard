use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::github::{GitHubClient, NormalizedPullRequest};
use crate::store::{ArtifactUpsert, CommitUpsert, Store, SyncRunStatus};

const PULL_REQUEST_ARTIFACT_KIND: &str = "pull_request";

/// Coordinates GitHub ingestion and persistence for a single account sync.
#[derive(Clone)]
pub struct IngestService {
    client: GitHubClient,
    store: Store,
}

/// Running counters tracked while a sync is in progress.
#[derive(Clone, Debug, Default)]
struct SyncProgress {
    artifacts_discovered: i64,
    artifacts_stored: i64,
    commits_stored: usize,
}

/// JSON summary returned after a sync completes successfully.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SyncSummary {
    pub username: String,
    pub window_days: u16,
    pub artifacts_discovered: usize,
    pub artifacts_stored: usize,
    pub commits_stored: usize,
}

impl IngestService {
    /// Creates an ingestion service from the GitHub client and persistence layer.
    pub fn new(client: GitHubClient, store: Store) -> Self {
        Self { client, store }
    }

    /// Syncs one public GitHub account, records the sync run, and returns a summary.
    pub async fn sync_account(&self, username: &str, window_days: u16) -> Result<SyncSummary> {
        let account_id = self.store.upsert_account(username).await?;
        let run_id = self.store.start_sync_run(account_id, window_days).await?;
        let mut progress = SyncProgress::default();

        match self
            .sync_account_inner(username, window_days, account_id, &mut progress)
            .await
        {
            Ok(()) => {
                self.store
                    .finish_sync_run(
                        run_id,
                        SyncRunStatus::Success,
                        progress.artifacts_discovered,
                        progress.artifacts_stored,
                        None,
                    )
                    .await?;

                Ok(SyncSummary {
                    username: username.to_owned(),
                    window_days,
                    artifacts_discovered: progress.artifacts_discovered as usize,
                    artifacts_stored: progress.artifacts_stored as usize,
                    commits_stored: progress.commits_stored,
                })
            }
            Err(error) => {
                let error_message = error.to_string();

                if let Err(finish_error) = self
                    .store
                    .finish_sync_run(
                        run_id,
                        SyncRunStatus::Failed,
                        progress.artifacts_discovered,
                        progress.artifacts_stored,
                        Some(&error_message),
                    )
                    .await
                {
                    return Err(error.context(format!(
                        "failed to record sync run {run_id} failure: {finish_error}"
                    )));
                }

                Err(error)
            }
        }
    }

    /// Performs the GitHub fetch and persistence work after the sync run is registered.
    async fn sync_account_inner(
        &self,
        username: &str,
        window_days: u16,
        account_id: i64,
        progress: &mut SyncProgress,
    ) -> Result<()> {
        self.client.ensure_public_user(username).await?;

        let pull_requests = self
            .client
            .fetch_authored_pull_requests(username, window_days)
            .await?;
        progress.artifacts_discovered = pull_requests.len() as i64;

        for pull_request in &pull_requests {
            let stored_commit_count = self.persist_pull_request(account_id, pull_request).await?;
            progress.artifacts_stored += 1;
            progress.commits_stored += stored_commit_count;
        }

        Ok(())
    }

    /// Upserts one normalized pull request and replaces its stored commit set.
    async fn persist_pull_request(
        &self,
        account_id: i64,
        pull_request: &NormalizedPullRequest,
    ) -> Result<usize> {
        let repository_id = self
            .store
            .upsert_repository(
                &pull_request.repository_owner,
                &pull_request.repository_name,
            )
            .await?;
        let artifact_id = self
            .store
            .upsert_artifact(&ArtifactUpsert {
                account_id,
                repository_id: Some(repository_id),
                kind: PULL_REQUEST_ARTIFACT_KIND,
                external_id: &pull_request.external_id,
                pr_number: Some(pull_request.number),
                title: Some(&pull_request.title),
                body: pull_request.body.as_deref(),
                state: Some(&pull_request.state),
                created_at: &pull_request.created_at,
                updated_at: &pull_request.updated_at,
                additions: pull_request.additions,
                deletions: pull_request.deletions,
                changed_files: pull_request.changed_files,
                base_branch: pull_request.base_branch.as_deref(),
                head_branch: pull_request.head_branch.as_deref(),
            })
            .await
            .with_context(|| {
                format!(
                    "failed to persist pull request {}/{}#{}",
                    pull_request.repository_owner,
                    pull_request.repository_name,
                    pull_request.number
                )
            })?;

        // Commit sets are treated as authoritative snapshots from GitHub, so reruns
        // replace the previously stored set instead of trying to diff it incrementally.
        self.store.delete_commits_for_artifact(artifact_id).await?;

        for commit in &pull_request.commits {
            self.store
                .upsert_commit(&CommitUpsert {
                    artifact_id,
                    sha: &commit.sha,
                    message: &commit.message,
                    authored_at: commit.authored_at.as_deref(),
                    committed_at: commit.committed_at.as_deref(),
                })
                .await
                .with_context(|| {
                    format!(
                        "failed to persist commit {} for pull request {}/{}#{}",
                        commit.sha,
                        pull_request.repository_owner,
                        pull_request.repository_name,
                        pull_request.number
                    )
                })?;
        }

        Ok(pull_request.commits.len())
    }
}

#[cfg(test)]
mod tests {
    use reqwest::Url;
    use serde_json::{Value, json};
    use sqlx::Row;
    use tempfile::TempDir;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{body_string_contains, method, path},
    };

    use crate::{github::GitHubClient, store::Store};

    use super::IngestService;

    #[tokio::test]
    async fn sync_account_persists_pull_requests_and_commits() {
        let server = MockServer::start().await;
        mock_account_lookup_user(&server, "octocat").await;
        mock_graphql_response(
            &server,
            "SearchAuthoredPullRequests",
            Some("\"cursor\":null"),
            json!({
                "search": {
                    "pageInfo": {
                        "hasNextPage": false,
                        "endCursor": null
                    },
                    "nodes": [
                        {
                            "__typename": "PullRequest",
                            "id": "PR_kwDOAAABc842",
                            "number": 42,
                            "createdAt": "2026-03-01T10:00:00Z",
                            "repository": {
                                "name": "cargo",
                                "isPrivate": false,
                                "owner": { "username": "rust-lang" }
                            }
                        }
                    ]
                }
            }),
        )
        .await;
        mock_graphql_response(
            &server,
            "PullRequestDetailsBatch",
            Some("PR_kwDOAAABc842"),
            json!({
                "nodes": [
                    {
                        "__typename": "PullRequest",
                        "id": "PR_kwDOAAABc842",
                        "databaseId": 9001,
                        "number": 42,
                        "title": "Improve parser coverage",
                        "body": "Adds regression tests and cleanup.",
                        "state": "OPEN",
                        "createdAt": "2026-03-01T10:00:00Z",
                        "updatedAt": "2026-03-01T11:00:00Z",
                        "additions": 17,
                        "deletions": 4,
                        "changedFiles": 3,
                        "baseRefName": "main",
                        "headRefName": "topic/coverage",
                        "repository": {
                            "name": "cargo",
                            "isPrivate": false,
                            "owner": { "username": "rust-lang" }
                        },
                        "commits": {
                            "pageInfo": {
                                "hasNextPage": false,
                                "endCursor": null
                            },
                            "nodes": [
                                {
                                    "commit": {
                                        "oid": "abc123",
                                        "message": "test: add parser regression",
                                        "authoredDate": "2026-03-01T09:00:00Z",
                                        "committedDate": "2026-03-01T09:05:00Z"
                                    }
                                },
                                {
                                    "commit": {
                                        "oid": "def456",
                                        "message": "refactor: simplify fixtures",
                                        "authoredDate": "2026-03-01T09:10:00Z",
                                        "committedDate": "2026-03-01T09:15:00Z"
                                    }
                                }
                            ]
                        }
                    }
                ]
            }),
        )
        .await;

        let temp_dir = TempDir::new().expect("temp dir should be created");
        let db_path = temp_dir.path().join("aislop.db");
        let store = Store::connect(&db_path)
            .await
            .expect("store should connect for ingest test");
        let client = test_client(&server);
        let service = IngestService::new(client, store.clone());

        let summary = service
            .sync_account("octocat", 14)
            .await
            .expect("sync should succeed");

        assert_eq!(summary.artifacts_discovered, 1);
        assert_eq!(summary.artifacts_stored, 1);
        assert_eq!(summary.commits_stored, 2);

        let repository_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM repositories")
            .fetch_one(store.pool())
            .await
            .expect("repository count should be queryable");
        let artifact_row = sqlx::query(
            "SELECT kind, external_id, title, additions, deletions, changed_files FROM artifacts",
        )
        .fetch_one(store.pool())
        .await
        .expect("artifact row should be queryable");
        let commit_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM commits")
            .fetch_one(store.pool())
            .await
            .expect("commit count should be queryable");
        let sync_status: String =
            sqlx::query_scalar("SELECT status FROM sync_runs ORDER BY id DESC LIMIT 1")
                .fetch_one(store.pool())
                .await
                .expect("sync status should be queryable");

        assert_eq!(repository_count, 1);
        assert_eq!(artifact_row.get::<String, _>("kind"), "pull_request");
        assert_eq!(artifact_row.get::<String, _>("external_id"), "9001");
        assert_eq!(
            artifact_row.get::<String, _>("title"),
            "Improve parser coverage"
        );
        assert_eq!(artifact_row.get::<i64, _>("additions"), 17);
        assert_eq!(artifact_row.get::<i64, _>("deletions"), 4);
        assert_eq!(artifact_row.get::<i64, _>("changed_files"), 3);
        assert_eq!(commit_count, 2);
        assert_eq!(sync_status, "success");
    }

    #[tokio::test]
    async fn sync_account_succeeds_when_no_pull_requests_are_found() {
        let server = MockServer::start().await;
        mock_account_lookup_user(&server, "octocat").await;
        mock_graphql_response(
            &server,
            "SearchAuthoredPullRequests",
            Some("\"cursor\":null"),
            json!({
                "search": {
                    "pageInfo": {
                        "hasNextPage": false,
                        "endCursor": null
                    },
                    "nodes": []
                }
            }),
        )
        .await;

        let temp_dir = TempDir::new().expect("temp dir should be created");
        let db_path = temp_dir.path().join("aislop.db");
        let store = Store::connect(&db_path)
            .await
            .expect("store should connect for ingest test");
        let client = test_client(&server);
        let service = IngestService::new(client, store.clone());

        let summary = service
            .sync_account("octocat", 30)
            .await
            .expect("sync should succeed without pull requests");

        let artifact_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM artifacts")
            .fetch_one(store.pool())
            .await
            .expect("artifact count should be queryable");
        let row = sqlx::query(
            "SELECT status, artifacts_discovered, artifacts_stored, error_message FROM sync_runs ORDER BY id DESC LIMIT 1",
        )
        .fetch_one(store.pool())
        .await
        .expect("sync run row should be queryable");

        assert_eq!(summary.artifacts_discovered, 0);
        assert_eq!(summary.artifacts_stored, 0);
        assert_eq!(summary.commits_stored, 0);
        assert_eq!(artifact_count, 0);
        assert_eq!(row.get::<String, _>("status"), "success");
        assert_eq!(row.get::<i64, _>("artifacts_discovered"), 0);
        assert_eq!(row.get::<i64, _>("artifacts_stored"), 0);
        assert!(row.get::<Option<String>, _>("error_message").is_none());
    }

    #[tokio::test]
    async fn sync_account_records_failed_run_when_user_is_missing() {
        let server = MockServer::start().await;
        mock_graphql_response(
            &server,
            "AccountLookup",
            Some("\"username\":\"missing-user\""),
            json!({
                "repositoryOwner": null
            }),
        )
        .await;

        let temp_dir = TempDir::new().expect("temp dir should be created");
        let db_path = temp_dir.path().join("aislop.db");
        let store = Store::connect(&db_path)
            .await
            .expect("store should connect for ingest test");
        let client = test_client(&server);
        let service = IngestService::new(client, store.clone());

        let error = service
            .sync_account("missing-user", 30)
            .await
            .expect_err("sync should fail for a missing user");

        let row = sqlx::query(
            "SELECT status, artifacts_discovered, artifacts_stored, error_message FROM sync_runs ORDER BY id DESC LIMIT 1",
        )
        .fetch_one(store.pool())
        .await
        .expect("sync run row should be queryable");

        assert!(error.to_string().contains("does not exist"));
        assert_eq!(row.get::<String, _>("status"), "failed");
        assert_eq!(row.get::<i64, _>("artifacts_discovered"), 0);
        assert_eq!(row.get::<i64, _>("artifacts_stored"), 0);
        assert!(
            row.get::<Option<String>, _>("error_message")
                .expect("sync run should have an error message")
                .contains("does not exist")
        );
    }

    fn test_client(server: &MockServer) -> GitHubClient {
        GitHubClient::with_base_url(
            "test-agent",
            "test-token",
            Url::parse(&format!("{}/", server.uri())).expect("base URL should parse"),
        )
        .expect("test GitHub client should be created")
    }

    async fn mock_account_lookup_user(server: &MockServer, username: &str) {
        mock_graphql_response(
            server,
            "AccountLookup",
            Some(&format!("\"username\":\"{username}\"")),
            json!({
                "repositoryOwner": {
                    "__typename": "User",
                    "username": username
                }
            }),
        )
        .await;
    }

    async fn mock_graphql_response(
        server: &MockServer,
        operation_name: &str,
        body_fragment: Option<&str>,
        data: Value,
    ) {
        let body_fragment = body_fragment.unwrap_or_default().to_owned();

        Mock::given(method("POST"))
            .and(path("/graphql"))
            .and(body_string_contains(operation_name))
            .and(body_string_contains(body_fragment))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "data": data })))
            .expect(1)
            .mount(server)
            .await;
    }
}
