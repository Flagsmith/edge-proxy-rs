use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::json;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum EdgeProxyError {
    #[error("unknown key '{0}'")]
    FlagsmithUnknownKey(String),

    #[error("feature '{0}' not found")]
    FeatureNotFound(String),

    #[error("service unavailable: {0}")]
    ServiceUnavailable(String),

    #[error("configuration error: {0}")]
    ConfigurationError(String),

    #[error("HTTP request failed: {0}")]
    HttpError(#[from] reqwest::Error),

    #[error("JSON serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),
}

impl IntoResponse for EdgeProxyError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            EdgeProxyError::FlagsmithUnknownKey(key) => {
                (StatusCode::UNAUTHORIZED, format!("unknown key '{}'", key))
            }
            EdgeProxyError::FeatureNotFound(feature) => (
                StatusCode::NOT_FOUND,
                format!("feature '{}' not found", feature),
            ),
            EdgeProxyError::ServiceUnavailable(msg) => (StatusCode::SERVICE_UNAVAILABLE, msg),
            EdgeProxyError::ConfigurationError(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg),
            EdgeProxyError::HttpError(e) => (StatusCode::BAD_GATEWAY, e.to_string()),
            EdgeProxyError::SerializationError(e) => {
                (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
            }
        };

        let status_name = match status {
            StatusCode::UNAUTHORIZED => "unauthorized",
            StatusCode::NOT_FOUND => "not_found",
            StatusCode::SERVICE_UNAVAILABLE => "service_unavailable",
            _ => "error",
        };

        let body = Json(json!({
            "status": status_name,
            "message": message,
        }));

        (status, body).into_response()
    }
}

pub type Result<T> = std::result::Result<T, EdgeProxyError>;
