use axum_test::TestServer;
use chrono::Utc;
use edge_proxy::config::settings::{AppSettings, HealthCheckSettings};
use edge_proxy::routes::create_router;

#[tokio::test]
async fn test_liveness_check() {
    // Given
    let settings = AppSettings::new();
    let (app, _) = create_router(settings);
    let server = TestServer::new(app).unwrap();

    // When
    let response = server.get("/proxy/health/liveness").await;

    // Then
    response.assert_status_ok();
}

#[tokio::test]
async fn test_health_check_returns_200_if_cache_was_updated_recently() {
    // Given
    let settings = AppSettings::new();
    let (app, service) = create_router(settings);
    {
        let mut last_updated = service.last_updated_at.write().await;
        *last_updated = Some(Utc::now());
    }
    let server = TestServer::new(app).unwrap();

    // When/Then
    for endpoint in ["/health", "/proxy/health", "/proxy/health/readiness"] {
        let response = server.get(endpoint).await;
        response.assert_status_ok();

        let body: serde_json::Value = response.json();
        assert_eq!(body["status"], "ok");
    }
}

#[tokio::test]
async fn test_health_check_returns_503_if_cache_was_not_updated() {
    // Given
    let settings = AppSettings::new();
    let (app, _service) = create_router(settings);
    let server = TestServer::new(app).unwrap();
    // last_updated_at is None by default

    // When/Then
    for endpoint in ["/health", "/proxy/health", "/proxy/health/readiness"] {
        let response = server.get(endpoint).await;
        response.assert_status(axum::http::StatusCode::SERVICE_UNAVAILABLE);

        let body: serde_json::Value = response.json();
        assert_eq!(body["status"], "error");
        assert_eq!(body["reason"], "environment document(s) not updated.");
        assert_eq!(body["last_successful_update"], serde_json::Value::Null);
    }
}

#[tokio::test]
async fn test_health_check_returns_503_if_cache_is_stale() {
    // Given
    let settings = AppSettings::new();
    let (app, service) = create_router(settings);
    {
        let mut last_updated = service.last_updated_at.write().await;
        *last_updated = Some(Utc::now() - chrono::Duration::days(10));
    }
    let server = TestServer::new(app).unwrap();

    // When/Then
    for endpoint in ["/health", "/proxy/health", "/proxy/health/readiness"] {
        let response = server.get(endpoint).await;
        response.assert_status(axum::http::StatusCode::SERVICE_UNAVAILABLE);

        let body: serde_json::Value = response.json();
        assert_eq!(body["status"], "error");
        assert_eq!(body["reason"], "environment document(s) stale.");
        assert!(body["last_successful_update"].is_string());
    }
}

#[tokio::test]
async fn test_health_check_returns_200_if_cache_is_never_stale() {
    // Given
    let mut settings = AppSettings::new();
    settings.health_check = HealthCheckSettings {
        environment_update_grace_period_seconds: None,
    };
    let (app, service) = create_router(settings);
    let last_update_time = Utc::now() - chrono::Duration::days(10);
    {
        let mut last_updated = service.last_updated_at.write().await;
        *last_updated = Some(last_update_time);
    }
    let server = TestServer::new(app).unwrap();

    // When/Then
    for endpoint in ["/health", "/proxy/health", "/proxy/health/readiness"] {
        let response = server.get(endpoint).await;
        response.assert_status_ok();

        let body: serde_json::Value = response.json();
        assert_eq!(body["status"], "ok");
        assert_eq!(body["reason"], serde_json::Value::Null);
        assert!(body["last_successful_update"].is_string());
    }
}
