use crate::domain::EvidenceItem;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ArtifactFeatures {
    pub ai_signal_score: f32,
    pub slop_signal_score: f32,
    pub evidence: Vec<EvidenceItem>,
}

pub trait Analyzer {
    fn analyze(&self, artifact_body: &str) -> ArtifactFeatures;
}

#[derive(Clone, Debug, Default)]
pub struct ExplicitMarkerAnalyzer;

impl Analyzer for ExplicitMarkerAnalyzer {
    fn analyze(&self, artifact_body: &str) -> ArtifactFeatures {
        let lowered = artifact_body.to_ascii_lowercase();
        let marker_hit = [
            "generated with",
            "copilot",
            "claude code",
            "cursor",
            "codex-",
        ]
        .iter()
        .any(|marker| lowered.contains(marker));

        if marker_hit {
            ArtifactFeatures {
                ai_signal_score: 1.0,
                slop_signal_score: 0.0,
                evidence: vec![EvidenceItem {
                    summary: "Explicit AI marker detected in artifact text".to_owned(),
                    weight: 1.0,
                }],
            }
        } else {
            ArtifactFeatures::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Analyzer, ExplicitMarkerAnalyzer};

    #[test]
    fn detects_known_markers() {
        let analyzer = ExplicitMarkerAnalyzer;
        let features = analyzer.analyze("Generated with Claude Code");

        assert!(features.ai_signal_score > 0.9);
        assert_eq!(features.evidence.len(), 1);
    }
}
