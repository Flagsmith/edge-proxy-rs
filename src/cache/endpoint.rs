use lru::LruCache;
use serde_json::Value;
use std::num::NonZeroUsize;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, trace};

// SAFETY: 128 is non-zero
const DEFAULT_CACHE_SIZE: NonZeroUsize = unsafe { NonZeroUsize::new_unchecked(128) };

/// Key for caching endpoint responses
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct CacheKey {
    pub environment_key: String,
    pub endpoint: String,
    pub params: String, // Serialized query parameters or request body
}

impl CacheKey {
    pub fn new(environment_key: String, endpoint: String, params: String) -> Self {
        Self {
            environment_key,
            endpoint,
            params,
        }
    }
}

/// Thread-safe LRU cache for endpoint responses
pub struct EndpointCache {
    flags_cache: Option<Arc<RwLock<LruCache<CacheKey, Value>>>>,
    identities_cache: Option<Arc<RwLock<LruCache<CacheKey, Value>>>>,
    /// Cache for pre-serialized environment document bytes (zero serialization per request)
    environment_document_cache: Option<Arc<RwLock<LruCache<CacheKey, Arc<[u8]>>>>>,
}

impl EndpointCache {
    pub fn new(
        flags_enabled: bool,
        flags_size: usize,
        identities_enabled: bool,
        identities_size: usize,
        environment_document_enabled: bool,
        environment_document_size: usize,
    ) -> Self {
        let flags_cache = if flags_enabled && flags_size > 0 {
            Some(Arc::new(RwLock::new(LruCache::new(
                NonZeroUsize::new(flags_size).unwrap_or(DEFAULT_CACHE_SIZE)
            ))))
        } else {
            None
        };

        let identities_cache = if identities_enabled && identities_size > 0 {
            Some(Arc::new(RwLock::new(LruCache::new(
                NonZeroUsize::new(identities_size).unwrap_or(DEFAULT_CACHE_SIZE)
            ))))
        } else {
            None
        };

        let environment_document_cache = if environment_document_enabled && environment_document_size > 0 {
            Some(Arc::new(RwLock::new(LruCache::new(
                NonZeroUsize::new(environment_document_size).unwrap_or(DEFAULT_CACHE_SIZE)
            ))))
        } else {
            None
        };

        Self {
            flags_cache,
            identities_cache,
            environment_document_cache,
        }
    }

    /// Get a cached flags response
    pub async fn get_flags(&self, key: &CacheKey) -> Option<Value> {
        if let Some(cache) = &self.flags_cache {
            let mut cache = cache.write().await;
            let result = cache.get(key).cloned();
            if result.is_some() {
                trace!("Flags cache hit for key: {:?}", key);
            } else {
                trace!("Flags cache miss for key: {:?}", key);
            }
            result
        } else {
            None
        }
    }

    /// Put a flags response in the cache
    pub async fn put_flags(&self, key: CacheKey, value: Value) {
        if let Some(cache) = &self.flags_cache {
            let mut cache = cache.write().await;
            cache.put(key.clone(), value);
            trace!("Cached flags response for key: {:?}", key);
        }
    }

    /// Get a cached identities response
    pub async fn get_identity(&self, key: &CacheKey) -> Option<Value> {
        if let Some(cache) = &self.identities_cache {
            let mut cache = cache.write().await;
            let result = cache.get(key).cloned();
            if result.is_some() {
                trace!("Identity cache hit for key: {:?}", key);
            } else {
                trace!("Identity cache miss for key: {:?}", key);
            }
            result
        } else {
            None
        }
    }

    /// Put an identity response in the cache
    pub async fn put_identity(&self, key: CacheKey, value: Value) {
        if let Some(cache) = &self.identities_cache {
            let mut cache = cache.write().await;
            cache.put(key.clone(), value);
            trace!("Cached identity response for key: {:?}", key);
        }
    }

    /// Get cached pre-serialized environment document bytes
    pub async fn get_environment_document(&self, key: &CacheKey) -> Option<Arc<[u8]>> {
        if let Some(cache) = &self.environment_document_cache {
            let mut cache = cache.write().await;
            let result = cache.get(key).cloned();
            if result.is_some() {
                trace!("Environment document cache hit for key: {:?}", key);
            } else {
                trace!("Environment document cache miss for key: {:?}", key);
            }
            result
        } else {
            None
        }
    }

    /// Put pre-serialized environment document bytes in the cache
    pub async fn put_environment_document(&self, key: CacheKey, bytes: Arc<[u8]>) {
        if let Some(cache) = &self.environment_document_cache {
            let mut cache = cache.write().await;
            cache.put(key.clone(), bytes);
            trace!("Cached environment document for key: {:?}", key);
        }
    }

