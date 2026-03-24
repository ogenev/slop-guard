use reqwest::Client;

#[derive(Clone, Debug)]
pub struct GitHubClient {
    inner: Client,
    user_agent: String,
}

impl GitHubClient {
    pub fn new(user_agent: impl Into<String>) -> Self {
        let user_agent = user_agent.into();
        let inner = Client::builder()
            .user_agent(user_agent.clone())
            .build()
            .expect("default GitHub client configuration should be valid");

        Self { inner, user_agent }
    }

    pub fn http(&self) -> &Client {
        &self.inner
    }

    pub fn user_agent(&self) -> &str {
        &self.user_agent
    }
}

impl Default for GitHubClient {
    fn default() -> Self {
        Self::new("slop-guard/dev")
    }
}
