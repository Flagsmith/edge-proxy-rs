mod fixtures;

use axum_test::TestServer;
use edge_proxy::routes::create_router;
use edge_proxy::config::settings::{AppSettings, EnvironmentKeyPair};
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

    // Pre-populate cache with test data
    service
        .cache
        .put_environment(&environment_1_api_key(), environment_1())
        .await;

    TestServer::new(app).unwrap()
}

#[tokio::test]
async fn test_get_flags() {
    let server = setup_test_server().await;

    let response = server
        .get("/api/v1/flags")
        .add_header("X-Environment-Key", &environment_1_api_key())
        .await;

    response.assert_status_ok();

    let body: serde_json::Value = response.json();
    let flags = body.as_array().unwrap();

    // Should have 2 flags (feature_3 is server-key-only and filtered out)
    assert_eq!(flags.len(), 2);

    // Verify feature_1 is present and correct
    let feature_1 = flags
        .iter()
        .find(|f| f["feature"]["name"] == "feature_1")
        .expect("feature_1 should be in response");
    assert_eq!(feature_1["enabled"], false);
    assert_eq!(feature_1["feature"]["id"], 1);
    assert_eq!(feature_1["feature_state_value"], "feature_1_value");

    // Verify feature_2 is present and correct
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
    let server = setup_test_server().await;

    let response = server
        .get("/api/v1/flags?feature=feature_1")
        .add_header("X-Environment-Key", &environment_1_api_key())
        .await;

    response.assert_status_ok();

    // Single feature is returned as an object, not an array
    let body: serde_json::Value = response.json();
    assert_eq!(body["feature"]["name"], "feature_1");
    assert_eq!(body["enabled"], false);
    assert_eq!(body["feature"]["id"], 1);
    assert_eq!(body["feature_state_value"], "feature_1_value");
}

#[tokio::test]
async fn test_get_flags_single_feature_server_key_only_returns_404() {
    let server = setup_test_server().await;

    let response = server
        .get("/api/v1/flags?feature=feature_3")
        .add_header("X-Environment-Key", &environment_1_api_key())
        .await;

    response.assert_status(axum::http::StatusCode::NOT_FOUND);

    let body: serde_json::Value = response.json();
    assert_eq!(body["status"], "not_found");
    assert_eq!(body["message"], "feature 'feature_3' not found");
}

#[tokio::test]
async fn test_get_flags_unknown_key() {
    let server = setup_test_server().await;

    let response = server
        .get("/api/v1/flags")
        .add_header("X-Environment-Key", "unknown_key")
        .await;

    response.assert_status(axum::http::StatusCode::UNAUTHORIZED);

    let body: serde_json::Value = response.json();
    assert_eq!(body["status"], "unauthorized");
    assert!(body["message"].as_str().unwrap().contains("unknown key"));
}

#[tokio::test]
async fn test_post_identity_with_traits() {
    let server = setup_test_server().await;

    let identity_data = json!({
        "identifier": "test_identity",
        "traits": [
            {"trait_key": "email", "trait_value": "test@example.com"}
        ]
    });

    let response = server
        .post("/api/v1/identities/")
        .add_header("X-Environment-Key", &environment_1_api_key())
        .json(&identity_data)
        .await;

    response.assert_status_ok();

    let body: serde_json::Value = response.json();

    // Should have flags array
    assert!(body["flags"].is_array());
    let flags = body["flags"].as_array().unwrap();
    assert_eq!(flags.len(), 2); // Should not include feature_3 (server-key-only)

    // Should have traits array echoed back
    assert_eq!(body["traits"][0]["trait_key"], "email");
    assert_eq!(body["traits"][0]["trait_value"], "test@example.com");

    // Verify both features are present
    let feature_names: Vec<&str> = flags
        .iter()
        .map(|f| f["feature"]["name"].as_str().unwrap())
        .collect();
    assert!(feature_names.contains(&"feature_1"));
    assert!(feature_names.contains(&"feature_2"));
}

#[tokio::test]
async fn test_post_identity_with_override() {
    let server = setup_test_server().await;

    let identity_data = json!({
        "identifier": "overridden-id",
        "traits": []
    });

    let response = server
        .post("/api/v1/identities/")
        .add_header("X-Environment-Key", &environment_1_api_key())
        .json(&identity_data)
        .await;

    response.assert_status_ok();

    let body: serde_json::Value = response.json();
    let flags = body["flags"].as_array().unwrap();

    // Check that identity override was applied
    let feature_1 = flags
        .iter()
        .find(|f| f["feature"]["name"] == "feature_1")
        .expect("feature_1 should be in response");
    assert_eq!(feature_1["feature_state_value"], "identity_override");
    assert_eq!(feature_1["enabled"], true);
}

#[tokio::test]
async fn test_get_identities() {
    let server = setup_test_server().await;

    let response = server
        .get("/api/v1/identities/?identifier=test_identity")
        .add_header("X-Environment-Key", &environment_1_api_key())
        .await;

    response.assert_status_ok();

    let body: serde_json::Value = response.json();

    assert!(body["flags"].is_array());
    assert!(body["traits"].is_array());
    assert_eq!(body["traits"].as_array().unwrap().len(), 0); // No traits in GET
}

#[tokio::test]
async fn test_get_environment_document() {
    let settings = AppSettings {
        environment_key_pairs: vec![EnvironmentKeyPair {
            server_side_key: "ser.test_key".to_string(),
            client_side_key: "client_key".to_string(),
        }],
        ..AppSettings::new()
    };

    let (app, service) = create_router(settings);
    // Cache stores by client key, not server key
    service
        .cache
        .put_environment("client_key", environment_1())
        .await;

    let server = TestServer::new(app).unwrap();

    let response = server
        .get("/api/v1/environment-document")
        .add_header("X-Environment-Key", "ser.test_key")
        .await;

    response.assert_status_ok();

    let body: serde_json::Value = response.json();
    assert_eq!(body["api_key"], environment_1_api_key());
}

#[tokio::test]
async fn test_get_environment_document_missing_key() {
    let server = setup_test_server().await;

    let response = server
        .get("/api/v1/environment-document")
        .await;

    response.assert_status(axum::http::StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_get_environment_document_client_key_rejected() {
    let server = setup_test_server().await;

    let response = server
        .get("/api/v1/environment-document")
        .add_header("X-Environment-Key", &environment_1_api_key())
        .await;

    response.assert_status(axum::http::StatusCode::UNAUTHORIZED);
}
