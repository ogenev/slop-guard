SELECT status, artifacts_discovered, artifacts_stored, error_message
FROM sync_runs
ORDER BY id DESC
LIMIT 1
