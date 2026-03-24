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
        .filter(|feature| !feature.evidence.is_empty())
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
    use crate::analyzers::{Analyzer, ArtifactFeatures, ExplicitMarkerAnalyzer};

    use super::aggregate;

    #[test]
    fn aggregates_explicit_marker_ratio() {
        let analyzer = ExplicitMarkerAnalyzer;
        let features = vec![
            analyzer.analyze("Generated with Claude Code"),
            ArtifactFeatures::default(),
        ];

        let aggregated = aggregate(&features);

        assert_eq!(aggregated.artifact_count, 2);
        assert_eq!(aggregated.explicit_marker_ratio, 0.5);
    }
}
