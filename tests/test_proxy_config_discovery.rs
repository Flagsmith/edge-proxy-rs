use edge_proxy::config::settings::{AppSettings, EnvironmentKeyPair};
use edge_proxy::error::EdgeProxyError;
use edge_proxy::services::EnvironmentService;
use serde_json::{Value, json};
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const PROXY_KEY: &str = "pk.test_proxy_key";
const CLIENT_KEY: &str = "config_client_key";
const SERVER_KEY: &str = "ser.config_key";

fn settings(api_url: &str, pairs: Vec<EnvironmentKeyPair>) -> AppSettings {
    AppSettings {
        environment_key_pairs: pairs,
        proxy_key: Some(PROXY_KEY.to_string()),
        api_url: api_url.to_string(),
        ..AppSettings::default()
    }
}

/// The frozen contract shape, extra fields included, so the tests prove
/// serde tolerates everything the endpoint actually sends.
fn config_body(environments: &[(&str, &str)]) -> Value {
    Value::Array(
        environments
            .iter()
            .map(|(client_key, server_key)| {
                json!({
                    "id": 30,
                    "name": "Test Environment",
                    "client_side_key": client_key,
                    "server_side_keys": [
                        {"key": server_key, "active": true, "expires_at": null}
                    ],
                    "updated_at": "2026-08-15T08:57:43.311081Z",
                    "project_id": 35,
                    "organisation_id": 82,
                })
            })
            .collect(),
    )
}

fn document_body(client_key: &str) -> Value {
    json!({
        "id": 1,
        "api_key": client_key,
        "name": "Test",
        "updated_at": "2026-08-22T00:00:00Z",
        "allow_client_traits": true,
        "hide_sensitive_data": false,
        "hide_disabled_flags": null,
        "use_identity_composite_key_for_hashing": true,
        "use_identity_overrides_in_local_eval": true,
        "project": {
            "id": 1,
            "name": "project-1",
            "hide_disabled_flags": false,
            "segments": [],
            "server_key_only_feature_ids": [],
            "organisation": {
                "id": 1,
                "name": "org-1",
                "feature_analytics": false,
                "persist_trait_data": true,
                "stop_serving_flags": false,
            },
        },
        "feature_states": [
            {
                "multivariate_feature_state_values": [],
                "feature_state_value": "config_value",
                "feature": {"id": 1, "name": "config_flag", "type": "STANDARD"},
                "enabled": true,
                "featurestate_uuid": "fs-uuid-1",
            }
        ],
        "identity_overrides": [],
    })
}

async fn mount_config(mock_server: &MockServer, body: Value, up_to: Option<u64>) {
    let mut mock = Mock::given(method("GET"))
        .and(path("/proxy/config/"))
        .and(header("X-Proxy-Key", PROXY_KEY))
        .respond_with(ResponseTemplate::new(200).set_body_json(body));
    if let Some(n) = up_to {
        mock = mock.up_to_n_times(n);
    }
    mock.mount(mock_server).await;
}

async fn mount_document(mock_server: &MockServer, server_key: &str, client_key: &str) {
    Mock::given(method("GET"))
        .and(path("/environment-document/"))
        .and(header("X-Environment-Key", server_key))
        .respond_with(ResponseTemplate::new(200).set_body_json(document_body(client_key)))
        .mount(mock_server)
        .await;
}

#[tokio::test]
async fn test_environment_from_proxy_config_is_served_after_one_refresh() {
    // Given a proxy configured with only a proxy key
    let mock_server = MockServer::start().await;
    mount_config(&mock_server, config_body(&[(CLIENT_KEY, SERVER_KEY)]), None).await;
    mount_document(&mock_server, SERVER_KEY, CLIENT_KEY).await;
    let service = EnvironmentService::new(settings(&mock_server.uri(), vec![]));

    // When
    let all_success = service.refresh_environment_caches().await;

    // Then both keys serve, document and flags included
    assert!(all_success);
    assert!(service.get_environment(CLIENT_KEY).await.is_ok());
    assert!(service.get_environment(SERVER_KEY).await.is_ok());
    let flags = service
        .get_flags_response_data(CLIENT_KEY, None)
        .await
        .unwrap();
    assert_eq!(flags.len(), 1);
}

