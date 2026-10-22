use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ExplanationCategory {
    Planner,
    Assignment,
    ParallelSafety,
    Risk,
    Context,
    Conflict,
    Verification,
    Permission,
    Replan,
    MergeReadiness,
    Freshness,
    Decision,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ExplanationReason {
    pub code: String,
    pub summary: String,
    pub detail: String,
    pub score: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceReference {
    pub kind: String,
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ExplanationRecord {
    pub id: String,
    pub project_id: String,
    pub category: ExplanationCategory,
    pub subject_type: String,
    pub subject_id: String,
    pub decision: String,
    pub outcome: String,
    pub reasons: Vec<ExplanationReason>,
    pub evidence_refs: Vec<EvidenceReference>,
    pub created_at: String,
}
