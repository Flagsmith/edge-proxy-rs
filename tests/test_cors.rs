mod fixtures;

use axum::http::Method;
use axum_test::TestServer;
use edge_proxy::config::settings::{AppSettings, EnvironmentKeyPair};
use edge_proxy::routes::create_router;
use fixtures::*;

const ORIGIN: &str = "https://app.example.com";

async fn server_allowing(allow_origins: &[&str]) -> TestServer {
    let settings = AppSettings {
        environment_key_pairs: vec![EnvironmentKeyPair {
            server_side_key: "ser.test_key".to_string(),
            client_side_key: environment_1_api_key(),
        }],
        allow_origins: allow_origins.iter().map(|o| o.to_string()).collect(),
        ..AppSettings::new()
    };
    let (app, service) = create_router(settings);
    service
        .cache
        .put_environment(&environment_1_api_key(), environment_1())
        .await;
    TestServer::new(app).unwrap()
}

#[tokio::test]
async fn test_default_echoes_any_origin_with_credentials() {
    // Given
    let server = server_allowing(&["*"]).await;

    // When
    let response = server
        .get("/api/v1/flags")
        .add_header("Origin", ORIGIN)
        .add_header("X-Environment-Key", &environment_1_api_key())
        .await;

    // Then
    response.assert_status_ok();
    response.assert_header("access-control-allow-origin", ORIGIN);
    response.assert_header("access-control-allow-credentials", "true");
}

#[tokio::test]
async fn test_listed_origin_is_echoed() {
    // Given
    let server = server_allowing(&["https://other.example.com", ORIGIN]).await;

    // When
    let response = server
        .get("/api/v1/flags")
        .add_header("Origin", ORIGIN)
        .add_header("X-Environment-Key", &environment_1_api_key())
        .await;

    // Then
    response.assert_header("access-control-allow-origin", ORIGIN);
    response.assert_header("access-control-allow-credentials", "true");
}

#[tokio::test]
async fn test_unlisted_origin_gets_no_cors_headers() {
    // Given
    let server = server_allowing(&["https://other.example.com"]).await;

    // When
    let response = server
        .get("/api/v1/flags")
        .add_header("Origin", ORIGIN)
        .add_header("X-Environment-Key", &environment_1_api_key())
        .await;

    // Then
    response.assert_status_ok();
    assert!(
        response
            .maybe_header("access-control-allow-origin")
            .is_none()
    );
}

#[tokio::test]
async fn test_preflight_mirrors_requested_method_and_headers() {
    // Given
    let server = server_allowing(&["*"]).await;

    // When
    let response = server
        .method(Method::OPTIONS, "/api/v1/flags")
        .add_header("Origin", ORIGIN)
        .add_header("Access-Control-Request-Method", "GET")
        .add_header("Access-Control-Request-Headers", "x-environment-key")
        .await;

    // Then
    response.assert_status_ok();
    response.assert_header("access-control-allow-origin", ORIGIN);
    response.assert_header("access-control-allow-credentials", "true");
    response.assert_header("access-control-allow-methods", "GET");
    response.assert_header("access-control-allow-headers", "x-environment-key");
    response.assert_header("access-control-max-age", "600");
}

#[tokio::test]
async fn test_vary_is_a_single_header() {
    // Given
    let server = server_allowing(&["*"]).await;

    // When
    let response = server
        .get("/api/v1/flags")
        .add_header("Origin", ORIGIN)
        .add_header("X-Environment-Key", &environment_1_api_key())
        .await;

    // Then
    let vary: Vec<&str> = response
        .iter_headers_by_name("vary")
        .map(|v| v.to_str().unwrap())
        .collect();
    assert_eq!(vary.len(), 1, "vary lines: {vary:?}");
    assert!(vary[0].contains("accept-encoding"), "vary: {vary:?}");
    assert!(vary[0].contains("origin"), "vary: {vary:?}");
}

#[tokio::test]
async fn test_request_without_origin_gets_no_cors_headers() {
    // Given
    let server = server_allowing(&["*"]).await;

    // When
    let response = server
        .get("/api/v1/flags")
        .add_header("X-Environment-Key", &environment_1_api_key())
        .await;

    // Then
    response.assert_status_ok();
    assert!(
        response
            .maybe_header("access-control-allow-origin")
            .is_none()
    );
}
