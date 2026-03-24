use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::github::GitHubClient;
use crate::store::Store;

#[derive(Clone)]
pub struct IngestService {
    client: GitHubClient,
    store: Store,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SyncSummary {
    pub login: String,
    pub window_days: u16,
    pub artifacts_discovered: usize,
}

impl IngestService {
    pub fn new(client: GitHubClient, store: Store) -> Self {
        Self { client, store }
    }

    pub async fn sync_account(&self, login: &str, window_days: u16) -> Result<SyncSummary> {
        let _ = self.client.http();
        let _ = self.store.pool();

        Ok(SyncSummary {
            login: login.to_owned(),
            window_days,
            artifacts_discovered: 0,
        })
    }
}
