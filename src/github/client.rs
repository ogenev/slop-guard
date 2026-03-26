use std::{env, fmt};

use anyhow::{Context, Result, anyhow, bail};
use chrono::{Duration, Utc};
use reqwest::{
    Client, Response, StatusCode, Url,
    header::{ACCEPT, HeaderMap, HeaderValue},
};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use super::models::{
    AccountLookupData, GraphQlCommitConnection, GraphQlError, GraphQlPullRequestCommitNode,
    GraphQlRepository, GraphQlResponse, NormalizedCommit, NormalizedPullRequest,
    PullRequestCommitsPageData, SearchPullRequestsData, SearchResultNode,
};

const DEFAULT_USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"));
const DEFAULT_GRAPHQL_URL: &str = "https://api.github.com/graphql";
const GITHUB_GRAPHQL_ACCEPT_HEADER: &str = "application/json";

// GitHub's schema still exposes `login`; we alias it to `username` so the rest
// of the codebase can consistently use our preferred terminology.
const ACCOUNT_LOOKUP_QUERY: &str = r#"
query AccountLookup($username: String!) {
  repositoryOwner(login: $username) {
    __typename
    username: login
  }
}
"#;

const SEARCH_AUTHORED_PULL_REQUESTS_QUERY: &str = r#"
query SearchAuthoredPullRequests($query: String!, $cursor: String) {
  search(query: $query, type: ISSUE, first: 100, after: $cursor) {
    pageInfo {
      hasNextPage
      endCursor
    }
    nodes {
      __typename
      ... on PullRequest {
        id
        databaseId
        number
        title
        body
        state
        createdAt
        updatedAt
        additions
        deletions
        changedFiles
        baseRefName
        headRefName
        repository {
          name
          isPrivate
          owner {
            username: login
          }
        }
        commits(first: 100) {
          pageInfo {
            hasNextPage
            endCursor
          }
          nodes {
            commit {
              oid
              message
              authoredDate
              committedDate
            }
          }
        }
      }
    }
  }
}
"#;

const PULL_REQUEST_COMMITS_PAGE_QUERY: &str = r#"
query PullRequestCommitsPage($pullRequestId: ID!, $cursor: String) {
  node(id: $pullRequestId) {
    __typename
    ... on PullRequest {
      repository {
        name
        isPrivate
        owner {
          username: login
        }
      }
      commits(first: 100, after: $cursor) {
        pageInfo {
          hasNextPage
          endCursor
        }
        nodes {
          commit {
            oid
            message
            authoredDate
            committedDate
          }
        }
      }
    }
  }
}
"#;

/// Small GitHub GraphQL client focused on public-account pull-request ingestion.
#[derive(Clone)]
pub struct GitHubClient {
    inner: Client,
    user_agent: String,
    graphql_url: Url,
    token: String,
}

impl fmt::Debug for GitHubClient {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GitHubClient")
            .field("inner", &self.inner)
            .field("user_agent", &self.user_agent)
            .field("graphql_url", &self.graphql_url)
            .field("token", &"<redacted>")
            .finish()
    }
}

impl GitHubClient {
    /// Creates a client that targets the default GitHub GraphQL endpoint.
    pub fn new(user_agent: impl Into<String>, token: impl Into<String>) -> Result<Self> {
        let graphql_url = Url::parse(DEFAULT_GRAPHQL_URL)?;
        Self::with_graphql_url(user_agent, token, graphql_url)
    }

    /// Creates a client from `GITHUB_TOKEN` using the crate-derived User-Agent.
    pub fn from_env() -> Result<Self> {
        let token = env::var("GITHUB_TOKEN")
            .context("GITHUB_TOKEN must be set for GitHub pull-request ingestion")?;

        Self::new(DEFAULT_USER_AGENT, token)
    }

    /// Creates a client from a custom base URL, primarily for tests.
    pub fn with_base_url(
        user_agent: impl Into<String>,
        token: impl Into<String>,
        base_url: Url,
    ) -> Result<Self> {
        let graphql_url = normalize_graphql_url(base_url)?;
        Self::with_graphql_url(user_agent, token, graphql_url)
    }

