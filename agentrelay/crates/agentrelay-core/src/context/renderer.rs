use std::{collections::BTreeMap, fmt::Write};

use super::{ContextSnapshot, ContextTier};

pub fn render_markdown(snapshot: &ContextSnapshot) -> String {
    let mut output = String::new();
    writeln!(output, "# AgentRelay Context\n").expect("writing to String cannot fail");
    writeln!(output, "## Context Contract\n").expect("writing to String cannot fail");
    writeln!(output, "- Project: {}", snapshot.project_name)
        .expect("writing to String cannot fail");
    writeln!(output, "- Repository: `{}`", snapshot.repository)
        .expect("writing to String cannot fail");
    writeln!(output, "- Profile: {}", snapshot.request.profile.as_label())
        .expect("writing to String cannot fail");
    writeln!(
        output,
        "- Token budget: {} (estimated {})",
        snapshot.request.token_budget, snapshot.estimated_tokens
    )
    .expect("writing to String cannot fail");
    writeln!(
        output,
        "- Compression: {:?} — {}",
        snapshot.compression.mode, snapshot.compression.detail
    )
    .expect("writing to String cannot fail");
    writeln!(
        output,
        "- Omitted lower-relevance items: {}",
        snapshot.omitted_item_count
    )
    .expect("writing to String cannot fail");

    if let Some(summary) = &snapshot.summary {
        writeln!(output, "\n## Validated Nemotron Summary\n")
            .expect("writing to String cannot fail");
        writeln!(
            output,
            "Model: `{}` via {}.\n",
            summary.model, summary.provider
        )
        .expect("writing to String cannot fail");
        let labels = source_labels(snapshot);
        for bullet in &summary.bullets {
            let references = bullet
                .source_keys
                .iter()
                .map(|key| labels.get(key).map_or(key.as_str(), String::as_str))
                .collect::<Vec<_>>()
                .join("; ");
            writeln!(output, "- {} _[{}]_", bullet.text, references)
                .expect("writing to String cannot fail");
        }
        render_tiers(
            &mut output,
            snapshot,
            &[ContextTier::Critical, ContextTier::Active],
            "Grounded Critical and Active Context",
        );
        render_source_index(&mut output, snapshot);
    } else {
        render_tiers(
            &mut output,
            snapshot,
            &[
                ContextTier::Critical,
                ContextTier::Active,
                ContextTier::Recent,
                ContextTier::Historical,
                ContextTier::Archived,
            ],
            "Selected Grounded Context",
        );
    }

    writeln!(output, "\n## Recommended Next Action\n").expect("writing to String cannot fail");
    writeln!(output, "{}", snapshot.recommended_next_action)
        .expect("writing to String cannot fail");
    output
}

fn render_tiers(
    output: &mut String,
    snapshot: &ContextSnapshot,
    tiers: &[ContextTier],
    heading: &str,
) {
    writeln!(output, "\n## {heading}\n").expect("writing to String cannot fail");
    if snapshot.items.is_empty() {
        writeln!(output, "No relevant project records were selected.")
            .expect("writing to String cannot fail");
        return;
    }
    for tier in tiers {
        let tier_items = snapshot
            .items
            .iter()
            .filter(|item| item.tier == *tier)
            .collect::<Vec<_>>();
        if tier_items.is_empty() {
            continue;
        }
        writeln!(output, "### {:?}\n", tier).expect("writing to String cannot fail");
        for item in tier_items {
            let references = item
                .sources
                .iter()
                .map(|reference| reference.label.as_str())
                .collect::<Vec<_>>()
                .join("; ");
            writeln!(
                output,
                "- **{}** — {} _[score {}; {}]_",
                item.title, item.content, item.relevance_score, references
            )
            .expect("writing to String cannot fail");
        }
    }
}

fn render_source_index(output: &mut String, snapshot: &ContextSnapshot) {
    writeln!(output, "\n## Source Index\n").expect("writing to String cannot fail");
    let mut sources = BTreeMap::new();
    for source in snapshot.items.iter().flat_map(|item| &item.sources) {
        sources.insert(source.key.as_str(), source.label.as_str());
    }
    for (key, label) in sources {
        writeln!(output, "- `{key}` — {label}").expect("writing to String cannot fail");
    }
}

fn source_labels(snapshot: &ContextSnapshot) -> BTreeMap<String, String> {
    snapshot
        .items
        .iter()
        .flat_map(|item| &item.sources)
        .map(|source| (source.key.clone(), source.label.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::{
        CompressionState, ContextProfile, ContextRequest, ContextSnapshot, ValidatedSummary,
    };

    #[test]
    fn rendering_is_deterministic() {
        let snapshot = ContextSnapshot {
            project_name: "demo".into(),
            repository: "C:/demo".into(),
            request: ContextRequest {
                project_id: "project-1".into(),
                session_id: None,
                task_id: None,
                token_budget: 2_000,
                profile: ContextProfile::Balanced,
            },
            items: Vec::new(),
            summary: None::<ValidatedSummary>,
            compression: CompressionState::deterministic("test"),
            estimated_tokens: 100,
            omitted_item_count: 0,
            recommended_next_action: "Add a task.".into(),
        };

        assert_eq!(render_markdown(&snapshot), render_markdown(&snapshot));
        assert!(!render_markdown(&snapshot).contains("Generated at"));
    }
}
