//! Learning graph data structures — Phase 4 of the Hermes integration.
//!
//! A `LearningGraph` captures the relationship between **skills** (the
//! procedural knowledge the agent has access to) and **memory cards**
//! (declarative observations extracted from past turns). The graph
//! supports:
//!
//! - **Skill ↔ Skill** edges declared in frontmatter
//!   (`metadata.hermes.related_skills`).
//! - **Memory ↔ Skill** edges inferred from lexical overlap (top-4
//!   matching skills per card by shared keywords).
//! - **Cluster** grouping by `category` / `state` for downstream
//!   visualization (TUI or web).
//!
//! Translated from Hermes Agent's `agent/learning_graph.py`. Designed
//! to round-trip cleanly through JSON for serialization in
//! `~/.heartq/memory/learning_graph.json`.

use std::collections::{BTreeMap, HashSet};

use serde::{Deserialize, Serialize};

/// A single skill node in the learning graph.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillNode {
    pub name: String,
    /// Top-level category (`software-development`, `research`, …) the
    /// skill belongs to.
    #[serde(default)]
    pub category: String,
    /// Where the skill lives: `base` (bundled), `profile` (user-local),
    /// `agent-created`, or `hub-installed`.
    #[serde(default)]
    pub source: String,
    /// Unix epoch seconds the skill was last touched (modified on disk).
    #[serde(default)]
    pub timestamp: Option<i64>,
    /// Number of times the skill has been invoked.
    #[serde(default)]
    pub use_count: u32,
    /// `active` or `archived` (curator output).
    #[serde(default = "default_active")]
    pub state: String,
    /// `user`, `agent`, or `None` (bundled).
    #[serde(default)]
    pub created_by: Option<String>,
    #[serde(default)]
    pub pinned: bool,
    /// Names declared in `metadata.hermes.related_skills`.
    #[serde(default)]
    pub related: Vec<String>,
}

fn default_active() -> String {
    "active".into()
}

/// A piece of declarative memory surfaced as a graph card.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryCard {
    /// `memory` or `profile`.
    pub source: String,
    #[serde(default)]
    pub timestamp: Option<i64>,
    pub title: String,
    pub body: String,
}

/// Unified graph-node shape used for visualization frontends.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GraphNode {
    pub id: String,
    pub label: String,
    /// `skill` or `memory`.
    pub kind: String,
    #[serde(default)]
    pub timestamp: Option<i64>,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub use_count: u32,
    #[serde(default = "default_active")]
    pub state: String,
    #[serde(default)]
    pub created_by: Option<String>,
    #[serde(default)]
    pub pinned: bool,
    /// For memory nodes: `memory` or `profile`. Empty for skill nodes.
    #[serde(default)]
    pub memory_source: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GraphEdge {
    pub source: String,
    pub target: String,
}

/// A loose cluster grouping (currently by category).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Cluster {
    pub id: String,
    pub label: String,
    pub node_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct GraphStats {
    pub skill_count: usize,
    pub memory_card_count: usize,
    pub edge_count: usize,
    pub cluster_count: usize,
}

/// The full learning graph.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct LearningGraph {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    #[serde(default)]
    pub clusters: Vec<Cluster>,
    pub memory: Vec<MemoryCard>,
    pub stats: GraphStats,
}

/// Helper used by [`crate::learning::builder::LearningGraphBuilder`].
#[derive(Debug, Default)]
pub struct SkillCardInputs {
    pub skills: Vec<SkillNode>,
    pub cards: Vec<MemoryCard>,
}

impl LearningGraph {
    /// Build a graph from skill nodes and memory cards. Performs no I/O —
    /// caller (the builder) supplies the data.
    pub fn build(inputs: SkillCardInputs) -> Self {
        let SkillCardInputs { skills, cards } = inputs;

        // Build skill→skill edges from `related_skills`.
        let skill_names: HashSet<&str> = skills.iter().map(|s| s.name.as_str()).collect();
        let mut edges: Vec<GraphEdge> = Vec::new();
        for s in &skills {
            for r in &s.related {
                if skill_names.contains(r.as_str()) {
                    edges.push(GraphEdge {
                        source: format!("skill:{}", s.name),
                        target: format!("skill:{r}"),
                    });
                }
            }
        }

        // Build memory→skill edges from lexical overlap (top-4).
        for card in &cards {
            let scores = score_skills_for_memory(card, &skills);
            for (name, _score) in scores.iter().take(4) {
                edges.push(GraphEdge {
                    source: format!("memory:{}:{}", card.source, card.title),
                    target: format!("skill:{name}"),
                });
            }
        }

        // Build graph nodes.
        let mut nodes: Vec<GraphNode> = Vec::with_capacity(skills.len() + cards.len());
        for s in &skills {
            nodes.push(GraphNode {
                id: format!("skill:{}", s.name),
                label: s.name.clone(),
                kind: "skill".into(),
                timestamp: s.timestamp,
                category: s.category.clone(),
                use_count: s.use_count,
                state: s.state.clone(),
                created_by: s.created_by.clone(),
                pinned: s.pinned,
                memory_source: None,
            });
        }
        for c in &cards {
            nodes.push(GraphNode {
                id: format!("memory:{}:{}", c.source, c.title),
                label: c.title.clone(),
                kind: "memory".into(),
                timestamp: c.timestamp,
                category: String::new(),
                use_count: 0,
                state: "active".into(),
                created_by: None,
                pinned: false,
                memory_source: Some(c.source.clone()),
            });
        }

        // Build clusters: one cluster per (kind, category).
        let mut clusters_map: BTreeMap<(String, String), Vec<String>> = BTreeMap::new();
        for n in &nodes {
            let key = (n.kind.clone(), n.category.clone());
            clusters_map.entry(key).or_default().push(n.id.clone());
        }
        let clusters: Vec<Cluster> = clusters_map
            .into_iter()
            .enumerate()
            .map(|(i, ((kind, category), node_ids))| Cluster {
                id: format!("cluster-{i}"),
                label: if category.is_empty() {
                    kind.clone()
                } else {
                    format!("{kind}:{category}")
                },
                node_ids,
            })
            .collect();

        let stats = GraphStats {
            skill_count: skills.len(),
            memory_card_count: cards.len(),
            edge_count: edges.len(),
            cluster_count: clusters.len(),
        };

        LearningGraph {
            nodes,
            edges,
            clusters,
            memory: cards,
            stats,
        }
    }
}

