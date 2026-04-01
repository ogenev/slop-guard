CREATE INDEX IF NOT EXISTS idx_sync_runs_account_started_at ON sync_runs(account_id, started_at DESC)