    /// Creates a client from a fully qualified GraphQL URL.
    fn with_graphql_url(
        user_agent: impl Into<String>,
        token: impl Into<String>,
        graphql_url: Url,
    ) -> Result<Self> {
        let user_agent = user_agent.into();
        let token = token.into();

        if token.trim().is_empty() {
            bail!("GitHub token must not be empty")
        }

        let inner = Client::builder()
            .user_agent(user_agent.clone())
            .build()
            .context("default GitHub client configuration should be valid")?;

        Ok(Self {
            inner,
            user_agent,
            graphql_url,
            token,
        })
    }

    /// Exposes the underlying HTTP client for focused testing.
    pub fn http(&self) -> &Client {
        &self.inner
    }

    /// Returns the User-Agent string configured for outgoing requests.
    pub fn user_agent(&self) -> &str {
        &self.user_agent
    }

    /// Verifies that the requested GitHub account exists and is a user, not an organization.
    pub async fn ensure_public_user(&self, username: &str) -> Result<()> {
        let response = self
            .post_graphql::<AccountLookupData>(
                ACCOUNT_LOOKUP_QUERY,
                json!({ "username": username }),
                &format!("failed to fetch GitHub account {username}"),
            )
            .await?;

        match response.repository_owner {
            Some(owner) if owner.typename == "User" => {
                let _ = owner.username;
                Ok(())
            }
            Some(owner) if owner.typename == "Organization" => {
                bail!("GitHub username {username} is not a user account")
            }
            Some(owner) => bail!(
                "GitHub username {username} resolved to unsupported account type {}",
                owner.typename
            ),
            None => bail!("GitHub user {username} does not exist"),
        }
    }

    /// Fetches recent public pull requests authored by the given username.
    pub async fn fetch_authored_pull_requests(
        &self,
        username: &str,
        window_days: u16,
    ) -> Result<Vec<NormalizedPullRequest>> {
        let cutoff = (Utc::now() - Duration::days(i64::from(window_days)))
            .date_naive()
            .format("%Y-%m-%d")
            .to_string();
        let search_query = format!("is:pr author:{username} created:>={cutoff}");
        let mut cursor = None;
        let mut pull_requests = Vec::new();

        loop {
            let response = self
                .post_graphql::<SearchPullRequestsData>(
                    SEARCH_AUTHORED_PULL_REQUESTS_QUERY,
                    json!({
                        "query": search_query.as_str(),
                        "cursor": cursor,
                    }),
                    &format!("failed to search authored pull requests for GitHub user {username}"),
                )
                .await?;
            let page_info = response.search.page_info;

            for node in response.search.nodes.into_iter().flatten() {
                if let Some(pull_request) = self.normalize_search_result_node(node).await? {
                    pull_requests.push(pull_request);
                }
            }

            if !page_info.has_next_page {
                break;
            }

            cursor = Some(page_info.end_cursor.context(format!(
                "GitHub GraphQL search page for user {username} is missing endCursor"
            ))?);
        }

        Ok(pull_requests)
    }

    /// Converts one GraphQL search node into our normalized PR shape.
    async fn normalize_search_result_node(
        &self,
        node: SearchResultNode,
    ) -> Result<Option<NormalizedPullRequest>> {
        if node.typename != "PullRequest" {
            return Ok(None);
        }

        let node_id = required(node.id, "pull request search result is missing node id")?;
        let repository = required(
            node.repository,
            format!("pull request node {node_id} is missing repository metadata"),
        )?;

        if repository.is_private {
            return Ok(None);
        }

        let GraphQlRepository {
            name: repository_name,
            is_private: _,
            owner,
        } = repository;
        let repository_owner = owner.username;
        let number = required(
            node.number,
            format!("pull request node {node_id} is missing number"),
        )?;
        let title = required(
            node.title,
            format!("pull request node {node_id} is missing title"),
        )?;
        let state = required(
            node.state,
            format!("pull request node {node_id} is missing state"),
        )?
        .to_ascii_lowercase();
        let created_at = required(
            node.created_at,
            format!("pull request node {node_id} is missing createdAt"),
        )?;
        let updated_at = required(
            node.updated_at,
            format!("pull request node {node_id} is missing updatedAt"),
        )?;
        let additions = required(
            node.additions,
            format!("pull request node {node_id} is missing additions"),
        )?;
        let deletions = required(
            node.deletions,
            format!("pull request node {node_id} is missing deletions"),
        )?;
        let changed_files = required(
            node.changed_files,
            format!("pull request node {node_id} is missing changedFiles"),
        )?;
        let external_id = node
            .database_id
            .map(|database_id| database_id.to_string())
            .unwrap_or_else(|| node_id.clone());
        let commit_connection = required(
            node.commits,
            format!("pull request node {node_id} is missing commit connection"),
        )?;
        // The search query eagerly fetches the first commit page so most PRs can be
        // normalized without additional round trips.
        let mut commits = normalize_commit_nodes(commit_connection.nodes);
        let mut page_info = commit_connection.page_info;

        while page_info.has_next_page {
            let cursor = page_info.end_cursor.context(format!(
                "pull request {repository_owner}/{repository_name}#{number} commit page is missing endCursor"
            ))?;
            let next_page = self
                .fetch_pull_request_commits_page(
                    &node_id,
                    &repository_owner,
                    &repository_name,
                    number,
                    &cursor,
                )
                .await?;

            commits.extend(normalize_commit_nodes(next_page.nodes));
            page_info = next_page.page_info;
        }

        Ok(Some(NormalizedPullRequest {
            external_id,
            repository_owner,
            repository_name,
            number,
            title,
            body: node.body,
            state,
            created_at,
            updated_at,
            additions,
            deletions,
            changed_files,
            base_branch: node.base_ref_name,
            head_branch: node.head_ref_name,
            commits,
        }))
    }

