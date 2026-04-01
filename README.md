# slop-guard

`slop-guard` is a Rust CLI for scoring whether a GitHub account's recent public activity looks mostly AI-generated and low-quality.

The project is intended as a maintainer triage tool, not an automatic enforcement system. The design separates:

- artifact-level AI evidence
- artifact-level slop / quality risk
- account-level predominance over a time window

The initial scope is pull-request-centric and uses local SQLite persistence.

## CLI

Current commands:

- `slop sync <username> [--days 30] [--details-concurrency 4]`
- `slop score <username> [--days 30]`
- `slop analyze <username> [--days 30] [--details-concurrency 4]`

Examples:

```bash
export GITHUB_TOKEN=...
export SLOP_DB_PATH=/tmp/slop.db

slop sync ogenev
slop sync ogenev --details-concurrency 8
slop score ogenev
slop analyze ogenev --days 90
```

Notes:

- `sync` fetches recent public authored pull requests from GitHub and stores them in SQLite
- `score` reads stored pull requests from SQLite and scores the account
- `analyze` runs `sync` and `score` in one command and prints combined JSON
- the default scoring/sync window is 30 days
- pull-request detail hydration runs with a default concurrency of 4 and can be tuned with `--details-concurrency`
- the database path can be overridden with `--db-path` or `SLOP_DB_PATH`
