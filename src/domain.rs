use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactKind {
    PullRequest,
    Issue,
    IssueComment,
    PullRequestReview,
    PullRequestReviewComment,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RiskLabel {
    Clear,
    Watch,
    Review,
    HighRisk,
    Abstain,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct EvidenceItem {
    pub summary: String,
    pub weight: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AccountWindow {
    pub login: String,
    pub window_days: u16,
    pub generated_at: DateTime<Utc>,
}

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
