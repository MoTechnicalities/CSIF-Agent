//! Data-driven grammar configuration for CSIF-Agent.

use regex::Regex;
use serde::Deserialize;
use std::error::Error;
use std::fs;
use std::path::Path;

const DEFAULT_QUERY_WHAT_IS: &str = r"^what is (?:a|an)?\s*(.+?)\?$";
const DEFAULT_QUERY_IS_A_CONFIRM: &str = r"^is (?:a|an )?(.+?) (?:a|an) (.+?)\?$";
const DEFAULT_QUERY_CAUSES_CONFIRM: &str = r"^does (?:a|an )?(.+?) cause (.+?)\?$";
const DEFAULT_QUERY_HAS_PROPERTY_CONFIRM: &str = r"^does (?:a|an )?(.+?) have (.+?)\?$";
const DEFAULT_QUERY_ADD_COMPUTE: &str = r"^what is\s+(-?\d+(?:\.\d+)?)\s*\+\s*(-?\d+(?:\.\d+)?)\?$";
const DEFAULT_TEACH_IS_A: &str = r"^(?:a|an) (.+?) is (?:a|an) (.+)$";
const DEFAULT_TEACH_CAUSES: &str = r"^(.+?) causes (.+)$";
const DEFAULT_TEACH_HAS_PROPERTY: &str = r"^(?:a|an) (.+?) has (.+)$";

#[derive(Debug, Clone)]
pub struct TeachFact {
    pub relation: String,
    pub subject: String,
    pub object: String,
}

#[derive(Debug, Clone)]
pub enum QueryIntent {
    Describe { subject: String },
    ConfirmRelation {
        relation: String,
        subject: String,
        object: String,
    },
    ComputeAdd { left: f64, right: f64 },
}

#[derive(Debug)]
pub struct Grammar {
    version: String,
    query_what_is: Regex,
    query_is_a_confirm: Regex,
    query_causes_confirm: Regex,
    query_has_property_confirm: Regex,
    query_add_compute: Regex,
    teach_is_a: Regex,
    teach_causes: Regex,
    teach_has_property: Regex,
}

#[derive(Debug, Deserialize)]
struct GrammarFile {
    version: Option<String>,
    query: QueryRules,
    teach: TeachRules,
}

#[derive(Debug, Deserialize)]
struct QueryRules {
    what_is: String,
    is_a_confirm: String,
    causes_confirm: String,
    has_property_confirm: String,
    add_compute: String,
}

#[derive(Debug, Deserialize)]
struct TeachRules {
    is_a: String,
    causes: String,
    has_property: String,
}

impl Default for Grammar {
    fn default() -> Self {
        Self {
            version: "v1".to_string(),
            query_what_is: Regex::new(DEFAULT_QUERY_WHAT_IS).expect("default what_is regex is valid"),
            query_is_a_confirm: Regex::new(DEFAULT_QUERY_IS_A_CONFIRM)
                .expect("default is_a_confirm regex is valid"),
            query_causes_confirm: Regex::new(DEFAULT_QUERY_CAUSES_CONFIRM)
                .expect("default causes_confirm regex is valid"),
            query_has_property_confirm: Regex::new(DEFAULT_QUERY_HAS_PROPERTY_CONFIRM)
                .expect("default has_property_confirm regex is valid"),
            query_add_compute: Regex::new(DEFAULT_QUERY_ADD_COMPUTE)
                .expect("default add_compute regex is valid"),
            teach_is_a: Regex::new(DEFAULT_TEACH_IS_A).expect("default teach is_a regex is valid"),
            teach_causes: Regex::new(DEFAULT_TEACH_CAUSES)
                .expect("default teach causes regex is valid"),
            teach_has_property: Regex::new(DEFAULT_TEACH_HAS_PROPERTY)
                .expect("default teach has_property regex is valid"),
        }
    }
}

