use crate::error::Result;
use crate::routes::extractors::extract_environment_key;
use crate::state::AppState;
use axum::{
    Json,
    extract::{Query, State},
    http::HeaderMap,
};
use serde::Deserialize;

#[derive(Deserialize)]
pub struct FlagsQuery {
    pub feature: Option<String>,
}

pub async fn get_flags(
    State(service): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<FlagsQuery>,
) -> Result<Json<serde_json::Value>> {
    let environment_key = extract_environment_key(&headers)?;

    let flags = service
        .get_flags_response_data(&environment_key, query.feature.as_deref())
        .await?;

    if query.feature.is_some() && flags.len() == 1 {
        // Return single feature
        Ok(Json(serde_json::to_value(&flags[0])?))
    } else {
        // Return all features
        Ok(Json(serde_json::to_value(flags)?))
    }
}