    /// Fetches an additional page of commits for a previously discovered pull request.
    async fn fetch_pull_request_commits_page(
        &self,
        pull_request_node_id: &str,
        repository_owner: &str,
        repository_name: &str,
        pull_request_number: i64,
        cursor: &str,
    ) -> Result<GraphQlCommitConnection> {
        let response = self
            .post_graphql::<PullRequestCommitsPageData>(
                PULL_REQUEST_COMMITS_PAGE_QUERY,
                json!({
                    "pullRequestId": pull_request_node_id,
                    "cursor": cursor,
                }),
                &format!(
                    "failed to fetch additional commits for pull request {repository_owner}/{repository_name}#{pull_request_number}"
                ),
            )
            .await?;
        let node = response.node.context(format!(
            "pull request {repository_owner}/{repository_name}#{pull_request_number} was not returned by GitHub GraphQL"
        ))?;

        if node.typename != "PullRequest" {
            bail!(
                "GitHub GraphQL node {} is not a pull request",
                pull_request_node_id
            )
        }

        if let Some(repository) = node.repository.as_ref()
            && repository.is_private
        {
            bail!(
                "pull request {repository_owner}/{repository_name}#{pull_request_number} is private and outside current scope"
            )
        }

        required(
            node.commits,
            format!(
                "pull request {repository_owner}/{repository_name}#{pull_request_number} is missing commit connection"
            ),
        )
    }

    /// Sends a GraphQL request and decodes its typed response payload.
    async fn post_graphql<T>(&self, query: &str, variables: Value, context: &str) -> Result<T>
    where
        T: DeserializeOwned,
    {
        let response = self
            .inner
            .post(self.graphql_url.clone())
            .bearer_auth(&self.token)
            .header(
                ACCEPT,
                HeaderValue::from_static(GITHUB_GRAPHQL_ACCEPT_HEADER),
            )
            .json(&json!({
                "query": query,
                "variables": variables,
            }))
            .send()
            .await
            .context("GitHub GraphQL request failed")?;

        self.decode_graphql_response(response, context).await
    }

    /// Maps HTTP and GraphQL failures into contextual anyhow errors.
    async fn decode_graphql_response<T>(&self, response: Response, context: &str) -> Result<T>
    where
        T: DeserializeOwned,
    {
        let status = response.status();
        let headers = response.headers().clone();
        let body = response
            .text()
            .await
            .with_context(|| format!("{context}: failed to read GitHub GraphQL response body"))?;

        if !status.is_success() {
            return Err(http_error_from_body(status, &headers, &body, context));
        }

        let envelope = serde_json::from_str::<GraphQlResponse<T>>(&body)
            .with_context(|| format!("{context}: failed to decode JSON response"))?;

        if let Some(errors) = envelope.errors
            && !errors.is_empty()
        {
            return Err(graphql_error_from_body(&headers, &errors, context));
        }

        envelope
            .data
            .ok_or_else(|| anyhow!("{context}: GitHub GraphQL response did not include data"))
    }
}

