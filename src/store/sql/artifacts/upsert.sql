INSERT INTO artifacts (
    account_id,
    repository_id,
    kind,
    external_id,
    pr_number,
    title,
    body,
    state,
    created_at,
    updated_at,
    additions,
    deletions,
    changed_files,
    base_branch,
    head_branch
)
VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
ON CONFLICT(kind, external_id) DO UPDATE SET
    account_id = excluded.account_id,
    repository_id = excluded.repository_id,
    pr_number = excluded.pr_number,
    title = excluded.title,
    body = excluded.body,
    state = excluded.state,
    created_at = excluded.created_at,
    updated_at = excluded.updated_at,
    additions = excluded.additions,
    deletions = excluded.deletions,
    changed_files = excluded.changed_files,
    base_branch = excluded.base_branch,
    head_branch = excluded.head_branch
RETURNING id
