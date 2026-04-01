use std::{
    collections::{HashMap, HashSet},
    env, fmt,
};

use anyhow::{Context, Result, anyhow, bail};
use chrono::{Duration, Utc};
use reqwest::{
    Client, Response, StatusCode, Url,
    header::{ACCEPT, HeaderMap, HeaderValue},
};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use tokio::task::JoinSet;

use super::{
    models::{
        AccountLookupData, GraphQlCommitConnection, GraphQlError, GraphQlPullRequestCommitNode,
        GraphQlRepository, GraphQlResponse, NormalizedCommit, NormalizedPullRequest,
        PullRequestCommitsPageData, PullRequestDetailsData, PullRequestDetailsNode,
        SearchPullRequestRefNode, SearchPullRequestsData,
    },
    queries::{
        ACCOUNT_LOOKUP_QUERY, PULL_REQUEST_COMMITS_PAGE_QUERY, PULL_REQUEST_DETAILS_QUERY,
        SEARCH_AUTHORED_PULL_REQUESTS_QUERY,
    },
};

const DEFAULT_USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"));
const DEFAULT_GRAPHQL_URL: &str = "https://api.github.com/graphql";
const GITHUB_GRAPHQL_ACCEPT_HEADER: &str = "application/json";

const PULL_REQUEST_DETAILS_BATCH_SIZE: usize = 20;

/// Default cap for concurrent pull-request detail batch hydration requests.
pub const DEFAULT_PULL_REQUEST_DETAILS_CONCURRENCY: usize = 4;

/// Lightweight discovery record collected from the authored-PR search query.
#[derive(Clone, Debug, PartialEq, Eq)]
struct DiscoveredPullRequestRef {
    node_id: String,
    repository_owner: String,
    repository_name: String,
    number: i64,
}

/// Small GitHub GraphQL client focused on public-account pull-request ingestion.
#[derive(Clone)]
pub struct GitHubClient {
    inner: Client,
    user_agent: String,
    graphql_url: Url,
    token: String,
    pull_request_details_concurrency: usize,
}

impl fmt::Debug for GitHubClient {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GitHubClient")
            .field("inner", &self.inner)
            .field("user_agent", &self.user_agent)
            .field("graphql_url", &self.graphql_url)
            .field(
                "pull_request_details_concurrency",
                &self.pull_request_details_concurrency,
            )
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
            pull_request_details_concurrency: DEFAULT_PULL_REQUEST_DETAILS_CONCURRENCY,
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

    /// Returns the configured maximum number of concurrent detail hydration requests.
    pub fn pull_request_details_concurrency(&self) -> usize {
        self.pull_request_details_concurrency
    }

    /// Overrides the maximum number of concurrent detail hydration requests.
    pub fn with_pull_request_details_concurrency(
        mut self,
        pull_request_details_concurrency: usize,
    ) -> Result<Self> {
        if pull_request_details_concurrency == 0 {
            bail!("pull request details concurrency must be at least 1")
        }

        self.pull_request_details_concurrency = pull_request_details_concurrency;
        Ok(self)
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
        let pull_request_refs = self
            .search_authored_pull_request_refs(username, &search_query)
            .await?;

        self.fetch_pull_request_details_batches(&pull_request_refs)
            .await
    }

    /// Hydrates discovered pull-request batches concurrently while preserving batch order.
    async fn fetch_pull_request_details_batches(
        &self,
        pull_request_refs: &[DiscoveredPullRequestRef],
    ) -> Result<Vec<NormalizedPullRequest>> {
        let detail_batches = pull_request_refs
            .chunks(PULL_REQUEST_DETAILS_BATCH_SIZE)
            .map(|batch| batch.to_vec())
            .collect::<Vec<_>>();

        if detail_batches.is_empty() {
            return Ok(Vec::new());
        }

        let mut join_set = JoinSet::new();
        let mut next_batch_index = 0;
        let mut completed_batches = Vec::with_capacity(detail_batches.len());

        while next_batch_index < detail_batches.len()
            && join_set.len() < self.pull_request_details_concurrency
        {
            self.spawn_pull_request_details_batch(
                &mut join_set,
                next_batch_index,
                detail_batches[next_batch_index].clone(),
            );
            next_batch_index += 1;
        }

        while let Some(join_result) = join_set.join_next().await {
            let (batch_index, batch_pull_requests) = match join_result {
                Ok(Ok(result)) => result,
                Ok(Err(error)) => {
                    join_set.abort_all();
                    return Err(error);
                }
                Err(error) => {
                    join_set.abort_all();
                    return Err(error).context("pull request detail hydration task failed to join");
                }
            };
            completed_batches.push((batch_index, batch_pull_requests));

            if next_batch_index < detail_batches.len() {
                self.spawn_pull_request_details_batch(
                    &mut join_set,
                    next_batch_index,
                    detail_batches[next_batch_index].clone(),
                );
                next_batch_index += 1;
            }
        }

        completed_batches.sort_by_key(|(batch_index, _)| *batch_index);

        let mut pull_requests = Vec::with_capacity(pull_request_refs.len());

        for (_, batch_pull_requests) in completed_batches {
            pull_requests.extend(batch_pull_requests);
        }

        Ok(pull_requests)
    }

