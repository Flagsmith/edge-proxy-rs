mod fixtures;

use async_trait::async_trait;
use axum_test::TestServer;
use edge_proxy::config::settings::{AppSettings, EnvironmentKeyPair};
use edge_proxy::identities::{IdentityPersistence, IdentityTraits, RequestOrigin};
use edge_proxy::models::{IdentityWithTraits, TraitModel};
use edge_proxy::routes::create_router;
use fixtures::*;
use rstest::rstest;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

#[derive(Debug, PartialEq)]
struct PersistCall {
    environment_api_key: Value,
    identifier: String,
    origin: RequestOrigin,
}

struct RecordingPersistence {
    traits: IdentityTraits,
    persist_calls: Mutex<Vec<PersistCall>>,
}

impl RecordingPersistence {
    fn new(evaluation: Vec<TraitModel>, response: Vec<TraitModel>) -> Self {
        Self {
            traits: IdentityTraits {
                evaluation,
                response,
            },
            persist_calls: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl IdentityPersistence for RecordingPersistence {
    async fn load(&self, _environment: &Value, _identity: &IdentityWithTraits) -> IdentityTraits {
        self.traits.clone()
    }

    fn persist(&self, environment: &Value, identity: &IdentityWithTraits, origin: RequestOrigin) {
        self.persist_calls.lock().unwrap().push(PersistCall {
            environment_api_key: environment["api_key"].clone(),
            identifier: identity.identifier.clone(),
            origin,
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

async fn setup_test_server(persistence: Option<Arc<dyn IdentityPersistence>>) -> TestServer {
    let settings = AppSettings {
        environment_key_pairs: vec![EnvironmentKeyPair {
            server_side_key: environment_1_server_key(),
            client_side_key: environment_1_api_key(),
        }],
        api_url: "http://test".to_string(),
        ..AppSettings::new()
    };

    let (app, service) = create_router(settings, persistence);

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
async fn test_identity_without_persistence_evaluates_and_returns_request_traits(
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
#[case::loaded_traits_match_segment("other", "test", "segment_override")]
#[case::request_traits_ignored("test", "other", "2.3")]
#[tokio::test]
async fn test_identity_with_persistence_evaluates_loaded_traits(
    #[case] request_first_name: &str,
    #[case] loaded_first_name: &str,
    #[case] expected_feature_2_value: &str,
) {
    // Given
    let persistence = Arc::new(RecordingPersistence::new(
        vec![trait_model("first_name", loaded_first_name)],
        Vec::new(),
    ));
    let server = setup_test_server(Some(persistence)).await;

    // When
    let body = post_identity(
        &server,
        &environment_1_api_key(),
        json!([{"trait_key": "first_name", "trait_value": request_first_name}]),
    )
    .await;

    // Then
    assert_eq!(feature_2_value(&body), expected_feature_2_value);
}

#[tokio::test]
async fn test_identity_with_persistence_returns_loaded_response_traits() {
    // Given
    let persistence = Arc::new(RecordingPersistence::new(
        Vec::new(),
        vec![trait_model("stored_key", "stored_value")],
    ));
    let server = setup_test_server(Some(persistence)).await;

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
#[case::client_key(environment_1_api_key(), RequestOrigin::Client)]
#[case::server_key(environment_1_server_key(), RequestOrigin::Server)]
#[tokio::test]
async fn test_identity_with_persistence_persists_once_with_origin(
    #[case] environment_key: String,
    #[case] expected_origin: RequestOrigin,
) {
    // Given
    let persistence = Arc::new(RecordingPersistence::new(Vec::new(), Vec::new()));
    let server = setup_test_server(Some(persistence.clone())).await;

    // When
    post_identity(&server, &environment_key, json!([])).await;

    // Then
    assert_eq!(
        *persistence.persist_calls.lock().unwrap(),
        vec![PersistCall {
            environment_api_key: json!(environment_1_api_key()),
            identifier: "identity_1".to_string(),
            origin: expected_origin,
        }]
    );
}