    /// Clear all cached responses for a specific environment
    pub async fn clear_environment(&self, environment_key: &str) {
        debug!("Clearing endpoint caches for environment: {}", environment_key);

        if let Some(cache) = &self.flags_cache {
            let mut cache = cache.write().await;
            // Collect keys first - can't mutate while iterating LruCache
            let keys_to_remove: Vec<CacheKey> = cache
                .iter()
                .filter(|(k, _)| k.environment_key == environment_key)
                .map(|(k, _)| k.clone())
                .collect();
            for key in keys_to_remove {
                cache.pop(&key);
            }
        }

        if let Some(cache) = &self.identities_cache {
            let mut cache = cache.write().await;
            let keys_to_remove: Vec<CacheKey> = cache
                .iter()
                .filter(|(k, _)| k.environment_key == environment_key)
                .map(|(k, _)| k.clone())
                .collect();
            for key in keys_to_remove {
                cache.pop(&key);
            }
        }

        if let Some(cache) = &self.environment_document_cache {
            let mut cache = cache.write().await;
            let keys_to_remove: Vec<CacheKey> = cache
                .iter()
                .filter(|(k, _)| k.environment_key == environment_key)
                .map(|(k, _)| k.clone())
                .collect();
            for key in keys_to_remove {
                cache.pop(&key);
            }
        }
    }

    /// Clear all caches
    pub async fn clear_all(&self) {
        debug!("Clearing all endpoint caches");

        if let Some(cache) = &self.flags_cache {
            let mut cache = cache.write().await;
            cache.clear();
        }

        if let Some(cache) = &self.identities_cache {
            let mut cache = cache.write().await;
            cache.clear();
        }

        if let Some(cache) = &self.environment_document_cache {
            let mut cache = cache.write().await;
            cache.clear();
        }
    }

    /// Check if flags caching is enabled
    pub fn is_flags_cache_enabled(&self) -> bool {
        self.flags_cache.is_some()
    }

    /// Check if identities caching is enabled
    pub fn is_identities_cache_enabled(&self) -> bool {
        self.identities_cache.is_some()
    }

    /// Check if environment document caching is enabled
    pub fn is_environment_document_cache_enabled(&self) -> bool {
        self.environment_document_cache.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_flags_cache_operations() {
        let cache = EndpointCache::new(true, 2, false, 0, false, 0);

        let key1 = CacheKey::new("env1".to_string(), "flags".to_string(), "".to_string());
        let key2 = CacheKey::new("env1".to_string(), "flags".to_string(), "feature=test".to_string());
        let key3 = CacheKey::new("env2".to_string(), "flags".to_string(), "".to_string());

        let value1 = serde_json::json!({"flag1": true});
        let value2 = serde_json::json!({"flag2": false});
        let value3 = serde_json::json!({"flag3": true});

        // Test put and get
        cache.put_flags(key1.clone(), value1.clone()).await;
        assert_eq!(cache.get_flags(&key1).await, Some(value1.clone()));

        // Test LRU eviction (cache size is 2)
        cache.put_flags(key2.clone(), value2.clone()).await;
        cache.put_flags(key3.clone(), value3.clone()).await;

        // key1 should be evicted
        assert_eq!(cache.get_flags(&key1).await, None);
        assert_eq!(cache.get_flags(&key2).await, Some(value2.clone()));
        assert_eq!(cache.get_flags(&key3).await, Some(value3));
    }

    #[tokio::test]
    async fn test_identity_cache_operations() {
        let cache = EndpointCache::new(false, 0, true, 2, false, 0);

        let key1 = CacheKey::new("env1".to_string(), "identities".to_string(), "user1".to_string());
        let value1 = serde_json::json!({"identity": "user1", "flags": []});

        cache.put_identity(key1.clone(), value1.clone()).await;
        assert_eq!(cache.get_identity(&key1).await, Some(value1));
    }

    #[tokio::test]
    async fn test_clear_environment() {
        let cache = EndpointCache::new(true, 10, true, 10, false, 0);

        let env1_key1 = CacheKey::new("env1".to_string(), "flags".to_string(), "".to_string());
        let env1_key2 = CacheKey::new("env1".to_string(), "identities".to_string(), "user1".to_string());
        let env2_key1 = CacheKey::new("env2".to_string(), "flags".to_string(), "".to_string());

        let value = serde_json::json!({"test": true});

        cache.put_flags(env1_key1.clone(), value.clone()).await;
        cache.put_identity(env1_key2.clone(), value.clone()).await;
        cache.put_flags(env2_key1.clone(), value.clone()).await;

        // Clear env1
        cache.clear_environment("env1").await;

        // env1 keys should be gone
        assert_eq!(cache.get_flags(&env1_key1).await, None);
        assert_eq!(cache.get_identity(&env1_key2).await, None);

        // env2 keys should remain
        assert_eq!(cache.get_flags(&env2_key1).await, Some(value));
    }

    #[tokio::test]
    async fn test_disabled_caches() {
        let cache = EndpointCache::new(false, 0, false, 0, false, 0);

        assert!(!cache.is_flags_cache_enabled());
        assert!(!cache.is_identities_cache_enabled());
        assert!(!cache.is_environment_document_cache_enabled());

        let key = CacheKey::new("env1".to_string(), "flags".to_string(), "".to_string());
        let value = serde_json::json!({"test": true});

        // Operations should be no-ops
        cache.put_flags(key.clone(), value.clone()).await;
        assert_eq!(cache.get_flags(&key).await, None);

        cache.put_identity(key.clone(), value.clone()).await;
        assert_eq!(cache.get_identity(&key).await, None);
    }
}