    /// Spawns one detail-hydration task for a discovered pull-request batch.
    fn spawn_pull_request_details_batch(
        &self,
        join_set: &mut JoinSet<Result<(usize, Vec<NormalizedPullRequest>)>>,
        batch_index: usize,
        pull_request_refs: Vec<DiscoveredPullRequestRef>,
    ) {
        let client = self.clone();

        join_set.spawn(async move {
            let pull_requests = client
                .fetch_pull_request_details_batch(&pull_request_refs)
                .await?;

            Ok((batch_index, pull_requests))
        });
    }

    /// Runs the lightweight authored-PR search query and collects discovered PR ids.
    async fn search_authored_pull_request_refs(
        &self,
        username: &str,
        search_query: &str,
    ) -> Result<Vec<DiscoveredPullRequestRef>> {
        let mut cursor = None;
        let mut pull_request_refs = Vec::new();
        let mut seen_node_ids = HashSet::new();

        loop {
            let response = self
                .post_graphql::<SearchPullRequestsData>(
                    SEARCH_AUTHORED_PULL_REQUESTS_QUERY,
                    json!({
                        "query": search_query,
                        "cursor": cursor,
                    }),
                    &format!("failed to search authored pull requests for GitHub user {username}"),
                )
                .await?;
            let page_info = response.search.page_info;

            for node in response.search.nodes.into_iter().flatten() {
                if let Some(pull_request_ref) = Self::normalize_search_ref_node(node)?
                    && seen_node_ids.insert(pull_request_ref.node_id.clone())
                {
                    pull_request_refs.push(pull_request_ref);
                }
            }

            if !page_info.has_next_page {
                break;
            }

            cursor = Some(page_info.end_cursor.context(format!(
                "GitHub GraphQL search page for user {username} is missing endCursor"
            ))?);
        }

        Ok(pull_request_refs)
    }

    /// Converts one lightweight search node into an internal discovered-PR reference.
    fn normalize_search_ref_node(
        node: SearchPullRequestRefNode,
    ) -> Result<Option<DiscoveredPullRequestRef>> {
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

        let number = required(
            node.number,
            format!("pull request node {node_id} is missing number"),
        )?;

        Ok(Some(DiscoveredPullRequestRef {
            node_id,
            repository_owner: repository.owner.username,
            repository_name: repository.name,
            number,
        }))
    }

    /// Hydrates one batch of discovered pull requests with full detail payloads.
    async fn fetch_pull_request_details_batch(
        &self,
        pull_request_refs: &[DiscoveredPullRequestRef],
    ) -> Result<Vec<NormalizedPullRequest>> {
        let response = self
            .post_graphql::<PullRequestDetailsData>(
                PULL_REQUEST_DETAILS_QUERY,
                json!({
                    "ids": pull_request_refs
                        .iter()
                        .map(|pull_request_ref| pull_request_ref.node_id.as_str())
                        .collect::<Vec<_>>(),
                }),
                &format!(
                    "failed to fetch details for {} discovered pull requests",
                    pull_request_refs.len()
                ),
            )
            .await?;
        let mut nodes_by_id = HashMap::with_capacity(response.nodes.len());

        for node in response.nodes.into_iter().flatten() {
            let node_id = required(
                node.id.clone(),
                "pull request details batch included a node without id",
            )?;
            nodes_by_id.insert(node_id, node);
        }

        let mut pull_requests = Vec::with_capacity(pull_request_refs.len());

        for pull_request_ref in pull_request_refs {
            let node = nodes_by_id.remove(&pull_request_ref.node_id).context(format!(
                "GitHub GraphQL details response did not include pull request {} / {}#{} (node {})",
                pull_request_ref.repository_owner,
                pull_request_ref.repository_name,
                pull_request_ref.number,
                pull_request_ref.node_id,
            ))?;

            if let Some(pull_request) = self.normalize_pull_request_details_node(node).await? {
                pull_requests.push(pull_request);
            }
        }

        Ok(pull_requests)
    }

    /// Converts one fully hydrated pull-request node into our normalized PR shape.
    async fn normalize_pull_request_details_node(
        &self,
        node: PullRequestDetailsNode,
    ) -> Result<Option<NormalizedPullRequest>> {
        if node.typename != "PullRequest" {
            bail!(
                "GitHub GraphQL details batch returned non-pull-request node type {}",
                node.typename
            )
        }

        let node_id = required(node.id, "pull request details node is missing node id")?;
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
        // The details batch eagerly fetches the first commit page so most PRs can be
        // normalized without additional commit-only round trips.
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
mod tests;
