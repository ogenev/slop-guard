# slop-guard

`slop-guard` is a Rust CLI for scoring whether a GitHub account's recent public activity looks mostly AI-generated and low-quality.

The project is intended as a maintainer triage tool, not an automatic enforcement system. The design separates:

- artifact-level AI evidence
- artifact-level slop / quality risk
- account-level predominance over a time window

The initial scope is pull-request-centric and uses local SQLite persistence.
