pub mod environment_document;
pub mod extractors;
pub mod flags;
pub mod health;
pub mod identities;

use crate::config::AppSettings;
use crate::services::EnvironmentService;
use axum::{
    Router,
    routing::{get, post},
};
use std::sync::Arc;
use tower_http::{compression::CompressionLayer, cors::CorsLayer, trace::TraceLayer};

pub fn create_router(settings: AppSettings) -> (Router, Arc<EnvironmentService>) {
    let environment_service = Arc::new(EnvironmentService::new(settings));

    let app = Router::new()
        // Health check routes
        .route("/health", get(health::health_check))
        .route("/proxy/health", get(health::health_check))
        .route("/proxy/health/readiness", get(health::health_check))
        .route("/proxy/health/liveness", get(health::liveness_check))
        // Flags routes (with and without trailing slash)
        .route("/api/v1/flags", get(flags::get_flags))
        .route("/api/v1/flags/", get(flags::get_flags))
        // Identities routes (with and without trailing slash)
        .route("/api/v1/identities", get(identities::get_identities))
        .route("/api/v1/identities/", get(identities::get_identities))
        .route("/api/v1/identities", post(identities::post_identities))
        .route("/api/v1/identities/", post(identities::post_identities))
        // Environment document route
        .route(
            "/api/v1/environment-document",
            get(environment_document::get_environment_document),
        )
        // Middleware layers
        .layer(CompressionLayer::new())
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(environment_service.clone());

    (app, environment_service)
}
