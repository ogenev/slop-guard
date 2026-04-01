/// Test-only SQL statements kept outside Rust source for readability.
pub(crate) const COUNT_SCHEMA_TABLES_QUERY: &str = include_str!("sql/test/count_schema_tables.sql");
pub(crate) const COUNT_COMMITS_FOR_ARTIFACT_QUERY: &str =
    include_str!("sql/test/count_commits_for_artifact.sql");
pub(crate) const SELECT_COMMIT_SHA_FOR_ARTIFACT_QUERY: &str =
    include_str!("sql/test/select_commit_sha_for_artifact.sql");
pub(crate) const SELECT_SYNC_RUN_STATUS_BY_ID_QUERY: &str =
    include_str!("sql/test/select_sync_run_status_by_id.sql");
pub(crate) const SELECT_SYNC_RUN_FINISHED_AT_BY_ID_QUERY: &str =
    include_str!("sql/test/select_sync_run_finished_at_by_id.sql");
pub(crate) const COUNT_REPOSITORIES_QUERY: &str = include_str!("sql/test/count_repositories.sql");
pub(crate) const SELECT_ARTIFACT_STORAGE_SUMMARY_QUERY: &str =
    include_str!("sql/test/select_artifact_storage_summary.sql");
pub(crate) const COUNT_COMMITS_QUERY: &str = include_str!("sql/test/count_commits.sql");
pub(crate) const COUNT_ARTIFACTS_QUERY: &str = include_str!("sql/test/count_artifacts.sql");
pub(crate) const SELECT_LATEST_SYNC_RUN_STATUS_QUERY: &str =
    include_str!("sql/test/select_latest_sync_run_status.sql");
pub(crate) const SELECT_LATEST_SYNC_RUN_OUTCOME_QUERY: &str =
    include_str!("sql/test/select_latest_sync_run_outcome.sql");
