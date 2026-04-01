SELECT COUNT(*)
FROM sqlite_master
WHERE type = 'table'
  AND name IN ('accounts', 'repositories', 'artifacts', 'commits', 'sync_runs')
