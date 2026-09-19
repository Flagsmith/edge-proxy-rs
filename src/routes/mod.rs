pub mod environment_document;
pub mod extractors;
pub mod flags;
pub mod health;
pub mod identities;

use crate::config::AppSettings;
use crate::middleware::{cors, usage_tracking};
use crate::services::EnvironmentService;
use axum::{
    Router,
    middleware::{from_fn_with_state, map_response},
    routing::{get, post},
};
use std::sync::Arc;
use tower_http::{compression::CompressionLayer, normalize_path::NormalizePath, trace::TraceLayer};

pub(crate) const FLAGS_PATH: &str = "/api/v1/flags";
pub(crate) const IDENTITIES_PATH: &str = "/api/v1/identities";
pub(crate) const ENVIRONMENT_DOCUMENT_PATH: &str = "/api/v1/environment-document";

pub fn create_router(settings: AppSettings) -> (Router, Arc<EnvironmentService>) {
    let cors = cors::layer(&settings.allow_origins);
    let environment_service = Arc::new(EnvironmentService::new(settings));

    let router = Router::new()
        // Health check routes
        .route("/health", get(health::health_check))
        .route("/proxy/health", get(health::health_check))
        .route("/proxy/health/readiness", get(health::health_check))
        .route("/proxy/health/liveness", get(health::liveness_check))
        // Flags routes (with and without trailing slash)
        .route(FLAGS_PATH, get(flags::get_flags))
        // Identities routes (with and without trailing slash)
        .route(IDENTITIES_PATH, get(identities::get_identities))
        .route(IDENTITIES_PATH, post(identities::post_identities))
        // Environment document route
        .route(
            ENVIRONMENT_DOCUMENT_PATH,
            get(environment_document::get_environment_document),
        )
        // Middleware layers
        .layer(from_fn_with_state(
            environment_service.clone(),
            usage_tracking::track_usage,
        ))
        .layer(CompressionLayer::new())
        .layer(cors)
        .layer(map_response(cors::merge_vary))
        .layer(TraceLayer::new_for_http())
        .with_state(environment_service.clone());

    // Trailing-slash normalization must wrap the router itself: axum matches
    // routes before `Router::layer` middleware runs
    let app = Router::new().fallback_service(NormalizePath::trim_trailing_slash(router));

    (app, environment_service)
}
