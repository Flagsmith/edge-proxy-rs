use flagsmith_flag_engine::engine_eval::FlagResult;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::borrow::Cow;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct APIFeature {
    pub id: i64,
    pub name: String,
    #[serde(rename = "type")]
    pub feature_type: Cow<'static, str>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct APIFeatureState {
    pub enabled: bool,
    pub feature: APIFeature,
    pub feature_state_value: Value,
}

impl From<&FlagResult> for APIFeatureState {
    fn from(flag_result: &FlagResult) -> Self {
        let json_value = serde_json::to_value(&flag_result.value).unwrap_or(Value::Null);

        APIFeatureState {
            enabled: flag_result.enabled,
            feature: APIFeature {
                id: flag_result.metadata.feature_id as i64,
                name: flag_result.name.clone(),
                feature_type: Cow::Owned(flag_result.metadata.feature_type.clone()),
            },
            feature_state_value: json_value,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct IdentityResponse {
    pub flags: Vec<APIFeatureState>,
    pub traits: Vec<serde_json::Value>,
}
