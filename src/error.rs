use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::json;
use thiserror::Error;
use validator::{ValidationErrors, ValidationErrorsKind};

#[derive(Error, Debug)]
pub enum EdgeProxyError {
    #[error("validation error: {0}")]
    Validation(String),

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
            EdgeProxyError::Validation(msg) => (StatusCode::BAD_REQUEST, msg),
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
            StatusCode::BAD_REQUEST => "bad_request",
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

impl From<ValidationErrors> for EdgeProxyError {
    fn from(errors: ValidationErrors) -> Self {
        let mut messages = flatten_validation_errors(&errors);
        messages.sort();
        EdgeProxyError::Validation(messages.join("; "))
    }
}

fn flatten_validation_errors(errors: &ValidationErrors) -> Vec<String> {
    let mut messages = Vec::new();
    for (field, kind) in errors.errors() {
        match kind {
            ValidationErrorsKind::Field(field_errors) => {
                for error in field_errors {
                    let message = error.message.as_deref().unwrap_or(&error.code);
                    messages.push(format!("{field}: {message}"));
                }
            }
            ValidationErrorsKind::Struct(nested) => {
                for message in flatten_validation_errors(nested) {
                    messages.push(format!("{field}.{message}"));
                }
            }
            ValidationErrorsKind::List(items) => {
                for (index, nested) in items {
                    for message in flatten_validation_errors(nested) {
                        messages.push(format!("{field}[{index}].{message}"));
                    }
                }
            }
        }
    }
    messages
}

pub type Result<T> = std::result::Result<T, EdgeProxyError>;