/// Normalizes a test base URL into the GraphQL endpoint expected by the client.
fn normalize_graphql_url(base_url: Url) -> Result<Url> {
    let trimmed_path = base_url.path().trim_end_matches('/');

    if trimmed_path.ends_with("graphql") {
        return Ok(base_url);
    }

    base_url
        .join("graphql")
        .with_context(|| format!("failed to construct GitHub GraphQL URL from base {base_url}"))
}

/// Converts a required GraphQL field into a value or a contextual error.
fn required<T>(value: Option<T>, message: impl Into<String>) -> Result<T> {
    value.ok_or_else(|| anyhow!(message.into()))
}

/// Drops null commit nodes and maps GitHub commit payloads into normalized commits.
fn normalize_commit_nodes(
    nodes: Vec<Option<GraphQlPullRequestCommitNode>>,
) -> Vec<NormalizedCommit> {
    nodes
        .into_iter()
        .flatten()
        .map(|node| NormalizedCommit {
            sha: node.commit.oid,
            message: node.commit.message,
            authored_at: node.commit.authored_date,
            committed_at: node.commit.committed_date,
        })
        .collect()
}

/// Formats an HTTP-level GitHub error, with special handling for rate limits.
fn http_error_from_body(
    status: StatusCode,
    headers: &HeaderMap,
    body: &str,
    context: &str,
) -> anyhow::Error {
    if is_rate_limit_error(headers, body) {
        anyhow!("{context}: GitHub API rate limit exceeded")
    } else if body.is_empty() {
        anyhow!("{context}: GitHub API returned HTTP {status}")
    } else {
        anyhow!("{context}: GitHub API returned HTTP {status}: {body}")
    }
}

/// Formats a GraphQL-level GitHub error, with special handling for rate limits.
fn graphql_error_from_body(
    headers: &HeaderMap,
    errors: &[GraphQlError],
    context: &str,
) -> anyhow::Error {
    let message = errors
        .iter()
        .map(|error| error.message.as_str())
        .collect::<Vec<_>>()
        .join("; ");

    if is_rate_limit_error(headers, &message) {
        anyhow!("{context}: GitHub API rate limit exceeded")
    } else {
        anyhow!("{context}: GitHub GraphQL returned errors: {message}")
    }
}

/// Detects rate-limit failures from either headers or returned error text.
fn is_rate_limit_error(headers: &HeaderMap, message: &str) -> bool {
    headers
        .get("x-ratelimit-remaining")
        .and_then(|value| value.to_str().ok())
        == Some("0")
        || message.to_ascii_lowercase().contains("rate limit")
}

