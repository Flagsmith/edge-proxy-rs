use crate::error::Result;
use crate::models::APIFeatureState;
use crate::routes::extractors::extract_environment_key;
use crate::state::AppState;
use axum::{
    Json,
    extract::{Query, State},
    http::HeaderMap,
    response::IntoResponse,
};
use serde::Deserialize;

#[derive(Deserialize)]
pub struct FlagsQuery {
    pub feature: Option<String>,
}

pub enum FlagsResponse {
    Single(APIFeatureState),
    Multiple(Vec<APIFeatureState>),
}

impl IntoResponse for FlagsResponse {
    fn into_response(self) -> axum::response::Response {
        match self {
            FlagsResponse::Single(f) => Json(f).into_response(),
            FlagsResponse::Multiple(f) => Json(f).into_response(),
        }
    }
}

pub async fn get_flags(
    State(service): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<FlagsQuery>,
) -> Result<FlagsResponse> {
    let environment_key = extract_environment_key(&headers)?;

    let mut flags = service
        .get_flags_response_data(&environment_key, query.feature.as_deref())
        .await?;

    if query.feature.is_some() && flags.len() == 1 {
        Ok(FlagsResponse::Single(flags.swap_remove(0)))
    } else {
        Ok(FlagsResponse::Multiple(flags))
    }
}
