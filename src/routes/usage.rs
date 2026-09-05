use super::{ENVIRONMENT_DOCUMENT_PATH, FLAGS_PATH, IDENTITIES_PATH};
use crate::services::EnvironmentService;
use crate::usage::Resource;
use axum::extract::{MatchedPath, Request, State};
use axum::middleware::Next;
use axum::response::Response;
use std::sync::Arc;

/// Count a request once the handler has answered: only a 2xx is usage.
pub async fn track_usage(
    State(service): State<Arc<EnvironmentService>>,
    request: Request,
    next: Next,
) -> Response {
    let resource = request
        .extensions()
        .get::<MatchedPath>()
        .and_then(|path| resource_for(path.as_str()));
    let environment_key = request
        .headers()
        .get("X-Environment-Key")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);

    let response = next.run(request).await;

    if !response.status().is_success() {
        return response;
    }
    if let (Some(resource), Some(environment_key)) = (resource, environment_key) {
        service.track_usage(&environment_key, resource);
    }
    response
}

fn resource_for(route: &str) -> Option<Resource> {
    match route {
        FLAGS_PATH => Some(Resource::Flags),
        IDENTITIES_PATH => Some(Resource::Identities),
        ENVIRONMENT_DOCUMENT_PATH => Some(Resource::EnvironmentDocument),
        _ => None,
    }
}
