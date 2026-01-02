use flagsmith_flag_engine::identities::Trait as FlagsmithTrait;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fmt;
use std::hash::{Hash, Hasher};
use validator::Validate;

#[derive(Debug, Clone, Serialize, Deserialize, Validate, PartialEq, Eq)]
pub struct TraitModel {
    pub trait_key: String,
    pub trait_value: serde_json::Value,
}

impl TraitModel {
    pub fn to_response_json(&self) -> serde_json::Value {
        json!({
            "trait_key": self.trait_key,
            "trait_value": self.trait_value,
        })
    }
}

impl From<&TraitModel> for FlagsmithTrait {
    fn from(t: &TraitModel) -> Self {
        let flagsmith_value = serde_json::from_value(t.trait_value.clone()).unwrap_or_default();
        FlagsmithTrait {
            trait_key: t.trait_key.clone(),
            trait_value: flagsmith_value,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct IdentityWithTraits {
    pub identifier: String,
    #[serde(default)]
    pub traits: Vec<TraitModel>,
}

impl IdentityWithTraits {
    pub fn new(identifier: String) -> Self {
        Self {
            identifier,
            traits: Vec::new(),
        }
    }

    pub fn with_traits(identifier: String, traits: Vec<TraitModel>) -> Self {
        Self { identifier, traits }
    }
}

impl fmt::Display for IdentityWithTraits {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "identifier:{}", self.identifier)?;
        if !self.traits.is_empty() {
            write!(f, "|traits:")?;
            for (i, t) in self.traits.iter().enumerate() {
                if i > 0 {
                    write!(f, ",")?;
                }
                write!(f, "{}=", t.trait_key)?;
                match &t.trait_value {
                    serde_json::Value::String(s) => write!(f, "{}", s)?,
                    serde_json::Value::Number(n) => write!(f, "{}", n)?,
                    serde_json::Value::Bool(true) => write!(f, "True")?,
                    serde_json::Value::Bool(false) => write!(f, "False")?,
                    other => write!(f, "{}", other)?,
                }
            }
        }
        Ok(())
    }
}

impl Hash for IdentityWithTraits {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.to_string().hash(state);
    }
}

impl PartialEq for IdentityWithTraits {
    fn eq(&self, other: &Self) -> bool {
        self.to_string() == other.to_string()
    }
}

impl Eq for IdentityWithTraits {}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::hash_map::DefaultHasher;

    #[test]
    fn test_identity_with_traits_str() {
        // Given
        let identifier = "foo".to_string();
        let traits = vec![
            TraitModel {
                trait_key: "foo".to_string(),
                trait_value: json!("bar"),
            },
            TraitModel {
                trait_key: "age".to_string(),
                trait_value: json!(21),
            },
            TraitModel {
                trait_key: "is_cool".to_string(),
                trait_value: json!(true),
            },
        ];

        let expected = "identifier:foo|traits:foo=bar,age=21,is_cool=True";

        // When
        let identity_with_traits = IdentityWithTraits::with_traits(identifier, traits);

        // Then
        assert_eq!(identity_with_traits.to_string(), expected);
    }

    #[test]
    fn test_identity_with_traits_hash() {
        // Given
        let identifier = "foo".to_string();
        let traits = vec![
            TraitModel {
                trait_key: "foo".to_string(),
                trait_value: json!("bar"),
            },
            TraitModel {
                trait_key: "age".to_string(),
                trait_value: json!(21),
            },
            TraitModel {
                trait_key: "is_cool".to_string(),
                trait_value: json!(true),
            },
        ];

        let expected_string = "identifier:foo|traits:foo=bar,age=21,is_cool=True";
        let mut hasher = DefaultHasher::new();
        expected_string.hash(&mut hasher);
        let expected_hash = hasher.finish();

        // When
        let identity_with_traits = IdentityWithTraits::with_traits(identifier, traits);
        let mut hasher2 = DefaultHasher::new();
        identity_with_traits.hash(&mut hasher2);
        let actual_hash = hasher2.finish();

        // Then
        assert_eq!(actual_hash, expected_hash);
    }
}
