use edge_proxy::cache::{EnvironmentsCache, LocalMemEnvironmentsCache};
use edge_proxy::config::settings::{AppSettings, EnvironmentKeyPair};
use edge_proxy::error::EdgeProxyError;
use edge_proxy::models::IdentityWithTraits;
use edge_proxy::services::environment::EnvironmentService;
use std::sync::Arc;

mod fixtures;
use fixtures::{environment_1, environment_1_api_key};

const TEST_SERVER_KEY: &str = "ser.test_server_key";

fn create_settings(client_key: &str) -> AppSettings {
    AppSettings {
        environment_key_pairs: vec![EnvironmentKeyPair {
            server_side_key: TEST_SERVER_KEY.to_string(),
            client_side_key: client_key.to_string(),
        }],
        api_url: "http://test.api".to_string(),
        ..AppSettings::default()
    }
}

async fn create_loaded_service() -> (Arc<EnvironmentService>, String) {
    let client_key = environment_1_api_key();
    let cache = Arc::new(LocalMemEnvironmentsCache::new());
    let service = Arc::new(EnvironmentService::with_cache(
        create_settings(&client_key),
        cache.clone(),
    ));
    cache.put_environment(&client_key, environment_1()).await;
    (service, client_key)
}

#[tokio::test]
async fn test_removed_environment_rejects_client_and_server_keys() {
    // Given
    let (service, client_key) = create_loaded_service().await;
    assert!(service.get_environment(&client_key).await.is_ok());
    assert!(service.get_environment(TEST_SERVER_KEY).await.is_ok());

    // When
    service.remove_environment(&client_key).await;

    // Then
    assert!(matches!(
        service.get_environment(&client_key).await,
        Err(EdgeProxyError::FlagsmithUnknownKey(_))
    ));
    assert!(matches!(
        service.get_environment(TEST_SERVER_KEY).await,
        Err(EdgeProxyError::FlagsmithUnknownKey(_))
    ));
}

#[tokio::test]
async fn test_remove_by_server_key_removes_the_whole_environment() {
    // Given
    let (service, client_key) = create_loaded_service().await;

    // When
    service.remove_environment(TEST_SERVER_KEY).await;

    // Then
    assert!(matches!(
        service.get_environment(&client_key).await,
        Err(EdgeProxyError::FlagsmithUnknownKey(_))
    ));
}

#[tokio::test]
async fn test_removed_key_is_rejected_by_flags() {
    // Given
    let (service, client_key) = create_loaded_service().await;
    assert!(
        service
            .get_flags_response_data(&client_key, None)
            .await
            .is_ok()
    );

    // When
    service.remove_environment(&client_key).await;

    // Then
    assert!(matches!(
        service.get_flags_response_data(&client_key, None).await,
        Err(EdgeProxyError::FlagsmithUnknownKey(_))
    ));
}

#[tokio::test]
async fn test_removed_key_is_rejected_by_identities() {
    // Given
    let (service, client_key) = create_loaded_service().await;
    let identity = IdentityWithTraits::new("some-user".to_string());
    assert!(
        service
            .get_identity_response_data(&identity, &client_key)
            .await
            .is_ok()
    );

    // When
    service.remove_environment(&client_key).await;

    // Then
    assert!(matches!(
        service
            .get_identity_response_data(&identity, &client_key)
            .await,
        Err(EdgeProxyError::FlagsmithUnknownKey(_))
    ));
}

#[tokio::test]
async fn test_removed_key_is_rejected_by_environment_document() {
    // Given
    let (service, client_key) = create_loaded_service().await;
    assert!(service.get_environment_bytes(&client_key).await.is_ok());
    assert!(service.get_environment_bytes(TEST_SERVER_KEY).await.is_ok());

    // When
    service.remove_environment(&client_key).await;

    // Then
    assert!(matches!(
        service.get_environment_bytes(&client_key).await,
        Err(EdgeProxyError::FlagsmithUnknownKey(_))
    ));
    assert!(matches!(
        service.get_environment_bytes(TEST_SERVER_KEY).await,
        Err(EdgeProxyError::FlagsmithUnknownKey(_))
    ));
}

#[tokio::test]
async fn test_removing_an_unknown_key_leaves_other_environments_alone() {
    // Given
    let (service, client_key) = create_loaded_service().await;

    // When
    service.remove_environment("not-a-configured-key").await;

    // Then
    assert!(service.get_environment(&client_key).await.is_ok());
}

#[tokio::test]
async fn test_removing_an_unknown_key_still_clears_cache_residue_under_it() {
    // Given a document a lost race left in the cache under a key that no
    // longer resolves
    let (service, _client_key) = create_loaded_service().await;
    service
        .cache
        .put_environment("ghost", serde_json::json!({"api_key": "ghost"}))
        .await;

    // When removal is repeated for the unresolvable key
    service.remove_environment("ghost").await;

    // Then the residue is gone
    assert!(service.cache.get_environment("ghost").await.is_none());
}