#[tokio::test]
async fn test_environment_dropped_from_proxy_config_is_removed_and_uncached() {
    // Given a served environment with a primed flags endpoint cache
    let mock_server = MockServer::start().await;
    mount_config(
        &mock_server,
        config_body(&[(CLIENT_KEY, SERVER_KEY)]),
        Some(1),
    )
    .await;
    mount_config(&mock_server, config_body(&[]), None).await;
    mount_document(&mock_server, SERVER_KEY, CLIENT_KEY).await;
    let service = EnvironmentService::new(settings(&mock_server.uri(), vec![]));
    service.refresh_environment_caches().await;
    assert!(
        service
            .get_flags_response_data(CLIENT_KEY, None)
            .await
            .is_ok()
    );

    // When the next config response omits the environment
    let all_success = service.refresh_environment_caches().await;

    // Then it is gone — including the primed cache entry, which is
    // consulted before the key gate
    assert!(all_success);
    assert!(matches!(
        service.get_flags_response_data(CLIENT_KEY, None).await,
        Err(EdgeProxyError::FlagsmithUnknownKey(_))
    ));
    assert!(matches!(
        service.get_environment(SERVER_KEY).await,
        Err(EdgeProxyError::FlagsmithUnknownKey(_))
    ));
}

#[tokio::test]
async fn test_static_environment_absent_from_proxy_config_is_never_removed() {
    // Given a statically configured environment the config never mentions
    let mock_server = MockServer::start().await;
    mount_config(&mock_server, config_body(&[]), None).await;
    mount_document(&mock_server, "ser.static_key", "static_client").await;
    let service = EnvironmentService::new(settings(
        &mock_server.uri(),
        vec![EnvironmentKeyPair {
            client_side_key: "static_client".to_string(),
            server_side_key: "ser.static_key".to_string(),
        }],
    ));

    // When
    let all_success = service.refresh_environment_caches().await;

    // Then the static environment keeps being served
    assert!(all_success);
    assert!(service.get_environment("static_client").await.is_ok());
    assert!(service.get_environment("ser.static_key").await.is_ok());
}

