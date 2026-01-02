use edge_proxy::cache::{EnvironmentsCache, LocalMemEnvironmentsCache};
use edge_proxy::cache::endpoint::{CacheKey, EndpointCache};
use edge_proxy::services::environment::EnvironmentService;
use edge_proxy::models::IdentityWithTraits;
use edge_proxy::config::settings::{AppSettings, EndpointCacheSettings, EndpointCachesSettings, EnvironmentKeyPair};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

mod fixtures;
use fixtures::environment_1;

const TEST_CLIENT_KEY: &str = "test_client_key";
const TEST_SERVER_KEY: &str = "ser.test_server_key";

/// Helper to create AppSettings with LRU cache enabled
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
    let settings = create_settings_with_cache(true, false);
    let cache = Arc::new(LocalMemEnvironmentsCache::new());
    let service = Arc::new(EnvironmentService::with_cache(settings, cache.clone()));

    // Pre-populate the environment cache
    cache
        .put_environment(TEST_CLIENT_KEY, environment_1())
        .await;

    // First call - should compute and cache
    let result1 = service
        .get_flags_response_data(TEST_CLIENT_KEY, None)
        .await
        .unwrap();
    assert_eq!(result1.len(), 2); // Two features (feature_3 is server-only)

    // Second call - should hit cache
    let result2 = service
        .get_flags_response_data(TEST_CLIENT_KEY, None)
        .await
        .unwrap();
    assert_eq!(result1, result2);

    // Test with specific feature
    let result3 = service
        .get_flags_response_data(TEST_CLIENT_KEY, Some("feature_1"))
        .await
        .unwrap();
    assert_eq!(result3.len(), 1);
    assert_eq!(result3[0].feature.name, "feature_1");
}

#[tokio::test]
async fn test_identities_endpoint_caching() {
    let settings = create_settings_with_cache(false, true);
    let cache = Arc::new(LocalMemEnvironmentsCache::new());
    let service = Arc::new(EnvironmentService::with_cache(settings, cache.clone()));

    // Pre-populate the environment cache
    cache
        .put_environment(TEST_CLIENT_KEY, environment_1())
        .await;

    use edge_proxy::models::TraitModel;

    let identity = IdentityWithTraits {
        identifier: "test_user".to_string(),
        traits: vec![
            TraitModel {
                trait_key: "test_trait".to_string(),
                trait_value: serde_json::json!("test_value"),
            }
        ],
    };

    // First call - should compute and cache
    let result1 = service
        .get_identity_response_data(&identity, TEST_CLIENT_KEY)
        .await
        .unwrap();
    assert!(!result1.flags.is_empty());

    // Second call - should hit cache
    let result2 = service
        .get_identity_response_data(&identity, TEST_CLIENT_KEY)
        .await
        .unwrap();
    assert_eq!(result1, result2);

    // Different identity - should miss cache
    let identity2 = IdentityWithTraits::new("other_user".to_string());
    let result3 = service
        .get_identity_response_data(&identity2, TEST_CLIENT_KEY)
        .await
        .unwrap();
    assert!(!result3.flags.is_empty());
}

#[tokio::test]
async fn test_cache_invalidation_on_environment_update() {
    let settings = create_settings_with_cache(true, true);
    let cache = Arc::new(LocalMemEnvironmentsCache::new());
    let service = Arc::new(EnvironmentService::with_cache(settings, cache.clone()));

    // Pre-populate the environment cache
    cache
        .put_environment(TEST_CLIENT_KEY, environment_1())
        .await;

    // Create cache entries
    let _ = service
        .get_flags_response_data(TEST_CLIENT_KEY, None)
        .await
        .unwrap();

    let identity = IdentityWithTraits::new("test_user".to_string());
    let _ = service
        .get_identity_response_data(&identity, TEST_CLIENT_KEY)
        .await
        .unwrap();

    // Verify cache is populated
    let cache_key_flags = CacheKey::new(TEST_CLIENT_KEY.to_string(), "flags".to_string(), "".to_string());
    assert!(service.endpoint_cache.get_flags(&cache_key_flags).await.is_some());

    // Simulate environment update by clearing endpoint cache
    service.endpoint_cache.clear_environment(TEST_CLIENT_KEY).await;

    // Verify cache is cleared
    assert!(service.endpoint_cache.get_flags(&cache_key_flags).await.is_none());
}

#[tokio::test]
async fn test_lru_eviction() {
    // Create cache with size 2
    let cache = EndpointCache::new(true, 2, false, 0, false, 0);

    let key1 = CacheKey::new("env1".to_string(), "flags".to_string(), "feature1".to_string());
    let key2 = CacheKey::new("env1".to_string(), "flags".to_string(), "feature2".to_string());
    let key3 = CacheKey::new("env1".to_string(), "flags".to_string(), "feature3".to_string());

    let value1 = serde_json::json!([{"feature": {"name": "feature1"}}]);
    let value2 = serde_json::json!([{"feature": {"name": "feature2"}}]);
    let value3 = serde_json::json!([{"feature": {"name": "feature3"}}]);

    // Fill cache to capacity
    cache.put_flags(key1.clone(), value1.clone()).await;
    cache.put_flags(key2.clone(), value2.clone()).await;

    // Verify both are in cache
    assert_eq!(cache.get_flags(&key1).await, Some(value1.clone()));
    assert_eq!(cache.get_flags(&key2).await, Some(value2.clone()));

    // Add third item - should evict least recently used (key1)
    cache.put_flags(key3.clone(), value3.clone()).await;

    // key1 should be evicted
    assert_eq!(cache.get_flags(&key1).await, None);
    // key2 and key3 should remain
    assert_eq!(cache.get_flags(&key2).await, Some(value2));
    assert_eq!(cache.get_flags(&key3).await, Some(value3));
}

#[tokio::test]
async fn test_disabled_cache() {
    let settings = create_settings_with_cache(false, false);
    let cache = Arc::new(LocalMemEnvironmentsCache::new());
    let service = Arc::new(EnvironmentService::with_cache(settings, cache.clone()));

    // Pre-populate the environment cache
    cache
        .put_environment(TEST_CLIENT_KEY, environment_1())
        .await;

    // Verify caches are disabled
    assert!(!service.endpoint_cache.is_flags_cache_enabled());
    assert!(!service.endpoint_cache.is_identities_cache_enabled());

    // Should still work but without caching
    let result = service
        .get_flags_response_data(TEST_CLIENT_KEY, None)
        .await
        .unwrap();
    assert!(!result.is_empty());
}