use flagsmith_flag_engine::identities::Trait as FlagsmithTrait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashSet;
use std::fmt;
use std::hash::{Hash, Hasher};
use validator::{Validate, ValidationError};

pub const TRAIT_KEY_MAX_LENGTH: u64 = 200;
pub const TRAIT_VALUE_MAX_LENGTH: u64 = 2000;
pub const IDENTIFIER_MAX_LENGTH: u64 = 2000;

#[derive(Debug, Clone, Serialize, Deserialize, Validate, PartialEq, Eq)]
pub struct TraitModel {
    #[validate(length(max = TRAIT_KEY_MAX_LENGTH, message = "must be at most 200 characters"))]
    pub trait_key: String,
    #[validate(custom(function = "validate_trait_value"))]
    pub trait_value: Value,
    #[serde(default, skip_serializing)]
    pub transient: bool,
}

// Core stores strings, lists and objects in a varchar(2000) column.
fn validate_trait_value(value: &Value) -> Result<(), ValidationError> {
    let length = match value {
        Value::String(s) => s.chars().count(),
        Value::Array(_) | Value::Object(_) => value.to_string().chars().count(),
        _ => return Ok(()),
    };
    if length as u64 > TRAIT_VALUE_MAX_LENGTH {
        return Err(
            ValidationError::new("length").with_message("must be at most 2000 characters".into())
        );
    }
    Ok(())
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
    #[validate(length(max = IDENTIFIER_MAX_LENGTH, message = "must be at most 2000 characters"))]
    pub identifier: String,
    #[serde(default)]
    #[validate(nested)]
    pub traits: Vec<TraitModel>,
    #[serde(default, skip_serializing)]
    pub transient: bool,
}

impl IdentityWithTraits {
    pub fn new(identifier: String) -> Self {
        Self {
            identifier,
            traits: Vec::new(),
            transient: false,
        }
    }

    pub fn with_traits(identifier: String, traits: Vec<TraitModel>) -> Self {
        Self {
            identifier,
            traits,
            transient: false,
        }
    }

    pub fn with_unique_traits(mut self) -> Self {
        let mut seen = HashSet::new();
        self.traits.reverse();
        self.traits.retain(|t| seen.insert(t.trait_key.clone()));
        self.traits.reverse();
        self
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
    use rstest::rstest;
    use serde_json::json;
    use std::collections::hash_map::DefaultHasher;

    fn identity(identifier: &str, trait_key: &str, trait_value: Value) -> IdentityWithTraits {
        IdentityWithTraits::with_traits(
            identifier.to_string(),
            vec![TraitModel {
                trait_key: trait_key.to_string(),
                trait_value,
                transient: false,
            }],
        )
    }

    #[rstest]
    #[case::identifier_at_limit(identity(&"i".repeat(2000), "k", json!("v")))]
    #[case::trait_key_at_limit(identity("id", &"k".repeat(200), json!("v")))]
    #[case::string_value_at_limit(identity("id", "k", json!("v".repeat(2000))))]
    #[case::number_value(identity("id", "k", json!(1)))]
    #[case::bool_value(identity("id", "k", json!(true)))]
    #[case::null_value(identity("id", "k", Value::Null))]
    fn test_validate_within_limits_is_ok(#[case] identity: IdentityWithTraits) {
        assert!(identity.validate().is_ok());
    }

    #[rstest]
    #[case::identifier_too_long(
        identity(&"i".repeat(2001), "k", json!("v")),
        "identifier: must be at most 2000 characters"
    )]
    #[case::trait_key_too_long(
        identity("id", &"k".repeat(201), json!("v")),
        "traits[0].trait_key: must be at most 200 characters"
    )]
    #[case::string_value_too_long(
        identity("id", "k", json!("v".repeat(2001))),
        "traits[0].trait_value: must be at most 2000 characters"
    )]
    #[case::list_value_too_long(
        identity("id", "k", json!(["v".repeat(2001)])),
        "traits[0].trait_value: must be at most 2000 characters"
    )]
    #[case::object_value_too_long(
        identity("id", "k", json!({"v": "v".repeat(2001)})),
        "traits[0].trait_value: must be at most 2000 characters"
    )]
    fn test_validate_over_limit_reports_the_field(
        #[case] identity: IdentityWithTraits,
        #[case] expected_message: &str,
    ) {
        // When
        let error = crate::error::EdgeProxyError::from(identity.validate().unwrap_err());

        // Then
        assert_eq!(
            error.to_string(),
            format!("validation error: {expected_message}")
        );
    }

    #[test]
    fn test_with_unique_traits_duplicate_keys_keeps_last_value() {
        // Given
        let identity = IdentityWithTraits::with_traits(
            "id".to_string(),
            vec![
                TraitModel {
                    trait_key: "a".to_string(),
                    trait_value: json!(1),
                    transient: false,
                },
                TraitModel {
                    trait_key: "b".to_string(),
                    trait_value: json!(1),
                    transient: false,
                },
                TraitModel {
                    trait_key: "a".to_string(),
                    trait_value: json!(2),
                    transient: false,
                },
            ],
        );

        // When
        let unique = identity.with_unique_traits();

        // Then
        assert_eq!(
            unique.traits,
            vec![
                TraitModel {
                    trait_key: "b".to_string(),
                    trait_value: json!(1),
                    transient: false,
                },
                TraitModel {
                    trait_key: "a".to_string(),
                    trait_value: json!(2),
                    transient: false,
                },
            ]
        );
    }

    #[test]
    fn test_identity_with_traits_str() {
        // Given
        let identifier = "foo".to_string();
        let traits = vec![
            TraitModel {
                trait_key: "foo".to_string(),
                trait_value: json!("bar"),
                transient: false,
            },
            TraitModel {
                trait_key: "age".to_string(),
                trait_value: json!(21),
                transient: false,
            },
            TraitModel {
                trait_key: "is_cool".to_string(),
                trait_value: json!(true),
                transient: false,
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
                transient: false,
            },
            TraitModel {
                trait_key: "age".to_string(),
                trait_value: json!(21),
                transient: false,
            },
            TraitModel {
                trait_key: "is_cool".to_string(),
                trait_value: json!(true),
                transient: false,
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

    #[rstest]
    #[case::omitted(json!({"identifier": "id", "traits": [{"trait_key": "k", "trait_value": "v"}]}), false, false)]
    #[case::set(
        json!({"identifier": "id", "transient": true, "traits": [{"trait_key": "k", "trait_value": "v", "transient": true}]}),
        true,
        true
    )]
    fn test_deserialize_transient(
        #[case] payload: Value,
        #[case] expected_identity_transient: bool,
        #[case] expected_trait_transient: bool,
    ) {
        // When
        let identity: IdentityWithTraits = serde_json::from_value(payload).unwrap();

        // Then
        assert_eq!(identity.transient, expected_identity_transient);
        assert_eq!(identity.traits[0].transient, expected_trait_transient);
    }

    #[test]
    fn test_transient_excluded_from_identity_key_and_serialization() {
        // Given
        let mut transient_identity = identity("id", "k", json!("v"));
        transient_identity.transient = true;
        transient_identity.traits[0].transient = true;

        // When
        let serialized = serde_json::to_value(&transient_identity).unwrap();

        // Then
        assert_eq!(transient_identity, identity("id", "k", json!("v")));
        assert_eq!(
            serialized,
            json!({"identifier": "id", "traits": [{"trait_key": "k", "trait_value": "v"}]})
        );
    }
}
