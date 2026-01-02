use edge_proxy::cache::endpoint::{CacheKey, EndpointCache};
use edge_proxy::cache::{EnvironmentsCache, LocalMemEnvironmentsCache};
use edge_proxy::config::settings::{
    AppSettings, EndpointCacheSettings, EndpointCachesSettings, EnvironmentKeyPair,
};
use edge_proxy::models::IdentityWithTraits;
use edge_proxy::services::environment::EnvironmentService;
use std::sync::Arc;

mod fixtures;
use fixtures::environment_1;

const TEST_CLIENT_KEY: &str = "test_client_key";
const TEST_SERVER_KEY: &str = "ser.test_server_key";

fn create_settings_with_cache(flags_enabled: bool, identities_enabled: bool) -> AppSettings {
    use edge_proxy::config::settings::{HealthCheckSettings, LoggingSettings, ServerSettings};

    AppSettings {
        environment_key_pairs: vec![EnvironmentKeyPair {
            server_side_key: TEST_SERVER_KEY.to_string(),
            client_side_key: TEST_CLIENT_KEY.to_string(),
        }],
        api_url: "http://test.api".to_string(),
        api_poll_frequency_seconds: 10,
        api_poll_timeout_seconds: 5,
        endpoint_caches: EndpointCachesSettings {
            flags: EndpointCacheSettings {
                use_cache: flags_enabled,
                cache_max_size: 10,
            },
            identities: EndpointCacheSettings {
                use_cache: identities_enabled,
                cache_max_size: 10,
            },
            environment_document: EndpointCacheSettings {
                use_cache: false,
                cache_max_size: 10,
            },
        },
        server: ServerSettings::default(),
        logging: LoggingSettings::default(),
        health_check: HealthCheckSettings::default(),
    }
}

#[tokio::test]
async fn test_flags_endpoint_caching() {
    // Given
    let settings = create_settings_with_cache(true, false);
    let cache = Arc::new(LocalMemEnvironmentsCache::new());
    let service = Arc::new(EnvironmentService::with_cache(settings, cache.clone()));
    cache
        .put_environment(TEST_CLIENT_KEY, environment_1())
        .await;

    // When
    let result1 = service
        .get_flags_response_data(TEST_CLIENT_KEY, None)
        .await
        .unwrap();

    // Then
    assert_eq!(result1.len(), 2);

    // When - second call should hit cache
    let result2 = service
        .get_flags_response_data(TEST_CLIENT_KEY, None)
        .await
        .unwrap();

    // Then
    assert_eq!(result1, result2);

    // When - test with specific feature
    let result3 = service
        .get_flags_response_data(TEST_CLIENT_KEY, Some("feature_1"))
        .await
        .unwrap();

    // Then
    assert_eq!(result3.len(), 1);
    assert_eq!(result3[0].feature.name, "feature_1");
}

#[tokio::test]
async fn test_identities_endpoint_caching() {
    // Given
    let settings = create_settings_with_cache(false, true);
    let cache = Arc::new(LocalMemEnvironmentsCache::new());
    let service = Arc::new(EnvironmentService::with_cache(settings, cache.clone()));
    cache
        .put_environment(TEST_CLIENT_KEY, environment_1())
        .await;

    use edge_proxy::models::TraitModel;
    let identity = IdentityWithTraits {
        identifier: "test_user".to_string(),
        traits: vec![TraitModel {
            trait_key: "test_trait".to_string(),
            trait_value: serde_json::json!("test_value"),
        }],
    };

    // When
    let result1 = service
        .get_identity_response_data(&identity, TEST_CLIENT_KEY)
        .await
        .unwrap();

    // Then
    assert!(!result1.flags.is_empty());

    // When - second call should hit cache
    let result2 = service
        .get_identity_response_data(&identity, TEST_CLIENT_KEY)
        .await
        .unwrap();

    // Then
    assert_eq!(result1, result2);

    // When - different identity should miss cache
    let identity2 = IdentityWithTraits::new("other_user".to_string());
    let result3 = service
        .get_identity_response_data(&identity2, TEST_CLIENT_KEY)
        .await
        .unwrap();

    // Then
    assert!(!result3.flags.is_empty());
}

#[tokio::test]
async fn test_cache_invalidation_on_environment_update() {
    // Given
    let settings = create_settings_with_cache(true, true);
    let cache = Arc::new(LocalMemEnvironmentsCache::new());
    let service = Arc::new(EnvironmentService::with_cache(settings, cache.clone()));
    cache
        .put_environment(TEST_CLIENT_KEY, environment_1())
        .await;

    let _ = service
        .get_flags_response_data(TEST_CLIENT_KEY, None)
        .await
        .unwrap();

    let identity = IdentityWithTraits::new("test_user".to_string());
    let _ = service
        .get_identity_response_data(&identity, TEST_CLIENT_KEY)
        .await
        .unwrap();

    let cache_key_flags = CacheKey::new(
        TEST_CLIENT_KEY.to_string(),
        "flags".to_string(),
        "".to_string(),
    );

    // Then - cache should be populated
    assert!(
        service
            .endpoint_cache
            .get_flags(&cache_key_flags)
            .await
            .is_some()
    );

    // When - simulate environment update
    service
        .endpoint_cache
        .clear_environment(TEST_CLIENT_KEY)
        .await;

    // Then - cache should be cleared
    assert!(
        service
            .endpoint_cache
            .get_flags(&cache_key_flags)
            .await
            .is_none()
    );
}

#[tokio::test]
async fn test_lru_eviction() {
    // Given
    let cache = EndpointCache::new(true, 2, false, 0, false, 0);
    let key1 = CacheKey::new(
        "env1".to_string(),
        "flags".to_string(),
        "feature1".to_string(),
    );
    let key2 = CacheKey::new(
        "env1".to_string(),
        "flags".to_string(),
        "feature2".to_string(),
    );
    let key3 = CacheKey::new(
        "env1".to_string(),
        "flags".to_string(),
        "feature3".to_string(),
    );
    let value1 = serde_json::json!([{"feature": {"name": "feature1"}}]);
    let value2 = serde_json::json!([{"feature": {"name": "feature2"}}]);
    let value3 = serde_json::json!([{"feature": {"name": "feature3"}}]);

    // When - fill cache to capacity
    cache.put_flags(key1.clone(), value1.clone()).await;
    cache.put_flags(key2.clone(), value2.clone()).await;

    // Then - both should be in cache
    assert_eq!(cache.get_flags(&key1).await, Some(value1.clone()));
    assert_eq!(cache.get_flags(&key2).await, Some(value2.clone()));

    // When - add third item
    cache.put_flags(key3.clone(), value3.clone()).await;

    // Then - key1 should be evicted (LRU)
    assert_eq!(cache.get_flags(&key1).await, None);
    assert_eq!(cache.get_flags(&key2).await, Some(value2));
    assert_eq!(cache.get_flags(&key3).await, Some(value3));
}

#[tokio::test]
async fn test_disabled_cache() {
    // Given
    let settings = create_settings_with_cache(false, false);
    let cache = Arc::new(LocalMemEnvironmentsCache::new());
    let service = Arc::new(EnvironmentService::with_cache(settings, cache.clone()));
    cache
        .put_environment(TEST_CLIENT_KEY, environment_1())
        .await;

    // Then - caches should be disabled
    assert!(!service.endpoint_cache.is_flags_cache_enabled());
    assert!(!service.endpoint_cache.is_identities_cache_enabled());

    // When - should still work without caching
    let result = service
        .get_flags_response_data(TEST_CLIENT_KEY, None)
        .await
        .unwrap();

    // Then
    assert!(!result.is_empty());
}
