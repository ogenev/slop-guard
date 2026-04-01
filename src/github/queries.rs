/// GitHub GraphQL documents used by the client.
pub(crate) const ACCOUNT_LOOKUP_QUERY: &str = include_str!("queries/account_lookup.graphql");
pub(crate) const SEARCH_AUTHORED_PULL_REQUESTS_QUERY: &str =
    include_str!("queries/search_authored_pull_requests.graphql");
pub(crate) const PULL_REQUEST_DETAILS_QUERY: &str =
    include_str!("queries/pull_request_details.graphql");
pub(crate) const PULL_REQUEST_COMMITS_PAGE_QUERY: &str =
    include_str!("queries/pull_request_commits_page.graphql");
