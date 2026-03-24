use anyhow::Result;
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
    let cli = Cli::parse();

    match cli.command {
        Command::SyncAccount { login, days } => sync_account(login, days).await?,
        Command::ScoreAccount { login, window_days } => score_account(login, window_days)?,
        Command::ShowLayout => show_layout()?,
    }

    Ok(())
}

async fn sync_account(login: String, days: u16) -> Result<()> {
    let store = Store::connect("sqlite::memory:").await?;
    let service = IngestService::new(GitHubClient::default(), store);
    let summary = service.sync_account(&login, days).await?;

    println!("{}", serde_json::to_string_pretty(&summary)?);
    Ok(())
}

fn score_account(login: String, window_days: u16) -> Result<()> {
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
