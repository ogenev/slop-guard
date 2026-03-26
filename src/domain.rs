use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Artifact categories the system can ingest, analyze, and score.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactKind {
    PullRequest,
    Issue,
    IssueComment,
    PullRequestReview,
    PullRequestReviewComment,
}

/// Final label emitted by the deterministic scoring layer.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RiskLabel {
    Clear,
    Watch,
    Review,
    HighRisk,
    Abstain,
}

/// Human-readable evidence item explaining part of a score.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct EvidenceItem {
    pub summary: String,
    pub weight: f32,
}

/// Account identity and time-window metadata attached to derived outputs.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AccountWindow {
    pub username: String,
    pub window_days: u16,
    pub generated_at: DateTime<Utc>,
}

/// Deterministic risk output returned by scoring for a single account window.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct RiskScore {
    pub account: AccountWindow,
    pub ai_usage_score: f32,
    pub slop_score: f32,
    pub predominance_score: f32,
    pub final_risk_score: f32,
    pub label: RiskLabel,
    pub abstained: bool,
    pub top_evidence: Vec<EvidenceItem>,
}
