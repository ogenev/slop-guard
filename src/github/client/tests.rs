use std::time::{Duration, Instant};

use reqwest::Url;
use serde_json::{Value, json};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{body_string_contains, method, path},
};

use super::{
    DEFAULT_PULL_REQUEST_DETAILS_CONCURRENCY, GitHubClient, SEARCH_AUTHORED_PULL_REQUESTS_QUERY,
};

#[test]
fn authored_pull_request_search_uses_larger_page_size() {
    assert!(SEARCH_AUTHORED_PULL_REQUESTS_QUERY.contains("first: 50"));
    assert!(!SEARCH_AUTHORED_PULL_REQUESTS_QUERY.contains("createdAt"));
}

#[test]
fn debug_output_redacts_the_github_token() {
    let client = GitHubClient::new("test-agent", "super-secret-token")
        .expect("test GitHub client should be created");

    let debug_output = format!("{client:?}");

    assert!(debug_output.contains("<redacted>"));
    assert!(!debug_output.contains("super-secret-token"));
}

#[test]
fn configures_pull_request_details_concurrency() {
    let client = GitHubClient::new("test-agent", "super-secret-token")
        .expect("test GitHub client should be created");
    assert_eq!(
        client.pull_request_details_concurrency(),
        DEFAULT_PULL_REQUEST_DETAILS_CONCURRENCY
    );

    let client = client
        .with_pull_request_details_concurrency(8)
        .expect("custom concurrency should be accepted");
    assert_eq!(client.pull_request_details_concurrency(), 8);

    let error = client
        .with_pull_request_details_concurrency(0)
        .expect_err("zero concurrency should be rejected");
    assert!(error.to_string().contains("at least 1"));
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
                        "number": 99,
                        "createdAt": "2026-03-05T10:00:00Z",
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
        Some("PR_kwDOAAABc843"),
        json!({
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
async fn hydrates_pull_request_details_batches_concurrently() {
    let server = MockServer::start().await;
    let client = test_client(&server);
    let discovered_nodes = (0..40)
        .map(discovered_pull_request_node)
        .collect::<Vec<_>>();
    let first_batch_details = (0..20).map(hydrated_pull_request_node).collect::<Vec<_>>();
    let second_batch_details = (20..40).map(hydrated_pull_request_node).collect::<Vec<_>>();

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
                "nodes": discovered_nodes
            }
        }),
    )
    .await;
    mock_graphql_response_with_template(
        &server,
        "PullRequestDetailsBatch",
        Some("PR_batch_00"),
        ResponseTemplate::new(200)
            .set_delay(Duration::from_millis(750))
            .set_body_json(json!({ "data": { "nodes": first_batch_details } })),
    )
    .await;
    mock_graphql_response_with_template(
        &server,
        "PullRequestDetailsBatch",
        Some("PR_batch_20"),
        ResponseTemplate::new(200)
            .set_delay(Duration::from_millis(750))
            .set_body_json(json!({ "data": { "nodes": second_batch_details } })),
    )
    .await;

    let started_at = Instant::now();
    let pull_requests = client
        .fetch_authored_pull_requests("octocat", 30)
        .await
        .expect("pull requests should be fetched");
    let elapsed = started_at.elapsed();

    assert_eq!(pull_requests.len(), 40);
    assert_eq!(pull_requests[0].external_id, "1000");
    assert_eq!(pull_requests[19].external_id, "1019");
    assert_eq!(pull_requests[20].external_id, "1020");
    assert_eq!(pull_requests[39].external_id, "1039");
    assert!(
        elapsed < Duration::from_millis(1300),
        "expected detail batches to overlap, but fetch took {elapsed:?}"
    );
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
                        "number": 7,
                        "createdAt": "2026-03-01T10:00:00Z",
                        "repository": {
                            "name": "secret-repo",
                            "isPrivate": true,
                            "owner": { "username": "private-org" }
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

fn discovered_pull_request_node(index: usize) -> Value {
    json!({
        "__typename": "PullRequest",
        "id": format!("PR_batch_{index:02}"),
        "number": index as i64 + 1,
        "createdAt": format!("2026-03-{:02}T10:00:00Z", (index % 28) + 1),
        "repository": {
            "name": "cargo",
            "isPrivate": false,
            "owner": { "username": "rust-lang" }
        }
    })
}

fn hydrated_pull_request_node(index: usize) -> Value {
    json!({
        "__typename": "PullRequest",
        "id": format!("PR_batch_{index:02}"),
        "databaseId": 1000 + index as i64,
        "number": index as i64 + 1,
        "title": format!("Batch pull request {index}"),
        "body": null,
        "state": "OPEN",
        "createdAt": format!("2026-03-{:02}T10:00:00Z", (index % 28) + 1),
        "updatedAt": format!("2026-03-{:02}T11:00:00Z", (index % 28) + 1),
        "additions": 1,
        "deletions": 0,
        "changedFiles": 1,
        "baseRefName": "main",
        "headRefName": format!("topic/batch-{index}"),
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
            "nodes": []
        }
    })
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
    mock_graphql_response_with_template(
        server,
        operation_name,
        body_fragment,
        ResponseTemplate::new(200).set_body_json(json!({ "data": data })),
    )
    .await;
}

async fn mock_graphql_response_with_template(
    server: &MockServer,
    operation_name: &str,
    body_fragment: Option<&str>,
    template: ResponseTemplate,
) {
    let body_fragment = body_fragment.unwrap_or_default().to_owned();

    Mock::given(method("POST"))
        .and(path("/graphql"))
        .and(body_string_contains(operation_name))
        .and(body_string_contains(body_fragment))
        .respond_with(template)
        .expect(1)
        .mount(server)
        .await;
}