/// Lexical-overlap score: count of distinct keywords (≥3 chars, lowercased,
/// alphanumeric) appearing in both the card body and the skill name. The
/// "skill name" is tokenized on hyphens.
fn score_skills_for_memory(card: &MemoryCard, skills: &[SkillNode]) -> Vec<(String, usize)> {
    let card_keywords = keywords(&format!("{} {}", card.title, card.body));
    if card_keywords.is_empty() {
        return Vec::new();
    }
    let mut scores: Vec<(String, usize)> = skills
        .iter()
        .map(|s| {
            let skill_keywords = keywords(&s.name);
            let overlap = card_keywords.intersection(&skill_keywords).count();
            (s.name.clone(), overlap)
        })
        .filter(|(_, score)| *score > 0)
        .collect();
    // Stable sort: highest overlap first; ties broken by skill name.
    scores.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    scores
}

/// Tokenize a string into a set of keywords: lowercase alphanumerics, length
/// ≥ 3, no duplicates.
fn keywords(text: &str) -> HashSet<String> {
    let mut set = HashSet::new();
    let mut buf = String::new();
    for c in text.chars() {
        if c.is_alphanumeric() {
            c.to_lowercase().for_each(|lc| buf.push(lc));
        } else if !buf.is_empty() {
            if buf.len() >= 3 {
                set.insert(std::mem::take(&mut buf));
            } else {
                buf.clear();
            }
        }
    }
    if buf.len() >= 3 {
        set.insert(buf);
    }
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_inputs_produce_empty_graph() {
        let g = LearningGraph::build(SkillCardInputs::default());
        assert!(g.nodes.is_empty());
        assert!(g.edges.is_empty());
        assert_eq!(g.stats.skill_count, 0);
        assert_eq!(g.stats.memory_card_count, 0);
    }

    #[test]
    fn related_skills_create_edges() {
        let skills = vec![
            SkillNode {
                name: "alpha".into(),
                category: "cat".into(),
                source: "base".into(),
                timestamp: None,
                use_count: 0,
                state: "active".into(),
                created_by: None,
                pinned: false,
                related: vec!["beta".into()],
            },
            SkillNode {
                name: "beta".into(),
                category: "cat".into(),
                source: "base".into(),
                timestamp: None,
                use_count: 0,
                state: "active".into(),
                created_by: None,
                pinned: false,
                related: vec![],
            },
        ];
        let g = LearningGraph::build(SkillCardInputs {
            skills,
            cards: vec![],
        });
        assert_eq!(g.edges.len(), 1);
        assert!(g.edges.iter().any(|e| e.source == "skill:alpha" && e.target == "skill:beta"));
    }

    #[test]
    fn memory_card_links_to_top_skills_by_lexical_overlap() {
        let skills = vec![
            SkillNode {
                name: "debug-rust".into(),
                related: vec![],
                ..blank_skill("dev")
            },
            SkillNode {
                name: "deploy".into(),
                related: vec![],
                ..blank_skill("dev")
            },
            SkillNode {
                name: "unrelated".into(),
                related: vec![],
                ..blank_skill("misc")
            },
        ];
        let cards = vec![MemoryCard {
            source: "memory".into(),
            timestamp: None,
            title: "Investigating rust panic".into(),
            body: "Found a borrow checker bug while debugging".into(),
        }];
        let g = LearningGraph::build(SkillCardInputs { skills, cards });
        let mut targets: Vec<String> = g
            .edges
            .iter()
            .filter(|e| e.source.starts_with("memory:"))
            .map(|e| e.target.clone())
            .collect();
        targets.sort();
        // "rust" and "debug" both keywords → debug-rust ranks first.
        assert!(targets.iter().any(|t| t == "skill:debug-rust"));
    }

    #[test]
    fn json_round_trips() {
        let skills = vec![SkillNode {
            name: "x".into(),
            category: "c".into(),
            source: "base".into(),
            timestamp: Some(1234),
            use_count: 2,
            state: "active".into(),
            created_by: None,
            pinned: true,
            related: vec!["y".into()],
        }];
        let cards = vec![MemoryCard {
            source: "memory".into(),
            timestamp: Some(5678),
            title: "T".into(),
            body: "B".into(),
        }];
        let g = LearningGraph::build(SkillCardInputs { skills, cards });
        let json = serde_json::to_string(&g).unwrap();
        let back: LearningGraph = serde_json::from_str(&json).unwrap();
        assert_eq!(back.stats.skill_count, 1);
        assert_eq!(back.stats.memory_card_count, 1);
        assert_eq!(back.nodes.len(), 2);
    }

    fn blank_skill(category: &str) -> SkillNode {
        SkillNode {
            name: String::new(),
            category: category.into(),
            source: "base".into(),
            timestamp: None,
            use_count: 0,
            state: "active".into(),
            created_by: None,
            pinned: false,
            related: vec![],
        }
    }
}