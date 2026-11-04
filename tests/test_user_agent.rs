use edge_proxy::config::settings::AppSettings;
use edge_proxy::services::EnvironmentService;
use serde_json::{Value, json};
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const PROXY_KEY: &str = "pk.test_user_agent_key";
const CLIENT_KEY: &str = "user_agent_client_key";
const SERVER_KEY: &str = "ser.user_agent_key";
const EXPECTED_USER_AGENT: &str = concat!("flagsmith-edge-proxy/", env!("CARGO_PKG_VERSION"));

fn document_body() -> Value {
    json!({
        "id": 1,
        "api_key": CLIENT_KEY,
        "name": "Test",
        "updated_at": "2026-11-04T00:00:00Z",
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
        "feature_states": [],
        "identity_overrides": [],
    })
}

#[tokio::test]
async fn test_requests_to_flagsmith_api_send_edge_proxy_user_agent() {
    // Given: the API only answers requests identifying as the edge proxy.
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/proxy/config/"))
        .and(header("User-Agent", EXPECTED_USER_AGENT))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([{
            "client_side_key": CLIENT_KEY,
            "server_side_keys": [{"key": SERVER_KEY, "active": true, "expires_at": null}],
        }])))
        .expect(1)
        .mount(&mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/environment-document/"))
        .and(header("User-Agent", EXPECTED_USER_AGENT))
        .respond_with(ResponseTemplate::new(200).set_body_json(document_body()))
        .expect(1)
        .mount(&mock_server)
        .await;

    let service = EnvironmentService::new(AppSettings {
        proxy_key: Some(PROXY_KEY.to_string()),
        api_url: mock_server.uri(),
        ..AppSettings::default()
    });

    // When
    let success = service.refresh_environment_caches().await;

    // Then: both the proxy config and the environment document were fetched.
    assert!(success);
    assert!(service.cache.get_environment(CLIENT_KEY).await.is_some());
}

#[test]
fn test_user_agent_is_name_slash_semver() {
    let version = EXPECTED_USER_AGENT
        .strip_prefix("flagsmith-edge-proxy/")
        .unwrap();
    let parts: Vec<&str> = version.split('.').collect();
    assert_eq!(parts.len(), 3);
    assert!(parts.iter().all(|p| p.parse::<u64>().is_ok()));
    assert_eq!(
        edge_proxy::services::environment::USER_AGENT,
        EXPECTED_USER_AGENT
    );
}
