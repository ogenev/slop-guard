use serde::Deserialize;

/// Normalized pull-request record used by ingestion and persistence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NormalizedPullRequest {
    pub external_id: String,
    pub repository_owner: String,
    pub repository_name: String,
    pub number: i64,
    pub title: String,
    pub body: Option<String>,
    pub state: String,
    pub created_at: String,
    pub updated_at: String,
    pub additions: i64,
    pub deletions: i64,
    pub changed_files: i64,
    pub base_branch: Option<String>,
    pub head_branch: Option<String>,
    pub commits: Vec<NormalizedCommit>,
}

/// Normalized commit record attached to a pull request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NormalizedCommit {
    pub sha: String,
    pub message: String,
    pub authored_at: Option<String>,
    pub committed_at: Option<String>,
}

/// Minimal GitHub GraphQL response envelope used by the client.
#[derive(Debug, Deserialize)]
pub(crate) struct GraphQlResponse<T> {
    pub data: Option<T>,
    pub errors: Option<Vec<GraphQlError>>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct GraphQlError {
    pub message: String,
}

/// Account lookup result used to distinguish users from organizations.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AccountLookupData {
    pub repository_owner: Option<GraphQlAccountOwnerNode>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct GraphQlAccountOwnerNode {
    #[serde(rename = "__typename")]
    pub typename: String,
    pub username: String,
}

/// Search result wrapper for authored pull-request discovery.
#[derive(Debug, Deserialize)]
pub(crate) struct SearchPullRequestsData {
    pub search: SearchConnection,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SearchConnection {
    pub page_info: GraphQlPageInfo,
    pub nodes: Vec<Option<SearchPullRequestRefNode>>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GraphQlPageInfo {
    pub has_next_page: bool,
    pub end_cursor: Option<String>,
}

/// Minimal pull-request-shaped search node used only for lightweight discovery.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SearchPullRequestRefNode {
    #[serde(rename = "__typename")]
    pub typename: String,
    pub id: Option<String>,
    pub number: Option<i64>,
    pub repository: Option<GraphQlRepository>,
}

/// Batch details response used after lightweight search discovery.
#[derive(Debug, Deserialize)]
pub(crate) struct PullRequestDetailsData {
    pub nodes: Vec<Option<PullRequestDetailsNode>>,
}

/// Fully hydrated pull-request node returned by the details batch query.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PullRequestDetailsNode {
    #[serde(rename = "__typename")]
    pub typename: String,
    pub id: Option<String>,
    pub database_id: Option<i64>,
    pub title: Option<String>,
    pub body: Option<String>,
    pub state: Option<String>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
    pub additions: Option<i64>,
    pub deletions: Option<i64>,
    pub changed_files: Option<i64>,
    pub base_ref_name: Option<String>,
    pub head_ref_name: Option<String>,
    pub commits: Option<GraphQlCommitConnection>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GraphQlRepository {
    pub name: String,
    pub is_private: bool,
    pub owner: GraphQlOwner,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct GraphQlOwner {
    pub username: String,
}

/// Commit page attached to a pull request node.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GraphQlCommitConnection {
    pub page_info: GraphQlPageInfo,
    pub nodes: Vec<Option<GraphQlPullRequestCommitNode>>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct GraphQlPullRequestCommitNode {
    pub commit: GraphQlCommit,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GraphQlCommit {
    pub oid: String,
    pub message: String,
    pub authored_date: Option<String>,
    pub committed_date: Option<String>,
}

/// Additional commit page fetched when a PR has more than one commit page.
#[derive(Debug, Deserialize)]
pub(crate) struct PullRequestCommitsPageData {
    pub node: Option<PullRequestNode>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct PullRequestNode {
    #[serde(rename = "__typename")]
    pub typename: String,
    pub commits: Option<GraphQlCommitConnection>,
}
