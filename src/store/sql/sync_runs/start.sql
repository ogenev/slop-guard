INSERT INTO sync_runs (
    account_id,
    window_days,
    status,
    started_at
)
VALUES (?, ?, ?, ?)
RETURNING id
