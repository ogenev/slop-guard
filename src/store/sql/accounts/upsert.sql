INSERT INTO accounts (username)
VALUES (?)
ON CONFLICT(username) DO UPDATE SET
    updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
RETURNING id
