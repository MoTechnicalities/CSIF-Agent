//! Data-driven grammar configuration for CSIF-Agent.

use regex::Regex;
use serde::Deserialize;
use std::error::Error;
use std::fs;
use std::path::Path;

const DEFAULT_QUERY_WHAT_IS: &str = r"^what is (?:(?:a|an)\s+)?(.+?)\?$";
const DEFAULT_QUERY_IS_A_CONFIRM: &str = r"^is (?:(?:a|an)\s+)?(.+?) (?:a|an) (.+?)\?$";
const DEFAULT_QUERY_CAUSES_CONFIRM: &str = r"^does (?:(?:a|an)\s+)?(.+?) cause (.+?)\?$";
const DEFAULT_QUERY_HAS_PROPERTY_CONFIRM: &str = r"^does (?:(?:a|an)\s+)?(.+?) have (.+?)\?$";
const DEFAULT_QUERY_ADD_COMPUTE: &str = r"^what is\s+(-?\d+(?:\.\d+)?)\s*\+\s*(-?\d+(?:\.\d+)?)\?$";
const DEFAULT_TEACH_IS_A: &str = r"^(?:a|an) (.+?) is (?:a|an) (.+)$";
const DEFAULT_TEACH_CAUSES: &str = r"^(.+?) causes (.+)$";
const DEFAULT_TEACH_HAS_PROPERTY: &str = r"^(?:a|an) (.+?) has (.+)$";
const DEFAULT_DESCRIBE_CLASSIFICATION: &str = "A {subject} is {direct}.";
const DEFAULT_DESCRIBE_PROPERTIES_INTRO: &str = "It can be";
const DEFAULT_DESCRIBE_PROPERTIES_OUTRO: &str = ".";
const DEFAULT_DESCRIBE_PROPERTY_CONNECTOR: &str = "and";
const DEFAULT_DESCRIBE_SUBTYPES_INTRO: &str = "There are several types, including";
const DEFAULT_DESCRIBE_SUBTYPES_OUTRO: &str = ".";
const DEFAULT_DESCRIBE_SUBTYPE_CONNECTOR: &str = "and";
const DEFAULT_DESCRIBE_OXFORD_COMMA: bool = true;
const DEFAULT_DESCRIBE_MAX_SUBTYPE_EXAMPLES: usize = 5;

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

#[derive(Debug, Clone)]
pub struct DescribeTemplates {
    pub classification: String,
    pub properties_intro: String,
    pub properties_outro: String,
    pub property_connector: String,
    pub subtypes_intro: String,
    pub subtypes_outro: String,
    pub subtype_connector: String,
    pub oxford_comma: bool,
    pub max_subtype_examples: usize,
}

impl Default for DescribeTemplates {
    fn default() -> Self {
        Self {
            classification: DEFAULT_DESCRIBE_CLASSIFICATION.to_string(),
            properties_intro: DEFAULT_DESCRIBE_PROPERTIES_INTRO.to_string(),
            properties_outro: DEFAULT_DESCRIBE_PROPERTIES_OUTRO.to_string(),
            property_connector: DEFAULT_DESCRIBE_PROPERTY_CONNECTOR.to_string(),
            subtypes_intro: DEFAULT_DESCRIBE_SUBTYPES_INTRO.to_string(),
            subtypes_outro: DEFAULT_DESCRIBE_SUBTYPES_OUTRO.to_string(),
            subtype_connector: DEFAULT_DESCRIBE_SUBTYPE_CONNECTOR.to_string(),
            oxford_comma: DEFAULT_DESCRIBE_OXFORD_COMMA,
            max_subtype_examples: DEFAULT_DESCRIBE_MAX_SUBTYPE_EXAMPLES,
        }
    }
}

impl DescribeTemplates {
    fn from_overrides(overrides: Option<DescribeTemplateRules>) -> Self {
        let defaults = Self::default();
        let Some(overrides) = overrides else {
            return defaults;
        };

        Self {
            classification: overrides.classification.unwrap_or(defaults.classification),
            properties_intro: overrides
                .properties_intro
                .unwrap_or(defaults.properties_intro),
            properties_outro: overrides
                .properties_outro
                .unwrap_or(defaults.properties_outro),
            property_connector: overrides
                .property_connector
                .unwrap_or(defaults.property_connector),
            subtypes_intro: overrides.subtypes_intro.unwrap_or(defaults.subtypes_intro),
            subtypes_outro: overrides.subtypes_outro.unwrap_or(defaults.subtypes_outro),
            subtype_connector: overrides
                .subtype_connector
                .unwrap_or(defaults.subtype_connector),
            oxford_comma: overrides.oxford_comma.unwrap_or(defaults.oxford_comma),
            max_subtype_examples: overrides
                .max_subtype_examples
                .unwrap_or(defaults.max_subtype_examples),
        }
    }
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
    describe_templates: DescribeTemplates,
}

#[derive(Debug, Deserialize)]
struct GrammarFile {
    version: Option<String>,
    query: QueryRules,
    teach: TeachRules,
    templates: Option<TemplateRules>,
}

#[derive(Debug, Deserialize)]
struct TemplateRules {
    describe: Option<DescribeTemplateRules>,
}

#[derive(Debug, Deserialize)]
struct DescribeTemplateRules {
    classification: Option<String>,
    properties_intro: Option<String>,
    properties_outro: Option<String>,
    property_connector: Option<String>,
    subtypes_intro: Option<String>,
    subtypes_outro: Option<String>,
    subtype_connector: Option<String>,
    oxford_comma: Option<bool>,
    max_subtype_examples: Option<usize>,
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
            describe_templates: DescribeTemplates::default(),
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
            describe_templates: DescribeTemplates::from_overrides(
                grammar_file.templates.and_then(|templates| templates.describe),
            ),
        })
    }

    pub fn version(&self) -> &str {
        &self.version
    }

    pub fn describe_templates(&self) -> &DescribeTemplates {
        &self.describe_templates
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
