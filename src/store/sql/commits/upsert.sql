INSERT INTO commits (
    artifact_id,
    sha,
    message,
    authored_at,
    committed_at
)
VALUES (?, ?, ?, ?, ?)
ON CONFLICT(artifact_id, sha) DO UPDATE SET
    message = excluded.message,
    authored_at = excluded.authored_at,
    committed_at = excluded.committed_at
RETURNING id
