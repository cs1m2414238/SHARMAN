use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::ConflictRecord;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "camelCase")]
pub enum NodeType {
    Goal,
    Task,
    File,
    Module,
    Test,
    Decision,
    Session,
}

impl NodeType {
    pub const fn as_key(self) -> &'static str {
        match self {
            Self::Goal => "goal",
            Self::Task => "task",
            Self::File => "file",
            Self::Module => "module",
            Self::Test => "test",
            Self::Decision => "decision",
            Self::Session => "session",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "camelCase")]
pub enum EdgeType {
    Contains,
    Touches,
    BelongsTo,
    DependsOn,
    Validates,
    Affects,
    ModifiedBy,
    Blocks,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GraphNode {
    pub id: String,
    pub node_type: NodeType,
    pub label: String,
    pub path: Option<String>,
    pub status: Option<String>,
    #[serde(default)]
    pub metadata: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GraphEdge {
    pub source: String,
    pub target: String,
    pub edge_type: EdgeType,
    pub evidence: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct GraphStats {
    pub goals: usize,
    pub tasks: usize,
    pub files: usize,
    pub modules: usize,
    pub tests: usize,
    pub decisions: usize,
    pub sessions: usize,
    pub edges: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectGraph {
    pub project_id: String,
    pub repository: String,
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub stats: GraphStats,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ImpactRequest {
    pub repository_path: String,
    /// Repository-relative file path or stable graph node ID.
    pub target: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "camelCase")]
pub enum RiskLevel {
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RiskFactor {
    pub key: String,
    pub label: String,
    pub points: u8,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ChangeRisk {
    pub score: u8,
    pub level: RiskLevel,
    pub factors: Vec<RiskFactor>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ImpactAnalysis {
    pub target: GraphNode,
    pub risk: ChangeRisk,
    pub affected_modules: Vec<GraphNode>,
    pub affected_tasks: Vec<GraphNode>,
    pub dependent_files: Vec<GraphNode>,
    pub validating_tests: Vec<GraphNode>,
    pub relevant_decisions: Vec<GraphNode>,
    pub related_sessions: Vec<GraphNode>,
    pub active_conflicts: Vec<ConflictRecord>,
    pub evidence_edges: Vec<GraphEdge>,
    pub tests_to_run: Vec<String>,
    pub warning: String,
}