#[cfg(test)]
mod tests {
    use reqwest::Url;
    use serde_json::{Value, json};
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{body_string_contains, method, path},
    };

    use super::GitHubClient;

    #[test]
    fn debug_output_redacts_the_github_token() {
        let client = GitHubClient::new("test-agent", "super-secret-token")
            .expect("test GitHub client should be created");

        let debug_output = format!("{client:?}");

        assert!(debug_output.contains("<redacted>"));
        assert!(!debug_output.contains("super-secret-token"));
    }

    #[tokio::test]
    async fn ensure_public_user_accepts_users() {
        let server = MockServer::start().await;
        let client = test_client(&server);

        mock_graphql_response(
            &server,
            "AccountLookup",
            Some("\"username\":\"octocat\""),
            json!({
                "repositoryOwner": {
                    "__typename": "User",
                    "username": "octocat"
                }
            }),
        )
        .await;

        client
            .ensure_public_user("octocat")
            .await
            .expect("user lookup should succeed");
    }

    #[tokio::test]
    async fn ensure_public_user_rejects_organizations() {
        let server = MockServer::start().await;
        let client = test_client(&server);

        mock_graphql_response(
            &server,
            "AccountLookup",
            Some("\"username\":\"rust-lang\""),
            json!({
                "repositoryOwner": {
                    "__typename": "Organization",
                    "username": "rust-lang"
                }
            }),
        )
        .await;

        let error = client
            .ensure_public_user("rust-lang")
            .await
            .expect_err("organization lookup should fail");

        assert!(error.to_string().contains("not a user account"));
    }

    #[tokio::test]
    async fn fetches_authored_pull_requests_with_commit_hydration() {
        let server = MockServer::start().await;
        let client = test_client(&server);

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
                            "databaseId": 12345,
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
                }
            }),
        )
        .await;

        let pull_requests = client
            .fetch_authored_pull_requests("octocat", 30)
            .await
            .expect("pull requests should be fetched");

        assert_eq!(pull_requests.len(), 1);
        let pull_request = &pull_requests[0];
        assert_eq!(pull_request.external_id, "12345");
        assert_eq!(pull_request.repository_owner, "rust-lang");
        assert_eq!(pull_request.repository_name, "cargo");
        assert_eq!(pull_request.number, 42);
        assert_eq!(pull_request.state, "open");
        assert_eq!(pull_request.base_branch.as_deref(), Some("main"));
        assert_eq!(pull_request.head_branch.as_deref(), Some("topic/coverage"));
        assert_eq!(pull_request.commits.len(), 2);
        assert_eq!(pull_request.commits[0].sha, "abc123");
    }

    #[tokio::test]
    async fn fetches_additional_commit_pages_for_pull_requests() {
        let server = MockServer::start().await;
        let client = test_client(&server);

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
                            "id": "PR_kwDOAAABc843",
                            "databaseId": 67890,
                            "number": 99,
                            "title": "Handle pagination",
                            "body": null,
                            "state": "MERGED",
                            "createdAt": "2026-03-05T10:00:00Z",
                            "updatedAt": "2026-03-05T11:00:00Z",
                            "additions": 8,
                            "deletions": 1,
                            "changedFiles": 2,
                            "baseRefName": "main",
                            "headRefName": "topic/pagination",
                            "repository": {
                                "name": "cargo",
                                "isPrivate": false,
                                "owner": { "username": "rust-lang" }
                            },
                            "commits": {
                                "pageInfo": {
                                    "hasNextPage": true,
                                    "endCursor": "commit-cursor-1"
                                },
                                "nodes": [
                                    {
                                        "commit": {
                                            "oid": "abc123",
                                            "message": "feat: start pagination handling",
                                            "authoredDate": "2026-03-05T09:00:00Z",
                                            "committedDate": "2026-03-05T09:05:00Z"
                                        }
                                    }
                                ]
                            }
                        }
                    ]
                }
            }),
        )
        .await;

        mock_graphql_response(
            &server,
            "PullRequestCommitsPage",
            Some("\"cursor\":\"commit-cursor-1\""),
            json!({
                "node": {
                    "__typename": "PullRequest",
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
                                    "oid": "def456",
                                    "message": "fix: finish pagination handling",
                                    "authoredDate": "2026-03-05T09:10:00Z",
                                    "committedDate": "2026-03-05T09:15:00Z"
                                }
                            }
                        ]
                    }
                }
            }),
        )
        .await;

        let pull_requests = client
            .fetch_authored_pull_requests("octocat", 30)
            .await
            .expect("pull requests should be fetched");

        assert_eq!(pull_requests.len(), 1);
        assert_eq!(pull_requests[0].commits.len(), 2);
        assert_eq!(pull_requests[0].commits[1].sha, "def456");
    }

    #[tokio::test]
    async fn skips_private_pull_requests_from_search_results() {
        let server = MockServer::start().await;
        let client = test_client(&server);

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
                            "id": "PR_kwDOAAABc844",
                            "databaseId": 77,
                            "number": 7,
                            "title": "Secret refactor",
                            "body": "Internal only",
                            "state": "OPEN",
                            "createdAt": "2026-03-01T10:00:00Z",
                            "updatedAt": "2026-03-01T11:00:00Z",
                            "additions": 10,
                            "deletions": 2,
                            "changedFiles": 1,
                            "baseRefName": "main",
                            "headRefName": "topic/private",
                            "repository": {
                                "name": "secret-repo",
                                "isPrivate": true,
                                "owner": { "username": "private-org" }
                            },
                            "commits": {
                                "pageInfo": {
                                    "hasNextPage": false,
                                    "endCursor": null
                                },
                                "nodes": []
                            }
                        }
                    ]
                }
            }),
        )
        .await;

        let pull_requests = client
            .fetch_authored_pull_requests("octocat", 30)
            .await
            .expect("pull request fetch should succeed");

        assert!(pull_requests.is_empty());
    }

    fn test_client(server: &MockServer) -> GitHubClient {
        GitHubClient::with_base_url(
            "test-agent",
            "test-token",
            Url::parse(&format!("{}/", server.uri())).expect("base URL should parse"),
        )
        .expect("test GitHub client should be created")
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