#[tokio::test]
async fn test_proxy_config_fetch_failure_removes_nothing() {
    // Given a served environment and a proxy config endpoint that starts
    // failing
    let mock_server = MockServer::start().await;
    mount_config(
        &mock_server,
        config_body(&[(CLIENT_KEY, SERVER_KEY)]),
        Some(1),
    )
    .await;
    Mock::given(method("GET"))
        .and(path("/proxy/config/"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&mock_server)
        .await;
    mount_document(&mock_server, SERVER_KEY, CLIENT_KEY).await;
    let service = EnvironmentService::new(settings(&mock_server.uri(), vec![]));
    assert!(service.refresh_environment_caches().await);

    // When the next sync fails
    let all_success = service.refresh_environment_caches().await;

    // Then the failure is reported but nothing is removed
    assert!(!all_success);
    assert!(service.get_environment(CLIENT_KEY).await.is_ok());
    assert!(service.get_environment(SERVER_KEY).await.is_ok());
}

#[tokio::test]
async fn test_server_key_rotation_stops_serving_the_old_key() {
    // Given a served environment whose document bytes are cached under
    // the old server key
    let mock_server = MockServer::start().await;
    mount_config(
        &mock_server,
        config_body(&[(CLIENT_KEY, "ser.old_key")]),
        Some(1),
    )
    .await;
    mount_config(
        &mock_server,
        config_body(&[(CLIENT_KEY, "ser.new_key")]),
        None,
    )
    .await;
    mount_document(&mock_server, "ser.old_key", CLIENT_KEY).await;
    mount_document(&mock_server, "ser.new_key", CLIENT_KEY).await;
    let service = EnvironmentService::new(settings(&mock_server.uri(), vec![]));
    service.refresh_environment_caches().await;
    assert!(service.get_environment_bytes("ser.old_key").await.is_ok());

    // When the config rotates the server key
    let all_success = service.refresh_environment_caches().await;

    // Then the old key stops serving — cached bytes included — and the
    // new key works
    assert!(all_success);
    assert!(matches!(
        service.get_environment_bytes("ser.old_key").await,
        Err(EdgeProxyError::FlagsmithUnknownKey(_))
    ));
    assert!(service.get_environment_bytes("ser.new_key").await.is_ok());
    assert!(service.get_environment(CLIENT_KEY).await.is_ok());
}

#[tokio::test]
async fn test_environment_without_usable_keys_is_skipped_not_failed() {
    // Given the config reports a healthy environment and a brand-new one
    // with no server-side keys yet
    let mock_server = MockServer::start().await;
    let body = json!([
        {
            "id": 30,
            "name": "healthy",
            "client_side_key": CLIENT_KEY,
            "server_side_keys": [{"key": SERVER_KEY, "active": true, "expires_at": null}],
            "updated_at": "2026-08-15T08:57:43.311081Z",
            "project_id": 35,
            "organisation_id": 82,
        },
        {
            "id": 31,
            "name": "no keys yet",
            "client_side_key": "keyless_client",
            "server_side_keys": [],
            "updated_at": "2026-08-15T08:57:43.311081Z",
            "project_id": 35,
            "organisation_id": 82,
        },
    ]);
    mount_config(&mock_server, body, None).await;
    mount_document(&mock_server, SERVER_KEY, CLIENT_KEY).await;
    let service = EnvironmentService::new(settings(&mock_server.uri(), vec![]));

    // When
    let all_success = service.refresh_environment_caches().await;

    // Then the poll succeeds — /health stays green — the healthy
    // environment serves, and the key-less one is simply not indexed
    assert!(all_success);
    assert!(service.get_environment(CLIENT_KEY).await.is_ok());
    assert!(matches!(
        service.get_environment("keyless_client").await,
        Err(EdgeProxyError::FlagsmithUnknownKey(_))
    ));
}

#[tokio::test]
async fn test_environment_whose_only_key_is_deactivated_is_dropped() {
    // Given a served environment whose only key the config then reports
    // as inactive
    let mock_server = MockServer::start().await;
    mount_config(
        &mock_server,
        config_body(&[(CLIENT_KEY, SERVER_KEY)]),
        Some(1),
    )
    .await;
    let deactivated = json!([{
        "id": 30,
        "name": "Test",
        "client_side_key": CLIENT_KEY,
        "server_side_keys": [{"key": SERVER_KEY, "active": false, "expires_at": null}],
        "updated_at": "2026-08-15T08:57:43.311081Z",
        "project_id": 35,
        "organisation_id": 82,
    }]);
    mount_config(&mock_server, deactivated, None).await;
    mount_document(&mock_server, SERVER_KEY, CLIENT_KEY).await;
    let service = EnvironmentService::new(settings(&mock_server.uri(), vec![]));
    service.refresh_environment_caches().await;
    assert!(service.get_environment(SERVER_KEY).await.is_ok());

    // When
    let all_success = service.refresh_environment_caches().await;

    // Then the environment is dropped entirely and the poll stays healthy
    assert!(all_success);
    assert!(matches!(
        service.get_environment(SERVER_KEY).await,
        Err(EdgeProxyError::FlagsmithUnknownKey(_))
    ));
    assert!(matches!(
        service.get_environment(CLIENT_KEY).await,
        Err(EdgeProxyError::FlagsmithUnknownKey(_))
    ));
}
