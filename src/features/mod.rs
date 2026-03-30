use crate::analyzers::ArtifactFeatures;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct AccountFeatureWindow {
    pub artifact_count: usize,
    pub ai_signal_average: f32,
    pub slop_signal_average: f32,
    pub explicit_marker_ratio: f32,
}

pub fn aggregate(features: &[ArtifactFeatures]) -> AccountFeatureWindow {
    if features.is_empty() {
        return AccountFeatureWindow::default();
    }

    let artifact_count = features.len();
    let ai_signal_sum: f32 = features.iter().map(|feature| feature.ai_signal_score).sum();
    let slop_signal_sum: f32 = features
        .iter()
        .map(|feature| feature.slop_signal_score)
        .sum();
    let explicit_marker_hits = features
        .iter()
        .filter(|feature| feature.has_explicit_marker)
        .count();
    let artifact_count_f32 = artifact_count as f32;

    AccountFeatureWindow {
        artifact_count,
        ai_signal_average: ai_signal_sum / artifact_count_f32,
        slop_signal_average: slop_signal_sum / artifact_count_f32,
        explicit_marker_ratio: explicit_marker_hits as f32 / artifact_count_f32,
    }
}

#[cfg(test)]
mod tests {
    use crate::{analyzers::ArtifactFeatures, domain::EvidenceItem};

    use super::aggregate;

    #[test]
    fn aggregates_explicit_marker_ratio() {
        let features = vec![
            ArtifactFeatures {
                has_explicit_marker: true,
                ai_signal_score: 1.0,
                slop_signal_score: 0.0,
                evidence: vec![EvidenceItem {
                    summary: "Explicit AI marker detected in pull request body".to_owned(),
                    weight: 1.0,
                }],
            },
            ArtifactFeatures::default(),
        ];

        let aggregated = aggregate(&features);

        assert_eq!(aggregated.artifact_count, 2);
        assert_eq!(aggregated.explicit_marker_ratio, 0.5);
    }

    #[test]
    fn ignores_non_marker_evidence_when_aggregating_marker_ratio() {
        let aggregated = aggregate(&[ArtifactFeatures {
            has_explicit_marker: false,
            ai_signal_score: 0.0,
            slop_signal_score: 0.25,
            evidence: vec![EvidenceItem {
                summary: "Oversized pull request heuristic triggered".to_owned(),
                weight: 0.25,
            }],
        }]);

        assert_eq!(aggregated.artifact_count, 1);
        assert_eq!(aggregated.explicit_marker_ratio, 0.0);
    }
}
