use axum::http::StatusCode;
use axum_test::TestServer;
use edge_proxy::config::settings::{AppSettings, EnvironmentKeyPair};
use edge_proxy::routes::create_router;
use edge_proxy::services::EnvironmentService;
use edge_proxy::usage::Resource;
use serde_json::{Value, json};
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

const PROXY_KEY: &str = "pk.test_proxy_key";
const CLIENT_KEY: &str = "config_client_key";
const SERVER_KEY: &str = "ser.config_key";

fn settings(api_url: &str, proxy_key: Option<&str>, pairs: Vec<EnvironmentKeyPair>) -> AppSettings {
    AppSettings {
        environment_key_pairs: pairs,
        proxy_key: proxy_key.map(str::to_string),
        api_url: api_url.to_string(),
        ..AppSettings::default()
    }
}

fn config_body() -> Value {
    json!([{
        "id": 30,
        "name": "Test Environment",
        "client_side_key": CLIENT_KEY,
        "server_side_keys": [
            {"key": SERVER_KEY, "active": true, "expires_at": null}
        ],
        "updated_at": "2026-08-15T08:57:43.311081Z",
        "project_id": 35,
        "organisation_id": 82,
    }])
}

fn document_body() -> Value {
    json!({
        "id": 1,
        "api_key": CLIENT_KEY,
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

async fn mount_config(mock_server: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/proxy/config/"))
        .and(header("X-Proxy-Key", PROXY_KEY))
        .respond_with(ResponseTemplate::new(200).set_body_json(config_body()))
        .mount(mock_server)
        .await;
}

async fn mount_document(mock_server: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/environment-document/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(document_body()))
        .mount(mock_server)
        .await;
}

async fn mount_usage(mock_server: &MockServer, status: u16, up_to: Option<u64>) {
    let mut mock = Mock::given(method("POST"))
        .and(path("/proxy/usage/"))
        .and(header("X-Proxy-Key", PROXY_KEY))
        .respond_with(ResponseTemplate::new(status));
    if let Some(n) = up_to {
        mock = mock.up_to_n_times(n);
    }
    mock.mount(mock_server).await;
}

async fn requests_to(mock_server: &MockServer, url_path: &str) -> Vec<Request> {
    mock_server
        .received_requests()
        .await
        .unwrap()
        .into_iter()
        .filter(|request| request.url.path() == url_path)
        .collect()
}

/// The usage rows of the request body, sorted by resource for
/// order-independent assertions.
fn usage_rows(request: &Request) -> Vec<Value> {
    let mut rows: Vec<Value> = serde_json::from_slice(&request.body).unwrap();
    rows.sort_by_key(|row| row["resource"].as_str().unwrap().to_string());
    rows
}

#[tokio::test]
async fn test_served_requests_flush_aggregated_usage() {
    // Given a discovered environment served through the full router
    let mock_server = MockServer::start().await;
    mount_config(&mock_server).await;
    mount_document(&mock_server).await;
    mount_usage(&mock_server, 204, None).await;
    let (app, service) = create_router(settings(&mock_server.uri(), Some(PROXY_KEY), vec![]));
    service.refresh_environment_caches().await;
    let server = TestServer::new(app).unwrap();

    // When SDK traffic arrives under both keys, then usage is flushed
    server
        .get("/api/v1/flags")
        .add_header("X-Environment-Key", CLIENT_KEY)
        .await
        .assert_status_ok();
    server
        .get("/api/v1/flags")
        .add_header("X-Environment-Key", CLIENT_KEY)
        .await
        .assert_status_ok();
    server
        .get("/api/v1/identities")
        .add_query_param("identifier", "user_1")
        .add_header("X-Environment-Key", CLIENT_KEY)
        .await
        .assert_status_ok();
    server
        .post("/api/v1/identities")
        .json(&json!({"identifier": "user_2"}))
        .add_header("X-Environment-Key", CLIENT_KEY)
        .await
        .assert_status_ok();
    server
        .get("/api/v1/environment-document")
        .add_header("X-Environment-Key", SERVER_KEY)
        .await
        .assert_status_ok();
    let flushed = service.flush_usage().await;

    // Then one POST reports everything, keyed by the client key even for
    // requests that presented the server key
    assert!(flushed);
    let posts = requests_to(&mock_server, "/proxy/usage/").await;
    assert_eq!(posts.len(), 1);
    assert_eq!(
        usage_rows(&posts[0]),
        vec![
            json!({"client_side_key": CLIENT_KEY, "resource": "environment-document", "count": 1}),
            json!({"client_side_key": CLIENT_KEY, "resource": "flags", "count": 2}),
            json!({"client_side_key": CLIENT_KEY, "resource": "identities", "count": 2}),
        ]
    );
}

#[tokio::test]
async fn test_unresolved_keys_are_never_counted() {
    // Given a proxy serving one environment
    let mock_server = MockServer::start().await;
    mount_config(&mock_server).await;
    mount_document(&mock_server).await;
    let (app, service) = create_router(settings(&mock_server.uri(), Some(PROXY_KEY), vec![]));
    service.refresh_environment_caches().await;
    let server = TestServer::new(app).unwrap();

    // When requests present an unknown key or none at all
    server
        .get("/api/v1/flags")
        .add_header("X-Environment-Key", "unknown_key")
        .await
        .assert_status_unauthorized();
    server
        .get("/api/v1/flags")
        .await
        .assert_status_unauthorized();

    // Then there is nothing to flush and no request is made
    assert!(service.flush_usage().await);
    assert!(requests_to(&mock_server, "/proxy/usage/").await.is_empty());
}

#[tokio::test]
async fn test_failed_requests_are_not_counted() {
    // Given one environment that serves and one whose document never loaded
    let mock_server = MockServer::start().await;
    let mut environments = config_body();
    environments.as_array_mut().unwrap().push(json!({
        "id": 31,
        "name": "Broken Environment",
        "client_side_key": "broken_client",
        "server_side_keys": [
            {"key": "ser.broken_key", "active": true, "expires_at": null}
        ],
        "updated_at": "2026-08-15T08:57:43.311081Z",
        "project_id": 35,
        "organisation_id": 82,
    }));
    Mock::given(method("GET"))
        .and(path("/proxy/config/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(environments))
        .mount(&mock_server)
        .await;
    Mock::given(method("GET"))
        .and(path("/environment-document/"))
        .and(header("X-Environment-Key", "ser.broken_key"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&mock_server)
        .await;
    mount_document(&mock_server).await;
    let (app, service) = create_router(settings(&mock_server.uri(), Some(PROXY_KEY), vec![]));
    service.refresh_environment_caches().await;
    let server = TestServer::new(app).unwrap();

    // When requests fail after their key resolved
    server
        .get("/api/v1/flags")
        .add_query_param("feature", "missing")
        .add_header("X-Environment-Key", CLIENT_KEY)
        .await
        .assert_status(StatusCode::NOT_FOUND);
    server
        .get("/api/v1/flags")
        .add_header("X-Environment-Key", "broken_client")
        .await
        .assert_status(StatusCode::SERVICE_UNAVAILABLE);

    // Then nothing is reported
    assert!(service.flush_usage().await);
    assert!(requests_to(&mock_server, "/proxy/usage/").await.is_empty());
}

#[tokio::test]
async fn test_failed_flush_merges_counts_into_the_next() {
    // Given a served request and a usage endpoint that fails once
    let mock_server = MockServer::start().await;
    mount_config(&mock_server).await;
    mount_document(&mock_server).await;
    mount_usage(&mock_server, 500, Some(1)).await;
    mount_usage(&mock_server, 204, None).await;
    let service = EnvironmentService::new(settings(&mock_server.uri(), Some(PROXY_KEY), vec![]));
    service.refresh_environment_caches().await;
    service.track_usage(CLIENT_KEY, Resource::Flags);

    // When the first flush fails and another request is served
    assert!(!service.flush_usage().await);
    service.track_usage(CLIENT_KEY, Resource::Flags);

    // Then the next flush carries both counts — nothing lost, nothing
    // double-counted
    assert!(service.flush_usage().await);
    let posts = requests_to(&mock_server, "/proxy/usage/").await;
    assert_eq!(posts.len(), 2);
    assert_eq!(
        usage_rows(&posts[1]),
        vec![json!({"client_side_key": CLIENT_KEY, "resource": "flags", "count": 2})]
    );
}

#[tokio::test]
async fn test_flush_without_proxy_key_is_inert() {
    // Given a statically configured proxy with no proxy key
    let mock_server = MockServer::start().await;
    mount_document(&mock_server).await;
    let service = EnvironmentService::new(settings(
        &mock_server.uri(),
        None,
        vec![EnvironmentKeyPair {
            client_side_key: CLIENT_KEY.to_string(),
            server_side_key: SERVER_KEY.to_string(),
        }],
    ));
    service.refresh_environment_caches().await;
    service.track_usage(CLIENT_KEY, Resource::Flags);

    // When / Then: flushing succeeds without reporting anything
    assert!(service.flush_usage().await);
    assert!(requests_to(&mock_server, "/proxy/usage/").await.is_empty());
}

#[tokio::test]
async fn test_rejected_flush_drops_the_batch_instead_of_retrying_it() {
    // Given a served request and a usage endpoint that rejects the batch
    let mock_server = MockServer::start().await;
    mount_config(&mock_server).await;
    mount_document(&mock_server).await;
    mount_usage(&mock_server, 400, None).await;
    let service = EnvironmentService::new(settings(&mock_server.uri(), Some(PROXY_KEY), vec![]));
    service.refresh_environment_caches().await;
    service.track_usage(CLIENT_KEY, Resource::Flags);

    // When the flush is rejected
    assert!(!service.flush_usage().await);

    // Then the rows are dropped, not resent forever: the next flush has
    // nothing to send
    assert!(service.flush_usage().await);
    assert_eq!(requests_to(&mock_server, "/proxy/usage/").await.len(), 1);
}

#[tokio::test]
async fn test_flush_chunks_batches_to_the_server_cap() {
    // Given served requests for more environments than one batch may hold
    let environments: Vec<Value> = (0..1001)
        .map(|n| {
            json!({
                "id": n,
                "name": format!("env {n}"),
                "client_side_key": format!("client_{n}"),
                "server_side_keys": [
                    {"key": format!("ser.key_{n}"), "active": true, "expires_at": null}
                ],
                "updated_at": "2026-08-15T08:57:43.311081Z",
                "project_id": 35,
                "organisation_id": 82,
            })
        })
        .collect();
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/proxy/config/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(Value::Array(environments)))
        .mount(&mock_server)
        .await;
    mount_document(&mock_server).await;
    mount_usage(&mock_server, 204, None).await;
    let service = EnvironmentService::new(settings(&mock_server.uri(), Some(PROXY_KEY), vec![]));
    service.refresh_environment_caches().await;
    for n in 0..1001 {
        service.track_usage(&format!("client_{n}"), Resource::Flags);
    }

    // When
    assert!(service.flush_usage().await);

    // Then the rows arrive split across two accepted requests
    let posts = requests_to(&mock_server, "/proxy/usage/").await;
    let row_counts: Vec<usize> = posts.iter().map(|post| usage_rows(post).len()).collect();
    assert_eq!(row_counts, vec![1000, 1]);
}

#[tokio::test]
async fn test_static_environment_usage_is_neither_counted_nor_marked() {
    // Given a proxy serving a static environment alongside a discovered one
    let mock_server = MockServer::start().await;
    mount_config(&mock_server).await;
    mount_document(&mock_server).await;
    mount_usage(&mock_server, 204, None).await;
    let service = EnvironmentService::new(settings(
        &mock_server.uri(),
        Some(PROXY_KEY),
        vec![EnvironmentKeyPair {
            client_side_key: "static_client".to_string(),
            server_side_key: "ser.static_key".to_string(),
        }],
    ));
    service.refresh_environment_caches().await;

    // When both environments serve a request, and usage is flushed
    service.track_usage("static_client", Resource::Flags);
    service.track_usage(CLIENT_KEY, Resource::Flags);
    assert!(service.flush_usage().await);

    // Then the static environment keeps its old billing: its document
    // fetch is not marked as the proxy's own, and it is not reported
    let fetches = requests_to(&mock_server, "/environment-document/").await;
    let marked: Vec<bool> = fetches
        .iter()
        .map(|request| request.headers.contains_key("X-Proxy-Key"))
        .collect();
    let static_fetches: Vec<&Request> = fetches
        .iter()
        .filter(|request| request.headers["X-Environment-Key"] == "ser.static_key")
        .collect();
    assert!(!static_fetches.is_empty());
    assert!(
        static_fetches
            .iter()
            .all(|request| !request.headers.contains_key("X-Proxy-Key"))
    );
    assert!(marked.contains(&true));
    let posts = requests_to(&mock_server, "/proxy/usage/").await;
    assert_eq!(
        usage_rows(&posts[0]),
        vec![json!({"client_side_key": CLIENT_KEY, "resource": "flags", "count": 1})]
    );
}

#[tokio::test]
async fn test_document_fetch_carries_the_proxy_key() {
    // Given a document endpoint that only answers requests marked as the
    // proxy's own
    let mock_server = MockServer::start().await;
    mount_config(&mock_server).await;
    Mock::given(method("GET"))
        .and(path("/environment-document/"))
        .and(header("X-Proxy-Key", PROXY_KEY))
        .respond_with(ResponseTemplate::new(200).set_body_json(document_body()))
        .mount(&mock_server)
        .await;
    let service = EnvironmentService::new(settings(&mock_server.uri(), Some(PROXY_KEY), vec![]));

    // When / Then
    assert!(service.refresh_environment_caches().await);
    assert!(service.get_environment(CLIENT_KEY).await.is_ok());
}

#[tokio::test]
async fn test_static_document_fetch_omits_the_proxy_key() {
    // Given a statically configured proxy with no proxy key
    let mock_server = MockServer::start().await;
    mount_document(&mock_server).await;
    let service = EnvironmentService::new(settings(
        &mock_server.uri(),
        None,
        vec![EnvironmentKeyPair {
            client_side_key: CLIENT_KEY.to_string(),
            server_side_key: SERVER_KEY.to_string(),
        }],
    ));

    // When
    assert!(service.refresh_environment_caches().await);

    // Then the document fetch is byte-identical to today's
    let fetches = requests_to(&mock_server, "/environment-document/").await;
    assert!(!fetches.is_empty());
    assert!(
        fetches
            .iter()
            .all(|request| !request.headers.contains_key("X-Proxy-Key"))
    );
}
