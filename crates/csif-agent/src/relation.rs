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
            },
        );
        by_name.insert(
            "causes".to_string(),
            RelationSpec {
                relation: RelationType::Causes,
                transitive: true,
            },
        );
        by_name.insert(
            "has_property".to_string(),
            RelationSpec {
                relation: RelationType::HasProperty,
                transitive: false,
            },
        );
        Self { by_name }
    }
}

impl RelationRegistry {
    pub fn spec_by_name(&self, name: &str) -> Option<&RelationSpec> {
        self.by_name.get(name)
    }

    pub fn spec_by_type(&self, relation: RelationType) -> Option<&RelationSpec> {
        self.by_name.values().find(|spec| spec.relation == relation)
    }
}
