//! Relation abstraction and registry for inference behavior.

use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RelationType {
    IsA,
    Causes,
    HasProperty,
}

impl RelationType {
    pub fn as_str(&self) -> &'static str {
        match self {
            RelationType::IsA => "is_a",
            RelationType::Causes => "causes",
            RelationType::HasProperty => "has_property",
        }
    }

    pub fn from_str(value: &str) -> Option<Self> {
        match value {
            "is_a" => Some(RelationType::IsA),
            "causes" => Some(RelationType::Causes),
            "has_property" => Some(RelationType::HasProperty),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RelationSpec {
    pub relation: RelationType,
    pub transitive: bool,
    pub max_depth: Option<usize>,
}

#[derive(Debug, Clone, Default)]
pub struct RelationDepthOverrides {
    pub is_a: Option<usize>,
    pub causes: Option<usize>,
    pub has_property: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct RelationRegistry {
    by_name: HashMap<String, RelationSpec>,
}

impl Default for RelationRegistry {
    fn default() -> Self {
        let mut by_name = HashMap::new();
        by_name.insert(
            "is_a".to_string(),
            RelationSpec {
                relation: RelationType::IsA,
                transitive: true,
                max_depth: None,
            },
        );
        by_name.insert(
            "causes".to_string(),
            RelationSpec {
                relation: RelationType::Causes,
                transitive: true,
                max_depth: Some(3),
            },
        );
        by_name.insert(
            "has_property".to_string(),
            RelationSpec {
                relation: RelationType::HasProperty,
                transitive: false,
                max_depth: Some(1),
            },
        );
        Self { by_name }
    }
}

impl RelationRegistry {
    pub fn with_depth_overrides(overrides: RelationDepthOverrides) -> Self {
        let mut registry = Self::default();
        if let Some(limit) = overrides.is_a {
            if let Some(spec) = registry.by_name.get_mut("is_a") {
                spec.max_depth = Some(limit);
            }
        }
        if let Some(limit) = overrides.causes {
            if let Some(spec) = registry.by_name.get_mut("causes") {
                spec.max_depth = Some(limit);
            }
        }
        if let Some(limit) = overrides.has_property {
            if let Some(spec) = registry.by_name.get_mut("has_property") {
                spec.max_depth = Some(limit);
            }
        }
        registry
    }

    pub fn spec_by_name(&self, name: &str) -> Option<&RelationSpec> {
        self.by_name.get(name)
    }

    pub fn spec_by_type(&self, relation: RelationType) -> Option<&RelationSpec> {
        self.by_name.values().find(|spec| spec.relation == relation)
    }
}
