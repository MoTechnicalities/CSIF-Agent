use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt::{Display, Formatter};
use std::fs;
use std::path::Path;

pub type Result<T> = std::result::Result<T, RWIFError>;

#[derive(Debug)]
pub enum RWIFError {
    EdgeNotFound(String),
    Io(std::io::Error),
    Serde(serde_json::Error),
}

impl Display for RWIFError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            RWIFError::EdgeNotFound(edge_id) => write!(f, "edge not found: {}", edge_id),
            RWIFError::Io(e) => write!(f, "io error: {}", e),
            RWIFError::Serde(e) => write!(f, "serde error: {}", e),
        }
    }
}

impl std::error::Error for RWIFError {}

impl From<std::io::Error> for RWIFError {
    fn from(value: std::io::Error) -> Self {
        RWIFError::Io(value)
    }
}

impl From<serde_json::Error> for RWIFError {
    fn from(value: serde_json::Error) -> Self {
        RWIFError::Serde(value)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Provenance {
    pub source_type: String,
    pub source_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PhaseEvent {
    pub timestamp: DateTime<Utc>,
    pub phase: f64,
    pub sigma: f64,
    pub source: Provenance,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RWIFEdge {
    pub edge_id: String,
    pub source: String,
    pub relation: String,
    pub target: String,
    pub lobe: String,
    pub trajectory: Vec<PhaseEvent>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RWIFNode {
    pub node_id: String,
    pub label: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RWIFCrystal {
    pub id: String,
    pub nodes: HashMap<String, RWIFNode>,
    pub edges: HashMap<String, RWIFEdge>,
}

impl RWIFCrystal {
    pub fn append_trajectory(&mut self, edge_id: &str, event: PhaseEvent) -> Result<()> {
        let edge = self
            .edges
            .get_mut(edge_id)
            .ok_or_else(|| RWIFError::EdgeNotFound(edge_id.to_string()))?;
        edge.trajectory.push(event);
        Ok(())
    }

    pub fn save_to_path(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let json = serde_json::to_string_pretty(self)?;
        fs::write(path, json)?;
        Ok(())
    }

    pub fn load_from_path(path: &Path) -> Result<Self> {
        let raw = fs::read_to_string(path)?;
        let crystal = serde_json::from_str::<RWIFCrystal>(&raw)?;
        Ok(crystal)
    }
}

#[derive(Clone, Debug)]
pub struct Node {
    pub node_id: String,
    pub label: String,
}

#[derive(Clone, Debug)]
pub struct Edge {
    pub edge_id: String,
    pub source_id: String,
    pub relation: String,
    pub target_id: String,
    pub theta: f64,
    pub sigma: f64,
}

#[derive(Clone, Debug)]
pub struct Crystal {
    pub crystal_id: String,
    pub crystal_label: String,
    pub domain: String,
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
}

#[derive(Clone, Debug)]
pub struct CrystalBank {
    pub bank_id: String,
    pub bank_label: String,
    pub crystals: Vec<Crystal>,
}

impl CrystalBank {
    pub fn select_baseline_crystal<'a>(
        &'a self,
        crystal_label: Option<&str>,
        domain: Option<&str>,
    ) -> Option<&'a Crystal> {
        let mut filtered: Vec<&Crystal> = self.crystals.iter().collect();

        if let Some(label) = crystal_label {
            filtered.retain(|c| c.crystal_label == label);
        }

        if let Some(d) = domain {
            filtered.retain(|c| c.domain == d);
        }

        filtered.sort_by(|a, b| a.crystal_id.cmp(&b.crystal_id));
        filtered.into_iter().next()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn sample_crystal() -> RWIFCrystal {
        let mut nodes = HashMap::new();
        nodes.insert(
            "n1".to_string(),
            RWIFNode {
                node_id: "n1".to_string(),
                label: "light".to_string(),
            },
        );
        nodes.insert(
            "n2".to_string(),
            RWIFNode {
                node_id: "n2".to_string(),
                label: "darkness".to_string(),
            },
        );

        let mut edges = HashMap::new();
        edges.insert(
            "e1".to_string(),
            RWIFEdge {
                edge_id: "e1".to_string(),
                source: "n1".to_string(),
                relation: "dispels".to_string(),
                target: "n2".to_string(),
                lobe: "English".to_string(),
                trajectory: vec![PhaseEvent {
                    timestamp: Utc.with_ymd_and_hms(2026, 5, 18, 0, 0, 0).unwrap(),
                    phase: 0.0,
                    sigma: 0.02,
                    source: Provenance {
                        source_type: "manual".to_string(),
                        source_id: "seed".to_string(),
                    },
                }],
            },
        );

        RWIFCrystal {
            id: "c1".to_string(),
            nodes,
            edges,
        }
    }

    #[test]
    fn append_only_trajectory_is_preserved() {
        let mut crystal = sample_crystal();
        let before = crystal.edges["e1"].trajectory.len();

        crystal
            .append_trajectory(
                "e1",
                PhaseEvent {
                    timestamp: Utc.with_ymd_and_hms(2026, 5, 18, 0, 1, 0).unwrap(),
                    phase: 0.1,
                    sigma: 0.03,
                    source: Provenance {
                        source_type: "sync".to_string(),
                        source_id: "agent-a".to_string(),
                    },
                },
            )
            .unwrap();

        let after = crystal.edges["e1"].trajectory.len();
        assert_eq!(before + 1, after);
    }

    #[test]
    fn save_and_load_round_trip() {
        let mut crystal = sample_crystal();
        crystal
            .append_trajectory(
                "e1",
                PhaseEvent {
                    timestamp: Utc.with_ymd_and_hms(2026, 5, 18, 0, 2, 0).unwrap(),
                    phase: 0.2,
                    sigma: 0.04,
                    source: Provenance {
                        source_type: "feedback".to_string(),
                        source_id: "run-1".to_string(),
                    },
                },
            )
            .unwrap();

        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("rwif_core_roundtrip_{}.json", stamp));

        crystal.save_to_path(&path).unwrap();
        let loaded = RWIFCrystal::load_from_path(&path).unwrap();

        assert_eq!(loaded.id, "c1");
        assert_eq!(loaded.edges["e1"].trajectory.len(), 2);

        let _ = fs::remove_file(path);
    }
}
