/// Runtime SQL statements used by the SQLite store.
pub(crate) const UPSERT_ACCOUNT_QUERY: &str = include_str!("sql/accounts/upsert.sql");
pub(crate) const FIND_ACCOUNT_BY_USERNAME_QUERY: &str =
    include_str!("sql/accounts/find_by_username.sql");
pub(crate) const UPSERT_REPOSITORY_QUERY: &str = include_str!("sql/repositories/upsert.sql");
pub(crate) const UPSERT_ARTIFACT_QUERY: &str = include_str!("sql/artifacts/upsert.sql");
pub(crate) const LOAD_PULL_REQUESTS_FOR_ACCOUNT_WINDOW_QUERY: &str =
    include_str!("sql/artifacts/load_pull_requests_for_account_window.sql");
pub(crate) const UPSERT_COMMIT_QUERY: &str = include_str!("sql/commits/upsert.sql");
pub(crate) const DELETE_COMMITS_FOR_ARTIFACT_QUERY: &str =
    include_str!("sql/commits/delete_for_artifact.sql");
pub(crate) const LOAD_COMMITS_FOR_ARTIFACT_QUERY: &str =
    include_str!("sql/commits/load_for_artifact.sql");
pub(crate) const START_SYNC_RUN_QUERY: &str = include_str!("sql/sync_runs/start.sql");
pub(crate) const FINISH_SYNC_RUN_QUERY: &str = include_str!("sql/sync_runs/finish.sql");
