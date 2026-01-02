use crate::state::AppState;
use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheckResponse {
    pub status: String,
    pub reason: Option<String>,
    pub last_successful_update: Option<chrono::DateTime<Utc>>,
}

impl HealthCheckResponse {
    pub fn ok(last_successful_update: Option<chrono::DateTime<Utc>>) -> Self {
        Self {
            status: "ok".to_string(),
            reason: None,
            last_successful_update,
        }
    }

    pub fn error(reason: String, last_successful_update: Option<chrono::DateTime<Utc>>) -> Self {
        Self {
            status: "error".to_string(),
            reason: Some(reason),
            last_successful_update,
        }
    }
}

pub async fn health_check(State(service): State<AppState>) -> impl IntoResponse {
    let last_updated = service.last_updated_at.read().await;

    match *last_updated {
        None => {
            let response = HealthCheckResponse::error(
                "environment document(s) not updated.".to_string(),
                None,
            );
            (StatusCode::SERVICE_UNAVAILABLE, Json(response))
        }
        Some(last_update_time) => {
            // Check if stale based on grace period
            if let Some(grace_period) = service.settings.health_check.environment_update_grace_period_seconds {
                let now = Utc::now();
                let elapsed = now.signed_duration_since(last_update_time);

                if elapsed.num_seconds() > grace_period as i64 {
                    let response = HealthCheckResponse::error(
                        "environment document(s) stale.".to_string(),
                        Some(last_update_time),
                    );
                    return (StatusCode::SERVICE_UNAVAILABLE, Json(response));
                }
            }

            let response = HealthCheckResponse::ok(Some(last_update_time));
            (StatusCode::OK, Json(response))
        }
    }
}

pub async fn liveness_check() -> impl IntoResponse {
    StatusCode::OK
}
