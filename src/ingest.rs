use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::github::GitHubClient;
use crate::store::{Store, SyncRunStatus};

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

        let account_id = self.store.upsert_account(login).await?;
        let run_id = self.store.start_sync_run(account_id, window_days).await?;

        // Real GitHub ingestion lands in phase 2. Phase 1 persists account + sync run metadata.
        self.store
            .finish_sync_run(run_id, SyncRunStatus::Success, 0, 0, None)
            .await?;

        Ok(SyncSummary {
            login: login.to_owned(),
            window_days,
            artifacts_discovered: 0,
        })
    }
}
