CREATE INDEX IF NOT EXISTS idx_commits_artifact_sha ON commits(artifact_id, sha)
