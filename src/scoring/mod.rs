use chrono::Utc;

use crate::domain::{AccountWindow, EvidenceItem, RiskLabel, RiskScore};
use crate::features::AccountFeatureWindow;

/// Applies the current deterministic heuristic weights to an account feature window.
#[derive(Clone, Debug, Default)]
pub struct ScoreEngine;

impl ScoreEngine {
    /// Produces a score object with label, abstain state, and human-readable evidence.
    pub fn score(
        &self,
        username: &str,
        window_days: u16,
        features: &AccountFeatureWindow,
    ) -> RiskScore {
        let ai_usage_score = features.ai_signal_average;
        let slop_score = features.slop_signal_average;
        let predominance_score =
            ((features.ai_signal_average + features.explicit_marker_ratio) / 2.0).clamp(0.0, 1.0);
        let final_risk_score =
            ((ai_usage_score * 0.45) + (slop_score * 0.35) + (predominance_score * 0.20))
                .clamp(0.0, 1.0);
        // With no artifacts, we intentionally abstain instead of guessing from empty input.
        let abstained = features.artifact_count == 0;
        let label = if abstained {
            RiskLabel::Abstain
        } else if final_risk_score >= 0.8 {
            RiskLabel::HighRisk
        } else if final_risk_score >= 0.55 {
            RiskLabel::Review
        } else if final_risk_score >= 0.3 {
            RiskLabel::Watch
        } else {
            RiskLabel::Clear
        };

        let mut top_evidence = Vec::new();

        if features.explicit_marker_ratio > 0.0 {
            top_evidence.push(EvidenceItem {
                summary: format!(
                    "{:.0}% of analyzed artifacts include explicit AI markers",
                    features.explicit_marker_ratio * 100.0
                ),
                weight: features.explicit_marker_ratio,
            });
        }

        if features.ai_signal_average > 0.0 {
            top_evidence.push(EvidenceItem {
                summary: format!(
                    "Average AI signal score is {:.2}",
                    features.ai_signal_average
                ),
                weight: features.ai_signal_average,
            });
        }

        if features.slop_signal_average > 0.0 {
            top_evidence.push(EvidenceItem {
                summary: format!(
                    "Average slop signal score is {:.2}",
                    features.slop_signal_average
                ),
                weight: features.slop_signal_average,
            });
        }

        RiskScore {
            account: AccountWindow {
                username: username.to_owned(),
                window_days,
                generated_at: Utc::now(),
            },
            ai_usage_score,
            slop_score,
            predominance_score,
            final_risk_score,
            label,
            abstained,
            top_evidence,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::domain::RiskLabel;
    use crate::features::AccountFeatureWindow;

    use super::ScoreEngine;

    #[test]
    fn abstains_without_artifacts() {
        let engine = ScoreEngine;
        let score = engine.score("example", 90, &AccountFeatureWindow::default());

        assert_eq!(score.label, RiskLabel::Abstain);
        assert!(score.abstained);
    }
}
