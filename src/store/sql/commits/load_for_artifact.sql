SELECT sha, message, authored_at, committed_at
FROM commits
WHERE artifact_id = ?
ORDER BY datetime(COALESCE(committed_at, authored_at)) ASC, id ASC
