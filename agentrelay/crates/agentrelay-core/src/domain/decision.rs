use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Decision {
    pub id: String,
    pub project_id: String,
    pub summary: String,
    pub reason: String,
    pub alternatives: Vec<String>,
    pub affected: Vec<String>,
    pub status: DecisionStatus,
    pub created_by: String,
    pub superseded_by: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum DecisionStatus {
    Active,
    Superseded,
    Reversed,
}

impl DecisionStatus {
    pub(crate) fn as_db(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Superseded => "superseded",
            Self::Reversed => "reversed",
        }
    }

    pub(crate) fn from_db(value: &str) -> Option<Self> {
        match value {
            "active" => Some(Self::Active),
            "superseded" => Some(Self::Superseded),
            "reversed" => Some(Self::Reversed),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateDecisionInput {
    pub repository_path: String,
    pub summary: String,
    pub reason: String,
    #[serde(default)]
    pub alternatives: Vec<String>,
    #[serde(default)]
    pub affected: Vec<String>,
    pub created_by: String,
    #[serde(default)]
    pub supersedes_decision_id: Option<String>,
}
