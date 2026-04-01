INSERT INTO repositories (owner, name, full_name)
VALUES (?, ?, ?)
ON CONFLICT(full_name) DO UPDATE SET
    owner = excluded.owner,
    name = excluded.name,
    updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
RETURNING id
