use std::{
    num::NonZeroUsize,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use serde::Serialize;

use crate::analyzers::analyze_pull_requests;
use crate::domain::RiskScore;
use crate::features::aggregate;
use crate::github::{DEFAULT_PULL_REQUEST_DETAILS_CONCURRENCY, GitHubClient};
use crate::ingest::{IngestService, SyncSummary};
use crate::scoring::ScoreEngine;
use crate::store::Store;

/// Top-level CLI definition for the current vertical slice.
#[derive(Debug, Parser)]
#[command(name = "slop", about = "AI slop account classifier scaffold")]
pub struct Cli {
    #[arg(
        long,
        global = true,
        env = "SLOP_DB_PATH",
        value_name = "PATH",
        help = "Path to SQLite database file"
    )]
    db_path: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Sync {
        username: String,
        #[arg(long, default_value_t = 30)]
        days: u16,
        #[arg(
            long,
            default_value_t = NonZeroUsize::new(DEFAULT_PULL_REQUEST_DETAILS_CONCURRENCY)
                .expect("default details concurrency should be non-zero"),
            value_name = "N",
            help = "Maximum number of concurrent pull-request detail batches to hydrate"
        )]
        details_concurrency: NonZeroUsize,
    },
    Score {
        username: String,
        #[arg(long = "days", default_value_t = 30)]
        days: u16,
    },
    Analyze {
        username: String,
        #[arg(long, default_value_t = 30)]
        days: u16,
        #[arg(
            long,
            default_value_t = NonZeroUsize::new(DEFAULT_PULL_REQUEST_DETAILS_CONCURRENCY)
                .expect("default details concurrency should be non-zero"),
            value_name = "N",
            help = "Maximum number of concurrent pull-request detail batches to hydrate"
        )]
        details_concurrency: NonZeroUsize,
    },
    ShowLayout,
}

/// Combined output emitted by the unified analyze command.
#[derive(Debug, Serialize)]
struct AnalyzeAccountOutput {
    sync: SyncSummary,
    score: RiskScore,
}

/// Parses CLI arguments and dispatches to the selected command.
pub async fn run() -> Result<()> {
    let Cli { db_path, command } = Cli::parse();

    match command {
        Command::Sync {
            username,
            days,
            details_concurrency,
        } => {
            let database_path = resolve_database_path(db_path.clone())?;
            let summary =
                sync_account(&database_path, &username, days, details_concurrency.get()).await?;
            println!("{}", serde_json::to_string_pretty(&summary)?);
        }
        Command::Score { username, days } => {
            let database_path = resolve_database_path(db_path.clone())?;
            let score = score_account(&database_path, &username, days).await?;
            println!("{}", serde_json::to_string_pretty(&score)?);
        }
        Command::Analyze {
            username,
            days,
            details_concurrency,
        } => {
            let database_path = resolve_database_path(db_path)?;
            let sync =
                sync_account(&database_path, &username, days, details_concurrency.get()).await?;
            let score = score_account(&database_path, &username, days).await?;
            let output = AnalyzeAccountOutput { sync, score };
            println!("{}", serde_json::to_string_pretty(&output)?);
        }
        Command::ShowLayout => show_layout()?,
    }

    Ok(())
}

/// Resolves the SQLite database path from CLI input or the OS app-data directory.
fn resolve_database_path(configured_path: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(path) = configured_path {
        return Ok(path);
    }

    let app_data_dir = dirs::data_local_dir()
        .or_else(dirs::data_dir)
        .context("unable to determine app-data directory; pass --db-path or set SLOP_DB_PATH")?;

    Ok(app_data_dir.join("slop").join("slop.db"))
}

/// Runs the real GitHub ingestion flow and returns the sync summary.
async fn sync_account(
    database_path: &Path,
    username: &str,
    days: u16,
    details_concurrency: usize,
) -> Result<SyncSummary> {
    let store = Store::connect(database_path).await?;
    let client =
        GitHubClient::from_env()?.with_pull_request_details_concurrency(details_concurrency)?;
    let service = IngestService::new(client, store);
    service.sync_account(username, days).await
}

/// Scores an account window from stored SQLite artifacts using the analyze-on-read flow.
async fn score_account(database_path: &Path, username: &str, days: u16) -> Result<RiskScore> {
    let store = Store::connect(database_path).await?;
    let artifacts = store
        .load_pull_requests_for_account_window(username, days)
        .await?;
    let analyzed_artifacts = analyze_pull_requests(&artifacts);
    let window = aggregate(&analyzed_artifacts);
    let engine = ScoreEngine;

    Ok(engine.score(username, days, &window))
}

/// Prints the current package/module layout for quick inspection.
fn show_layout() -> Result<()> {
    let layout = serde_json::json!({
        "package": "slop-guard",
        "modules": [
            "src/lib.rs",
            "src/main.rs",
            "src/cli.rs",
            "src/domain.rs",
            "src/ingest.rs",
            "src/github/",
            "src/store/",
            "src/analyzers/",
            "src/features/",
            "src/scoring/"
        ]
    });

    println!("{}", serde_json::to_string_pretty(&layout)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use clap::Parser;

    use crate::github::DEFAULT_PULL_REQUEST_DETAILS_CONCURRENCY;

    use super::{Cli, Command, resolve_database_path};

    #[test]
    fn prefers_explicit_database_path() {
        let provided = PathBuf::from("/tmp/slop-custom.db");
        let resolved = resolve_database_path(Some(provided.clone()))
            .expect("resolving explicit database path should succeed");

        assert_eq!(resolved, provided);
    }

    #[test]
    fn parses_renamed_commands() {
        let cli = Cli::try_parse_from(["slop", "sync", "ogi", "--days", "30"])
            .expect("sync command should parse");
        match cli.command {
            Command::Sync {
                username,
                days,
                details_concurrency,
            } => {
                assert_eq!(username, "ogi");
                assert_eq!(days, 30);
                assert_eq!(
                    details_concurrency.get(),
                    DEFAULT_PULL_REQUEST_DETAILS_CONCURRENCY
                );
            }
            other => panic!("expected sync command, got {other:?}"),
        }

        let cli = Cli::try_parse_from(["slop", "score", "ogi", "--days", "45"])
            .expect("score command should parse");
        match cli.command {
            Command::Score { username, days } => {
                assert_eq!(username, "ogi");
                assert_eq!(days, 45);
            }
            other => panic!("expected score command, got {other:?}"),
        }

        let cli = Cli::try_parse_from([
            "slop",
            "analyze",
            "ogi",
            "--days",
            "60",
            "--details-concurrency",
            "8",
        ])
        .expect("analyze command should parse");
        match cli.command {
            Command::Analyze {
                username,
                days,
                details_concurrency,
            } => {
                assert_eq!(username, "ogi");
                assert_eq!(days, 60);
                assert_eq!(details_concurrency.get(), 8);
            }
            other => panic!("expected analyze command, got {other:?}"),
        }
    }

    #[test]
    fn rejects_zero_details_concurrency() {
        let error = Cli::try_parse_from(["slop", "sync", "ogi", "--details-concurrency", "0"])
            .expect_err("zero details concurrency should be rejected");

        assert!(error.to_string().contains("zero"));
    }
}
