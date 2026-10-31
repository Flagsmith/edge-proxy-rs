mod fixtures;

use async_trait::async_trait;
use axum_test::TestServer;
use edge_proxy::config::settings::{AppSettings, EnvironmentKeyPair};
use edge_proxy::identities::{EnvironmentKeyType, IdentityStore, IdentityTraits};
use edge_proxy::models::{IdentityWithTraits, TraitModel};
use edge_proxy::routes::create_router;
use fixtures::*;
use rstest::rstest;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

#[derive(Debug, PartialEq)]
struct SaveCall {
    environment_api_key: Value,
    identifier: String,
    key_type: EnvironmentKeyType,
}

struct RecordingIdentityStore {
    traits: IdentityTraits,
    save_calls: Mutex<Vec<SaveCall>>,
}

impl RecordingIdentityStore {
    fn new(traits: Vec<TraitModel>, system_traits: Vec<TraitModel>) -> Self {
        Self {
            traits: IdentityTraits {
                traits,
                system_traits,
            },
            save_calls: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl IdentityStore for RecordingIdentityStore {
    async fn load_traits(
        &self,
        _environment: &Value,
        _identity: &IdentityWithTraits,
    ) -> IdentityTraits {
        self.traits.clone()
    }

    fn save_traits(
        &self,
        environment: &Value,
        identity: &IdentityWithTraits,
        key_type: EnvironmentKeyType,
    ) {
        self.save_calls.lock().unwrap().push(SaveCall {
            environment_api_key: environment["api_key"].clone(),
            identifier: identity.identifier.clone(),
            key_type,
        });
    }
}

fn trait_model(trait_key: &str, trait_value: &str) -> TraitModel {
    TraitModel {
        trait_key: trait_key.to_string(),
        trait_value: json!(trait_value),
        transient: false,
    }
}

async fn setup_test_server(identity_store: Option<Arc<dyn IdentityStore>>) -> TestServer {
    let settings = AppSettings {
        environment_key_pairs: vec![EnvironmentKeyPair {
            server_side_key: environment_1_server_key(),
            client_side_key: environment_1_api_key(),
        }],
        api_url: "http://test".to_string(),
        ..AppSettings::new()
    };

    let (app, service) = create_router(settings, identity_store);

    service
        .cache
        .put_environment(&environment_1_api_key(), environment_1())
        .await;

    TestServer::new(app).unwrap()
}

async fn post_identity(server: &TestServer, environment_key: &str, traits: Value) -> Value {
    let response = server
        .post("/api/v1/identities/")
        .add_header("X-Environment-Key", environment_key)
        .json(&json!({"identifier": "identity_1", "traits": traits}))
        .await;
    response.assert_status_ok();
    response.json()
}

fn feature_2_value(body: &Value) -> &Value {
    &body["flags"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["feature"]["name"] == "feature_2")
        .unwrap()["feature_state_value"]
}

#[rstest]
#[case::segment_match("test", "segment_override")]
#[case::segment_miss("other", "2.3")]
#[tokio::test]
async fn test_identity_without_store_evaluates_and_returns_request_traits(
    #[case] first_name: &str,
    #[case] expected_feature_2_value: &str,
) {
    // Given
    let server = setup_test_server(None).await;
    let request_traits = json!([{"trait_key": "first_name", "trait_value": first_name}]);

    // When
    let body = post_identity(&server, &environment_1_api_key(), request_traits.clone()).await;

    // Then
    assert_eq!(feature_2_value(&body), expected_feature_2_value);
    assert_eq!(body["traits"], request_traits);
}

#[rstest]
#[case::loaded_trait_matches_segment(vec![trait_model("first_name", "test")], vec![], "segment_override")]
#[case::loaded_trait_misses_segment(vec![trait_model("first_name", "other")], vec![], "2.3")]
#[case::system_trait_wins_over_loaded_trait(
    vec![trait_model("first_name", "other")],
    vec![trait_model("first_name", "test")],
    "segment_override"
)]
#[tokio::test]
async fn test_identity_with_store_evaluates_loaded_and_system_traits(
    #[case] traits: Vec<TraitModel>,
    #[case] system_traits: Vec<TraitModel>,
    #[case] expected_feature_2_value: &str,
) {
    // Given
    let identity_store = Arc::new(RecordingIdentityStore::new(traits, system_traits));
    let server = setup_test_server(Some(identity_store)).await;

    // When
    let body = post_identity(
        &server,
        &environment_1_api_key(),
        json!([{"trait_key": "first_name", "trait_value": "ignored"}]),
    )
    .await;

    // Then
    assert_eq!(feature_2_value(&body), expected_feature_2_value);
}

#[tokio::test]
async fn test_identity_with_store_returns_loaded_traits_without_system_traits() {
    // Given
    let identity_store = Arc::new(RecordingIdentityStore::new(
        vec![trait_model("stored_key", "stored_value")],
        vec![trait_model("system_key", "system_value")],
    ));
    let server = setup_test_server(Some(identity_store)).await;

    // When
    let body = post_identity(
        &server,
        &environment_1_api_key(),
        json!([{"trait_key": "request_key", "trait_value": "request_value"}]),
    )
    .await;

    // Then
    assert_eq!(
        body["traits"],
        json!([{"trait_key": "stored_key", "trait_value": "stored_value"}])
    );
}

#[rstest]
#[case::client_key(environment_1_api_key(), EnvironmentKeyType::Client)]
#[case::server_key(environment_1_server_key(), EnvironmentKeyType::Server)]
#[tokio::test]
async fn test_identity_with_store_saves_once_with_key_type(
    #[case] environment_key: String,
    #[case] expected_key_type: EnvironmentKeyType,
) {
    // Given
    let identity_store = Arc::new(RecordingIdentityStore::new(Vec::new(), Vec::new()));
    let server = setup_test_server(Some(identity_store.clone())).await;

    // When
    post_identity(&server, &environment_key, json!([])).await;

    // Then
    assert_eq!(
        *identity_store.save_calls.lock().unwrap(),
        vec![SaveCall {
            environment_api_key: json!(environment_1_api_key()),
            identifier: "identity_1".to_string(),
            key_type: expected_key_type,
        }]
    );
}
