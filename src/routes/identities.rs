use crate::error::Result;
use crate::models::{IdentityResponse, IdentityWithTraits};
use crate::routes::extractors::extract_environment_key;
use crate::state::AppState;
use axum::{
    extract::{Query, State},
    http::HeaderMap,
    Json,
};
use serde::Deserialize;

#[derive(Deserialize)]
pub struct IdentitiesQuery {
    pub identifier: String,
}

pub async fn get_identities(
    State(service): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<IdentitiesQuery>,
) -> Result<Json<IdentityResponse>> {
    let environment_key = extract_environment_key(&headers)?;

    let identity = IdentityWithTraits::new(query.identifier);
    let response = service
        .get_identity_response_data(&identity, &environment_key)
        .await?;

    Ok(Json(response))
}

pub async fn post_identities(
    State(service): State<AppState>,
    headers: HeaderMap,
    Json(identity): Json<IdentityWithTraits>,
) -> Result<Json<IdentityResponse>> {
    let environment_key = extract_environment_key(&headers)?;

    let response = service
        .get_identity_response_data(&identity, &environment_key)
        .await?;

    Ok(Json(response))
}
