mod fixtures;

use axum_test::TestServer;
use edge_proxy::config::settings::{AppSettings, EnvironmentKeyPair};
use edge_proxy::routes::create_router;
use fixtures::*;
use serde_json::json;

async fn setup_test_server() -> TestServer {
    let settings = AppSettings {
        environment_key_pairs: vec![EnvironmentKeyPair {
            server_side_key: "ser.test_key".to_string(),
            client_side_key: environment_1_api_key(),
        }],
        api_url: "http://test".to_string(),
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
async fn test_get_flags() {
    // Given
    let server = setup_test_server().await;

    // When
    let response = server
        .get("/api/v1/flags")
        .add_header("X-Environment-Key", &environment_1_api_key())
        .await;

    // Then
    response.assert_status_ok();
    let body: serde_json::Value = response.json();
    let flags = body.as_array().unwrap();
    assert_eq!(flags.len(), 2);

    let feature_1 = flags
        .iter()
        .find(|f| f["feature"]["name"] == "feature_1")
        .expect("feature_1 should be in response");
    assert_eq!(feature_1["enabled"], false);
    assert_eq!(feature_1["feature"]["id"], 1);
    assert_eq!(feature_1["feature_state_value"], "feature_1_value");

    let feature_2 = flags
        .iter()
        .find(|f| f["feature"]["name"] == "feature_2")
        .expect("feature_2 should be in response");
    assert_eq!(feature_2["enabled"], true);
    assert_eq!(feature_2["feature"]["id"], 2);
    assert_eq!(feature_2["feature_state_value"], "2.3");
}

#[tokio::test]
async fn test_get_flags_single_feature() {
    // Given
    let server = setup_test_server().await;

    // When
    let response = server
        .get("/api/v1/flags?feature=feature_1")
        .add_header("X-Environment-Key", &environment_1_api_key())
        .await;

    // Then
    response.assert_status_ok();
    let body: serde_json::Value = response.json();
    assert_eq!(body["feature"]["name"], "feature_1");
    assert_eq!(body["enabled"], false);
    assert_eq!(body["feature"]["id"], 1);
    assert_eq!(body["feature_state_value"], "feature_1_value");
}

#[tokio::test]
async fn test_get_flags_single_feature_server_key_only_returns_404() {
    // Given
    let server = setup_test_server().await;

    // When
    let response = server
        .get("/api/v1/flags?feature=feature_3")
        .add_header("X-Environment-Key", &environment_1_api_key())
        .await;

    // Then
    response.assert_status(axum::http::StatusCode::NOT_FOUND);
    let body: serde_json::Value = response.json();
    assert_eq!(body["status"], "not_found");
    assert_eq!(body["message"], "feature 'feature_3' not found");
}

#[tokio::test]
async fn test_get_flags_unknown_key() {
    // Given
    let server = setup_test_server().await;

    // When
    let response = server
        .get("/api/v1/flags")
        .add_header("X-Environment-Key", "unknown_key")
        .await;

    // Then
    response.assert_status(axum::http::StatusCode::UNAUTHORIZED);
    let body: serde_json::Value = response.json();
    assert_eq!(body["status"], "unauthorized");
    assert!(body["message"].as_str().unwrap().contains("unknown key"));
}

#[tokio::test]
async fn test_post_identity_with_traits() {
    // Given
    let server = setup_test_server().await;
    let identity_data = json!({
        "identifier": "test_identity",
        "traits": [
            {"trait_key": "email", "trait_value": "test@example.com"}
        ]
    });

    // When
    let response = server
        .post("/api/v1/identities/")
        .add_header("X-Environment-Key", &environment_1_api_key())
        .json(&identity_data)
        .await;

    // Then
    response.assert_status_ok();
    let body: serde_json::Value = response.json();
    assert!(body["flags"].is_array());
    let flags = body["flags"].as_array().unwrap();
    assert_eq!(flags.len(), 2);
    assert_eq!(body["traits"][0]["trait_key"], "email");
    assert_eq!(body["traits"][0]["trait_value"], "test@example.com");

    let feature_names: Vec<&str> = flags
        .iter()
        .map(|f| f["feature"]["name"].as_str().unwrap())
        .collect();
    assert!(feature_names.contains(&"feature_1"));
    assert!(feature_names.contains(&"feature_2"));
}

#[tokio::test]
async fn test_post_identity_with_override() {
    // Given
    let server = setup_test_server().await;
    let identity_data = json!({
        "identifier": "overridden-id",
        "traits": []
    });

    // When
    let response = server
        .post("/api/v1/identities/")
        .add_header("X-Environment-Key", &environment_1_api_key())
        .json(&identity_data)
        .await;

    // Then
    response.assert_status_ok();
    let body: serde_json::Value = response.json();
    let flags = body["flags"].as_array().unwrap();
    let feature_1 = flags
        .iter()
        .find(|f| f["feature"]["name"] == "feature_1")
        .expect("feature_1 should be in response");
    assert_eq!(feature_1["feature_state_value"], "identity_override");
    assert_eq!(feature_1["enabled"], true);
}

#[tokio::test]
async fn test_get_identities() {
    // Given
    let server = setup_test_server().await;

    // When
    let response = server
        .get("/api/v1/identities/?identifier=test_identity")
        .add_header("X-Environment-Key", &environment_1_api_key())
        .await;

    // Then
    response.assert_status_ok();
    let body: serde_json::Value = response.json();
    assert!(body["flags"].is_array());
    assert!(body["traits"].is_array());
    assert_eq!(body["traits"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn test_get_environment_document() {
    // Given
    let settings = AppSettings {
        environment_key_pairs: vec![EnvironmentKeyPair {
            server_side_key: "ser.test_key".to_string(),
            client_side_key: "client_key".to_string(),
        }],
        ..AppSettings::new()
    };
    let (app, service) = create_router(settings);
    service
        .cache
        .put_environment("client_key", environment_1())
        .await;
    let server = TestServer::new(app).unwrap();

    // When
    let response = server
        .get("/api/v1/environment-document")
        .add_header("X-Environment-Key", "ser.test_key")
        .await;

    // Then
    response.assert_status_ok();
    let body: serde_json::Value = response.json();
    assert_eq!(body["api_key"], environment_1_api_key());
}

#[tokio::test]
async fn test_get_environment_document_missing_key() {
    // Given
    let server = setup_test_server().await;

    // When
    let response = server.get("/api/v1/environment-document").await;

    // Then
    response.assert_status(axum::http::StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_get_environment_document_client_key_rejected() {
    // Given
    let server = setup_test_server().await;

    // When
    let response = server
        .get("/api/v1/environment-document")
        .add_header("X-Environment-Key", &environment_1_api_key())
        .await;

    // Then
    response.assert_status(axum::http::StatusCode::UNAUTHORIZED);
}
