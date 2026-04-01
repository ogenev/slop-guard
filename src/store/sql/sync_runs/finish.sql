UPDATE sync_runs
SET status = ?,
    finished_at = ?,
    artifacts_discovered = ?,
    artifacts_stored = ?,
    error_message = ?
WHERE id = ?
