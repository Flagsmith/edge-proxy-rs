use crate::error::{EdgeProxyError, Result};
use crate::routes::extractors::extract_environment_key;
use crate::state::AppState;
use axum::{
    extract::State,
    http::{HeaderMap, header},
    response::IntoResponse,
};

pub async fn get_environment_document(
    State(service): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse> {
    let environment_key = extract_environment_key(&headers)?;

    // Verify it's a server key
    if !environment_key.starts_with("ser.") {
        return Err(EdgeProxyError::FlagsmithUnknownKey(environment_key));
    }

    // Get pre-serialized bytes (with endpoint caching if enabled)
    let body = service.get_environment_bytes(&environment_key).await?;

    Ok(([(header::CONTENT_TYPE, "application/json")], body.to_vec()))
}
