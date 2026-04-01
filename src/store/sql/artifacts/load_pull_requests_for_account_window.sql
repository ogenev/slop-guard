SELECT
    a.id AS artifact_id,
    a.account_id,
    acc.username,
    r.owner AS repository_owner,
    r.name AS repository_name,
    r.full_name AS repository_full_name,
    a.external_id,
    a.pr_number,
    a.title,
    a.body,
    a.state,
    a.created_at,
    a.updated_at,
    a.additions,
    a.deletions,
    a.changed_files,
    a.base_branch,
    a.head_branch
FROM artifacts a
JOIN accounts acc ON acc.id = a.account_id
LEFT JOIN repositories r ON r.id = a.repository_id
WHERE a.account_id = ?
  AND a.kind = ?
  AND datetime(a.created_at) >= datetime(?)
ORDER BY datetime(a.created_at) DESC, a.id DESC
