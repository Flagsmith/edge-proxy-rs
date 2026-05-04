use edge_proxy::config::settings::{AppSettings, EnvironmentKeyPair};
use edge_proxy::services::EnvironmentService;
use serde_json::{Value, json};
use std::time::Duration;
use tracing_test::traced_test;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const SERVER_KEY: &str = "ser.test_pagination_key";
const CLIENT_KEY: &str = "client_pagination_key";

fn settings_for(api_url: &str, poll_seconds: u64) -> AppSettings {
    AppSettings {
        environment_key_pairs: vec![EnvironmentKeyPair {
            server_side_key: SERVER_KEY.to_string(),
            client_side_key: CLIENT_KEY.to_string(),
        }],
        api_url: api_url.to_string(),
        api_poll_frequency_seconds: poll_seconds,
        api_poll_timeout_seconds: 5,
        ..AppSettings::default()
    }
}

fn page_body(overrides: &[&str], updated_at: &str) -> Value {
    json!({
        "id": 1,
        "api_key": CLIENT_KEY,
        "name": "Test",
        "updated_at": updated_at,
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
                "feature_state_value": "default",
                "feature": {"id": 1, "name": "test_flag", "type": "STANDARD"},
                "enabled": false,
                "featurestate_uuid": "fs-uuid-1",
            }
        ],
        "identity_overrides": overrides
            .iter()
            .map(|ident| override_for(ident))
            .collect::<Vec<_>>(),
    })
}

fn override_for(identifier: &str) -> Value {
    json!({
        "identifier": identifier,
        "identity_uuid": format!("uuid-{}", identifier),
        "created_date": "2026-05-01T00:00:00Z",
        "environment_api_key": CLIENT_KEY,
        "identity_features": [
            {
                "django_id": null,
                "feature": {"id": 1, "name": "test_flag", "type": "STANDARD"},
                "featurestate_uuid": format!("fsu-{}", identifier),
                "feature_state_value": format!("value-{}", identifier),
                "enabled": true,
            }
        ],
        "identity_traits": [],
        "composite_key": format!("{}_{}", CLIENT_KEY, identifier),
        "django_id": null,
        "dashboard_alias": null,
    })
}

