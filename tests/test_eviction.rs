use edge_proxy::cache::{EnvironmentsCache, LocalMemEnvironmentsCache};
use edge_proxy::config::settings::{
    AppSettings, EndpointCacheSettings, EndpointCachesSettings, EnvironmentKeyPair,
};
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
        endpoint_caches: EndpointCachesSettings {
            flags: EndpointCacheSettings {
                use_cache: true,
                cache_max_size: 10,
            },
            identities: EndpointCacheSettings {
                use_cache: true,
                cache_max_size: 10,
            },
            environment_document: EndpointCacheSettings {
                use_cache: true,
                cache_max_size: 10,
            },
        },
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
async fn test_evicted_environment_rejects_client_and_server_keys() {
    // Given
    let (service, client_key) = create_loaded_service().await;
    assert!(service.get_environment(&client_key).await.is_ok());
    assert!(service.get_environment(TEST_SERVER_KEY).await.is_ok());

    // When
    service.evict_environment(&client_key).await;

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
async fn test_eviction_by_server_key_removes_the_whole_environment() {
    // Given
    let (service, client_key) = create_loaded_service().await;

    // When
    service.evict_environment(TEST_SERVER_KEY).await;

    // Then
    assert!(matches!(
        service.get_environment(&client_key).await,
        Err(EdgeProxyError::FlagsmithUnknownKey(_))
    ));
}

// The flags endpoint cache is consulted before the key gate, so eviction
// must actively clear it or an evicted key keeps being served from cache.
#[tokio::test]
async fn test_eviction_clears_primed_flags_endpoint_cache() {
    // Given a primed flags endpoint cache
    let (service, client_key) = create_loaded_service().await;
    assert!(
        service
            .get_flags_response_data(&client_key, None)
            .await
            .is_ok()
    );

    // When
    service.evict_environment(&client_key).await;

    // Then
    assert!(matches!(
        service.get_flags_response_data(&client_key, None).await,
        Err(EdgeProxyError::FlagsmithUnknownKey(_))
    ));
}

#[tokio::test]
async fn test_eviction_clears_primed_identities_endpoint_cache() {
    // Given a primed identities endpoint cache
    let (service, client_key) = create_loaded_service().await;
    let identity = IdentityWithTraits::new("some-user".to_string());
    assert!(
        service
            .get_identity_response_data(&identity, &client_key)
            .await
            .is_ok()
    );

    // When
    service.evict_environment(&client_key).await;

    // Then
    assert!(matches!(
        service
            .get_identity_response_data(&identity, &client_key)
            .await,
        Err(EdgeProxyError::FlagsmithUnknownKey(_))
    ));
}

#[tokio::test]
async fn test_eviction_clears_primed_environment_document_cache() {
    // Given a primed environment-document endpoint cache
    let (service, client_key) = create_loaded_service().await;
    assert!(service.get_environment_bytes(&client_key).await.is_ok());
    assert!(service.get_environment_bytes(TEST_SERVER_KEY).await.is_ok());

    // When
    service.evict_environment(&client_key).await;

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
async fn test_evicting_an_unknown_key_is_a_noop() {
    // Given
    let (service, client_key) = create_loaded_service().await;

    // When
    service.evict_environment("not-a-configured-key").await;

    // Then
    assert!(service.get_environment(&client_key).await.is_ok());
}
