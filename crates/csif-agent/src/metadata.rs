//! Startup metadata and migration checks for CSIF-Agent banks.

use crate::grammar::canonicalize_entity;
use rwif_core::{RWIFCrystal, RWIFEdge, RWIFNode};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

pub const CURRENT_SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentMetadata {
    pub schema_version: u32,
    pub grammar_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LobeState {
    pub applied: Vec<AppliedLobe>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppliedLobe {
    pub id: String,
    pub version: String,
    pub fingerprint: String,
}

pub fn metadata_path_for_bank(bank_path: &Path) -> PathBuf {
    let mut path = bank_path.to_path_buf();
    path.set_extension("meta.json");
    path
}

pub fn lobe_state_path_for_bank(bank_path: &Path) -> PathBuf {
    let mut path = bank_path.to_path_buf();
    path.set_extension("lobes.json");
    path
}

pub fn load_metadata(path: &Path) -> Result<Option<AgentMetadata>, Box<dyn Error>> {
    if !path.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(path)?;
    let meta: AgentMetadata = serde_json::from_str(&raw)?;
    Ok(Some(meta))
}

pub fn save_metadata(path: &Path, meta: &AgentMetadata) -> Result<(), Box<dyn Error>> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let raw = serde_json::to_string_pretty(meta)?;
    fs::write(path, raw)?;
    Ok(())
}

pub fn load_lobe_state(path: &Path) -> Result<LobeState, Box<dyn Error>> {
    if !path.exists() {
        return Ok(LobeState::default());
    }
    let raw = fs::read_to_string(path)?;
    let state: LobeState = serde_json::from_str(&raw)?;
    Ok(state)
}

pub fn save_lobe_state(path: &Path, state: &LobeState) -> Result<(), Box<dyn Error>> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let raw = serde_json::to_string_pretty(state)?;
    fs::write(path, raw)?;
    Ok(())
}

pub fn migrate_schema_v1_to_v2(crystal: &mut RWIFCrystal) -> bool {
    if crystal.nodes.is_empty() {
        return false;
    }

    let mut old_to_new_id: HashMap<String, String> = HashMap::new();
    let mut canonical_to_id: HashMap<String, String> = HashMap::new();
    let mut new_nodes: HashMap<String, RWIFNode> = HashMap::new();
    let mut ordered_node_ids: Vec<String> = crystal.nodes.keys().cloned().collect();
    ordered_node_ids.sort();

    for node_id in ordered_node_ids {
        let Some(node) = crystal.nodes.get(&node_id) else {
            continue;
        };
        let canonical = canonicalize_entity(&node.label).unwrap_or_else(|| node.label.clone());
        if let Some(existing_id) = canonical_to_id.get(&canonical) {
            old_to_new_id.insert(node_id.clone(), existing_id.clone());
            continue;
        }

        canonical_to_id.insert(canonical.clone(), node_id.clone());
        old_to_new_id.insert(node_id.clone(), node_id.clone());
        new_nodes.insert(
            node_id.clone(),
            RWIFNode {
                node_id: node_id.clone(),
                label: canonical,
            },
        );
    }

    let mut merged_edges = HashMap::<String, RWIFEdge>::new();
    for edge in crystal.edges.values() {
        let source = old_to_new_id
            .get(&edge.source)
            .cloned()
            .unwrap_or_else(|| edge.source.clone());
        let target = old_to_new_id
            .get(&edge.target)
            .cloned()
            .unwrap_or_else(|| edge.target.clone());

        let Some(source_label) = new_nodes.get(&source).map(|n| n.label.clone()) else {
            continue;
        };
        let Some(target_label) = new_nodes.get(&target).map(|n| n.label.clone()) else {
            continue;
        };
        let edge_id = format!(
            "e_{}_{}_{}",
            slug(&source_label),
            edge.relation,
            slug(&target_label)
        );

        if let Some(existing) = merged_edges.get_mut(&edge_id) {
            existing.trajectory.extend(edge.trajectory.clone());
        } else {
            merged_edges.insert(
                edge_id.clone(),
                RWIFEdge {
                    edge_id,
                    source,
                    relation: edge.relation.clone(),
                    target,
                    lobe: edge.lobe.clone(),
                    trajectory: edge.trajectory.clone(),
                },
            );
        }
    }

    for edge in merged_edges.values_mut() {
        edge.trajectory.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
    }

    let changed = crystal.nodes.len() != new_nodes.len()
        || crystal.edges.len() != merged_edges.len()
        || crystal
            .nodes
            .iter()
            .any(|(id, node)| new_nodes.get(id).map(|n| n.label.as_str()) != Some(node.label.as_str()));
    crystal.nodes = new_nodes;
    crystal.edges = merged_edges;
    changed
}

fn slug(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect::<String>()
}