#[tokio::test]
async fn test_fetch_environment_follows_next_link_through_three_pages() {
    // Given: three pages, first two carry rel="next" Link headers.
    let mock = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/v1/environment-document/"))
        .and(query_param_absent("page_id"))
        .and(wiremock::matchers::header("X-Environment-Key", SERVER_KEY))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(page_body(&["user-1", "user-2"], "2026-05-01T00:00:00Z"))
                .insert_header(
                    "Link",
                    "</api/v1/environment-document/?page_id=identity_override%3A1%3Acursor-2>; rel=\"next\"",
                ),
        )
        .expect(1)
        .mount(&mock)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/v1/environment-document/"))
        .and(query_param("page_id", "identity_override:1:cursor-2"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(page_body(&["user-3"], "2026-05-01T00:00:00Z"))
                .insert_header(
                    "Link",
                    "</api/v1/environment-document/?page_id=identity_override%3A1%3Acursor-3>; rel=\"next\"",
                ),
        )
        .expect(1)
        .mount(&mock)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/v1/environment-document/"))
        .and(query_param("page_id", "identity_override:1:cursor-3"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(page_body(&["user-4", "user-5"], "2026-05-01T00:00:00Z")),
        )
        .expect(1)
        .mount(&mock)
        .await;

    let api_url = format!("{}/api/v1", mock.uri());
    let service = EnvironmentService::new(settings_for(&api_url, 60));

    // When
    assert!(service.refresh_environment_caches().await);

    // Then: every override across all three pages is in the cached document.
    let document = service.cache.get_environment(CLIENT_KEY).await.unwrap();
    let overrides = document["identity_overrides"].as_array().unwrap();
    let identifiers: Vec<&str> = overrides
        .iter()
        .map(|o| o["identifier"].as_str().unwrap())
        .collect();
    assert_eq!(
        identifiers,
        vec!["user-1", "user-2", "user-3", "user-4", "user-5"]
    );

    // Page-1 fields are authoritative (project, feature_states, name).
    assert_eq!(document["name"], "Test");
    assert_eq!(document["feature_states"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn test_fetch_environment_single_page_no_link_header() {
    let mock = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/v1/environment-document/"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(page_body(&["only-user"], "2026-05-01T00:00:00Z")),
        )
        .expect(1) // exactly one request — no follow-up
        .mount(&mock)
        .await;

    let api_url = format!("{}/api/v1", mock.uri());
    let service = EnvironmentService::new(settings_for(&api_url, 60));

    assert!(service.refresh_environment_caches().await);

    let document = service.cache.get_environment(CLIENT_KEY).await.unwrap();
    let overrides = document["identity_overrides"].as_array().unwrap();
    assert_eq!(overrides.len(), 1);
    assert_eq!(overrides[0]["identifier"], "only-user");
}

#[tokio::test]
async fn test_fetch_environment_resolves_absolute_next_link() {
    let mock = MockServer::start().await;
    let absolute_next = format!(
        "{}/api/v1/environment-document/?page_id=identity_override%3A1%3Aabsolute",
        mock.uri()
    );

    Mock::given(method("GET"))
        .and(path("/api/v1/environment-document/"))
        .and(query_param_absent("page_id"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(page_body(&["a"], "2026-05-01T00:00:00Z"))
                .insert_header("Link", format!("<{}>; rel=\"next\"", absolute_next)),
        )
        .expect(1)
        .mount(&mock)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/v1/environment-document/"))
        .and(query_param("page_id", "identity_override:1:absolute"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(page_body(&["b"], "2026-05-01T00:00:00Z")),
        )
        .expect(1)
        .mount(&mock)
        .await;

    let api_url = format!("{}/api/v1", mock.uri());
    let service = EnvironmentService::new(settings_for(&api_url, 60));

    assert!(service.refresh_environment_caches().await);
    let document = service.cache.get_environment(CLIENT_KEY).await.unwrap();
    let overrides = document["identity_overrides"].as_array().unwrap();
    assert_eq!(overrides.len(), 2);
    assert_eq!(overrides[0]["identifier"], "a");
    assert_eq!(overrides[1]["identifier"], "b");
}

#[tokio::test]
#[traced_test]
async fn test_fetch_environment_warns_when_exceeds_poll_interval() {
    let mock = MockServer::start().await;

    // Page 1 stalls long enough to push elapsed past poll_frequency_seconds=1
    // before iteration 2 begins; the warning fires at the top of iteration 2.
    Mock::given(method("GET"))
        .and(path("/api/v1/environment-document/"))
        .and(query_param_absent("page_id"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(page_body(&["a"], "2026-05-01T00:00:00Z"))
                .insert_header(
                    "Link",
                    "</api/v1/environment-document/?page_id=identity_override%3A1%3Aslow>; rel=\"next\"",
                )
                .set_delay(Duration::from_millis(1500)),
        )
        .expect(1)
        .mount(&mock)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/v1/environment-document/"))
        .and(query_param("page_id", "identity_override:1:slow"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(page_body(&["b"], "2026-05-01T00:00:00Z")),
        )
        .expect(1)
        .mount(&mock)
        .await;

    let api_url = format!("{}/api/v1", mock.uri());
    let service = EnvironmentService::new(settings_for(&api_url, 1));

    assert!(service.refresh_environment_caches().await);

    assert!(logs_contain(
        "environment-document fetch exceeded the configured poll interval"
    ));
    assert!(logs_contain("elapsed_seconds"));
    assert!(logs_contain("poll_frequency_seconds=1"));
}

// ---- Test helpers below ------------------------------------------------------

fn query_param_absent(name: &'static str) -> impl wiremock::Match {
    struct Absent(&'static str);
    impl wiremock::Match for Absent {
        fn matches(&self, req: &wiremock::Request) -> bool {
            !req.url.query_pairs().any(|(k, _)| k.as_ref() == self.0)
        }
    }
    Absent(name)
}
