use crate::{domain::EvidenceItem, store::PullRequestReadModel};

/// Typed analyzer output produced for one stored artifact.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ArtifactFeatures {
    pub has_explicit_marker: bool,
    pub ai_signal_score: f32,
    pub slop_signal_score: f32,
    pub evidence: Vec<EvidenceItem>,
}

/// Analyzer contract for deriving artifact features from stored pull requests.
pub trait Analyzer {
    fn analyze(&self, artifact: &PullRequestReadModel) -> ArtifactFeatures;
}

/// Detects explicit textual disclosures that an artifact was AI-generated.
#[derive(Clone, Debug, Default)]
pub struct ExplicitMarkerAnalyzer;

impl Analyzer for ExplicitMarkerAnalyzer {
    fn analyze(&self, artifact: &PullRequestReadModel) -> ArtifactFeatures {
        let mut evidence = Vec::new();

        if contains_explicit_marker(&artifact.title) {
            evidence.push(EvidenceItem {
                summary: "Explicit AI marker detected in pull request title".to_owned(),
                weight: 1.0,
            });
        }

        if artifact
            .body
            .as_deref()
            .is_some_and(contains_explicit_marker)
        {
            evidence.push(EvidenceItem {
                summary: "Explicit AI marker detected in pull request body".to_owned(),
                weight: 1.0,
            });
        }

        let commit_marker_hits = artifact
            .commits
            .iter()
            .filter(|commit| contains_explicit_marker(&commit.message))
            .count();
        if commit_marker_hits > 0 {
            evidence.push(EvidenceItem {
                summary: format!(
                    "Explicit AI marker detected in {} commit message{}",
                    commit_marker_hits,
                    if commit_marker_hits == 1 { "" } else { "s" }
                ),
                weight: 1.0,
            });
        }

        if evidence.is_empty() {
            ArtifactFeatures::default()
        } else {
            ArtifactFeatures {
                has_explicit_marker: true,
                ai_signal_score: 1.0,
                slop_signal_score: 0.0,
                evidence,
            }
        }
    }
}

/// Runs the currently wired analyzer set over a stored pull-request window.
pub fn analyze_pull_requests(artifacts: &[PullRequestReadModel]) -> Vec<ArtifactFeatures> {
    let analyzer = ExplicitMarkerAnalyzer;

    artifacts
        .iter()
        .map(|artifact| analyzer.analyze(artifact))
        .collect()
}

fn contains_explicit_marker(text: &str) -> bool {
    let lowered = text.to_ascii_lowercase();

    [
        "generated with",
        "copilot",
        "claude code",
        "cursor",
        "codex-",
    ]
    .iter()
    .any(|marker| lowered.contains(marker))
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, Utc};

    use crate::store::{PullRequestCommitReadModel, PullRequestReadModel};

    use super::{Analyzer, ExplicitMarkerAnalyzer, analyze_pull_requests};

    fn parse_timestamp(value: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(value)
            .expect("timestamp fixture should parse")
            .with_timezone(&Utc)
    }

    fn build_pull_request(
        title: &str,
        body: Option<&str>,
        commit_messages: &[&str],
    ) -> PullRequestReadModel {
        PullRequestReadModel {
            artifact_id: 1,
            account_id: 1,
            username: "ogi".to_owned(),
            repository_owner: "rust-lang".to_owned(),
            repository_name: "cargo".to_owned(),
            repository_full_name: "rust-lang/cargo".to_owned(),
            external_id: "9001".to_owned(),
            number: 42,
            title: title.to_owned(),
            body: body.map(str::to_owned),
            state: "open".to_owned(),
            created_at: parse_timestamp("2026-03-01T10:00:00Z"),
            updated_at: parse_timestamp("2026-03-01T11:00:00Z"),
            additions: 17,
            deletions: 4,
            changed_files: 3,
            base_branch: Some("main".to_owned()),
            head_branch: Some("topic/coverage".to_owned()),
            commits: commit_messages
                .iter()
                .enumerate()
                .map(|(index, message)| PullRequestCommitReadModel {
                    sha: format!("sha-{index}"),
                    message: (*message).to_owned(),
                    authored_at: Some(Utc::now()),
                    committed_at: Some(Utc::now()),
                })
                .collect(),
        }
    }

    #[test]
    fn detects_known_markers_in_pull_request_body() {
        let analyzer = ExplicitMarkerAnalyzer;
        let pull_request = build_pull_request(
            "Improve parser coverage",
            Some("Generated with Claude Code"),
            &[],
        );

        let features = analyzer.analyze(&pull_request);

        assert!(features.has_explicit_marker);
        assert!(features.ai_signal_score > 0.9);
        assert_eq!(features.evidence.len(), 1);
        assert_eq!(
            features.evidence[0].summary,
            "Explicit AI marker detected in pull request body"
        );
    }

    #[test]
    fn detects_known_markers_in_commit_messages() {
        let analyzer = ExplicitMarkerAnalyzer;
        let pull_request = build_pull_request(
            "Routine follow-up",
            Some("No disclosure here"),
            &["Generated with Cursor", "test: add coverage"],
        );

        let features = analyzer.analyze(&pull_request);

        assert!(features.has_explicit_marker);
        assert_eq!(features.evidence.len(), 1);
        assert_eq!(
            features.evidence[0].summary,
            "Explicit AI marker detected in 1 commit message"
        );
    }

    #[test]
    fn analyzes_pull_request_windows() {
        let pull_requests = vec![
            build_pull_request("PR 1", Some("Generated with Claude Code"), &[]),
            build_pull_request("PR 2", Some("Routine contributor follow-up"), &[]),
        ];

        let features = analyze_pull_requests(&pull_requests);

        assert_eq!(features.len(), 2);
        assert!(features[0].has_explicit_marker);
        assert!(!features[1].has_explicit_marker);
    }
}