impl Grammar {
    pub fn load_from_path(path: &Path) -> Result<Self, Box<dyn Error>> {
        let contents = fs::read_to_string(path)?;
        let grammar_file: GrammarFile = toml::from_str(&contents)?;

        Ok(Self {
            version: grammar_file.version.unwrap_or_else(|| "v1".to_string()),
            query_what_is: Regex::new(&grammar_file.query.what_is)?,
            query_is_a_confirm: Regex::new(&grammar_file.query.is_a_confirm)?,
            query_causes_confirm: Regex::new(&grammar_file.query.causes_confirm)?,
            query_has_property_confirm: Regex::new(&grammar_file.query.has_property_confirm)?,
            query_add_compute: Regex::new(&grammar_file.query.add_compute)?,
            teach_is_a: Regex::new(&grammar_file.teach.is_a)?,
            teach_causes: Regex::new(&grammar_file.teach.causes)?,
            teach_has_property: Regex::new(&grammar_file.teach.has_property)?,
        })
    }

    pub fn version(&self) -> &str {
        &self.version
    }

    pub fn parse_query(&self, input: &str) -> Option<QueryIntent> {
        let normalized = normalize_query(input);

        if let Some(captures) = self.query_add_compute.captures(&normalized) {
            let left: f64 = captures.get(1)?.as_str().parse().ok()?;
            let right: f64 = captures.get(2)?.as_str().parse().ok()?;
            return Some(QueryIntent::ComputeAdd { left, right });
        }

        if let Some(captures) = self.query_is_a_confirm.captures(&normalized) {
            let subject = canonicalize_entity(captures.get(1)?.as_str())?;
            let object = canonicalize_entity(captures.get(2)?.as_str())?;
            return Some(QueryIntent::ConfirmRelation {
                relation: "is_a".to_string(),
                subject,
                object,
            });
        }

        if let Some(captures) = self.query_causes_confirm.captures(&normalized) {
            let subject = canonicalize_entity(captures.get(1)?.as_str())?;
            let object = canonicalize_entity(captures.get(2)?.as_str())?;
            return Some(QueryIntent::ConfirmRelation {
                relation: "causes".to_string(),
                subject,
                object,
            });
        }

        if let Some(captures) = self.query_has_property_confirm.captures(&normalized) {
            let subject = canonicalize_entity(captures.get(1)?.as_str())?;
            let object = canonicalize_entity(captures.get(2)?.as_str())?;
            return Some(QueryIntent::ConfirmRelation {
                relation: "has_property".to_string(),
                subject,
                object,
            });
        }

        let captures = self.query_what_is.captures(&normalized)?;
        let subject = captures.get(1)?.as_str().trim();
        canonicalize_entity(subject).map(|subject| QueryIntent::Describe { subject })
    }

    pub fn parse_teach(&self, input: &str) -> Option<TeachFact> {
        let normalized = normalize_teach(input);

        if let Some(captures) = self.teach_is_a.captures(&normalized) {
            return Some(TeachFact {
                relation: "is_a".to_string(),
                subject: canonicalize_entity(captures.get(1)?.as_str())?,
                object: canonicalize_entity(captures.get(2)?.as_str())?,
            });
        }

        if let Some(captures) = self.teach_causes.captures(&normalized) {
            return Some(TeachFact {
                relation: "causes".to_string(),
                subject: canonicalize_entity(captures.get(1)?.as_str())?,
                object: canonicalize_entity(captures.get(2)?.as_str())?,
            });
        }

        if let Some(captures) = self.teach_has_property.captures(&normalized) {
            return Some(TeachFact {
                relation: "has_property".to_string(),
                subject: canonicalize_entity(captures.get(1)?.as_str())?,
                object: canonicalize_entity(captures.get(2)?.as_str())?,
            });
        }

        None
    }
}

fn normalize_query(text: &str) -> String {
    text.trim().to_lowercase()
}

fn normalize_teach(text: &str) -> String {
    text.trim().trim_end_matches('.').trim().to_lowercase()
}

pub fn canonicalize_entity(text: &str) -> Option<String> {
    let mut value = text.trim().to_lowercase();
    if let Some(rest) = value.strip_prefix("a ") {
        value = rest.trim().to_string();
    } else if let Some(rest) = value.strip_prefix("an ") {
        value = rest.trim().to_string();
    }

    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}
