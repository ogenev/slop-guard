use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

use crate::analyzers::{Analyzer, ExplicitMarkerAnalyzer};
use crate::features::aggregate;
use crate::github::GitHubClient;
use crate::ingest::IngestService;
use crate::scoring::ScoreEngine;
use crate::store::Store;

#[derive(Debug, Parser)]
#[command(name = "aislop", about = "AI slop account classifier scaffold")]
pub struct Cli {
    #[arg(
        long,
        global = true,
        env = "AISLOP_DB_PATH",
        value_name = "PATH",
        help = "Path to SQLite database file"
    )]
    db_path: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    SyncAccount {
        login: String,
        #[arg(long, default_value_t = 90)]
        days: u16,
    },
    ScoreAccount {
        login: String,
        #[arg(long, default_value_t = 90)]
        window_days: u16,
    },
    ShowLayout,
}

pub async fn run() -> Result<()> {
    let Cli { db_path, command } = Cli::parse();

    match command {
        Command::SyncAccount { login, days } => {
            let database_path = resolve_database_path(db_path.clone())?;
            sync_account(&database_path, login, days).await?
        }
        Command::ScoreAccount { login, window_days } => {
            let database_path = resolve_database_path(db_path)?;
            score_account(&database_path, login, window_days)?
        }
        Command::ShowLayout => show_layout()?,
    }

    Ok(())
}

fn resolve_database_path(configured_path: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(path) = configured_path {
        return Ok(path);
    }

    let app_data_dir = dirs::data_local_dir()
        .or_else(dirs::data_dir)
        .context("unable to determine app-data directory; pass --db-path or set AISLOP_DB_PATH")?;

    Ok(app_data_dir.join("aislop").join("aislop.db"))
}

async fn sync_account(database_path: &Path, login: String, days: u16) -> Result<()> {
    let store = Store::connect(database_path).await?;
    let service = IngestService::new(GitHubClient::default(), store);
    let summary = service.sync_account(&login, days).await?;

    println!("{}", serde_json::to_string_pretty(&summary)?);
    Ok(())
}

fn score_account(_database_path: &Path, login: String, window_days: u16) -> Result<()> {
    let analyzer = ExplicitMarkerAnalyzer;
    let artifacts = vec![
        analyzer.analyze("Generated with Claude Code"),
        analyzer.analyze("Routine contributor follow-up"),
    ];
    let window = aggregate(&artifacts);
    let engine = ScoreEngine;
    let score = engine.score(&login, window_days, &window);

    println!("{}", serde_json::to_string_pretty(&score)?);
    Ok(())
}

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

    use super::resolve_database_path;

    #[test]
    fn prefers_explicit_database_path() {
        let provided = PathBuf::from("/tmp/aislop-custom.db");
        let resolved = resolve_database_path(Some(provided.clone()))
            .expect("resolving explicit database path should succeed");

        assert_eq!(resolved, provided);
    }
}
