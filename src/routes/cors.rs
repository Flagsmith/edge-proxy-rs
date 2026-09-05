use axum::http::HeaderValue;
use axum::http::header::VARY;
use axum::response::Response;
use std::time::Duration;
use tower_http::cors::{AllowHeaders, AllowMethods, AllowOrigin, CorsLayer};

// Browsers reject `*` on credentialed requests, so the origin, method and
// headers are echoed back instead, as the Python proxy does.
pub fn layer(allow_origins: &[String]) -> CorsLayer {
    let allow_origin = if allow_origins.iter().any(|origin| origin == "*") {
        AllowOrigin::mirror_request()
    } else {
        let allowed = allow_origins.to_vec();
        AllowOrigin::predicate(move |origin, _| {
            allowed
                .iter()
                .any(|allowed| allowed.as_bytes() == origin.as_bytes())
        })
    };
    CorsLayer::new()
        .allow_origin(allow_origin)
        .allow_credentials(true)
        .allow_methods(AllowMethods::mirror_request())
        .allow_headers(AllowHeaders::mirror_request())
        .max_age(Duration::from_secs(600))
}

// Each tower-http layer appends its own `Vary` line; fold them into one.
pub async fn merge_vary(mut response: Response) -> Response {
    let headers = response.headers_mut();
    if headers.get_all(VARY).iter().count() < 2 {
        return response;
    }
    let merged = headers
        .get_all(VARY)
        .iter()
        .map(HeaderValue::as_bytes)
        .collect::<Vec<_>>()
        .join(&b", "[..]);
    if let Ok(value) = HeaderValue::from_bytes(&merged) {
        headers.insert(VARY, value);
    }
    response
}
