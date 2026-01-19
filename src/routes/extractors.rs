use crate::error::{EdgeProxyError, Result};
use axum::http::HeaderMap;

pub fn extract_environment_key(headers: &HeaderMap) -> Result<String> {
    headers
        .get("X-Environment-Key")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .ok_or_else(|| EdgeProxyError::FlagsmithUnknownKey("".to_string()))
}
