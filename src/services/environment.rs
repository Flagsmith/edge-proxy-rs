use crate::cache::{CacheKey, EndpointCache, EnvironmentsCache, LocalMemEnvironmentsCache};
use crate::config::settings::{AppSettings, EnvironmentKeyPair};
use crate::error::{EdgeProxyError, Result};
use crate::models::{APIFeatureState, IdentityResponse, IdentityWithTraits};
use crate::services::feature_utils::filter_out_server_key_only_flag_results;
use flagsmith_flag_engine::identities::Trait as FlagsmithTrait;
use chrono::{DateTime, Utc};
use flagsmith_flag_engine::engine::get_evaluation_result;
use flagsmith_flag_engine::engine_eval::{add_identity_to_context, FlagResult};
use reqwest::Client;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tracing::{debug, error, info};

pub struct EnvironmentService {
    pub cache: Arc<dyn EnvironmentsCache>,
    pub endpoint_cache: Arc<EndpointCache>,
    client: Client,
    pub settings: AppSettings,
    pub last_updated_at: Arc<RwLock<Option<DateTime<Utc>>>>,
    key_mapping: HashMap<String, String>,        // any_key -> server_key (for validation)
    server_to_client: HashMap<String, String>,   // server_key -> client_key (for cache lookup)
}

impl EnvironmentService {
    pub fn new(settings: AppSettings) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(settings.api_poll_timeout_seconds))
            .gzip(true)
            .build()
            .expect("Failed to create HTTP client");

        let mut key_mapping = HashMap::new();
        let mut server_to_client = HashMap::new();
        for pair in &settings.environment_key_pairs {
            key_mapping.insert(
                pair.client_side_key.clone(),
                pair.server_side_key.clone(),
            );
            // Also allow server keys to map to themselves
            key_mapping.insert(
                pair.server_side_key.clone(),
                pair.server_side_key.clone(),
            );
            // Map server key to client key for cache lookup
            server_to_client.insert(
                pair.server_side_key.clone(),
                pair.client_side_key.clone(),
            );
        }

        // Create endpoint cache based on settings
        let endpoint_cache = Arc::new(EndpointCache::new(
            settings.endpoint_caches.flags.use_cache,
            settings.endpoint_caches.flags.cache_max_size,
            settings.endpoint_caches.identities.use_cache,
            settings.endpoint_caches.identities.cache_max_size,
            settings.endpoint_caches.environment_document.use_cache,
            settings.endpoint_caches.environment_document.cache_max_size,
        ));

        Self {
            cache: Arc::new(LocalMemEnvironmentsCache::new()),
            endpoint_cache,
            client,
            settings,
            last_updated_at: Arc::new(RwLock::new(None)),
            key_mapping,
            server_to_client,
        }
    }

    pub fn with_cache(settings: AppSettings, cache: Arc<dyn EnvironmentsCache>) -> Self {
        let mut service = Self::new(settings);
        service.cache = cache;
        service
    }

    pub async fn refresh_environment_caches(&self) -> bool {
        let mut all_success = true;

        for pair in &self.settings.environment_key_pairs {
            match self.fetch_environment(&pair).await {
                Ok(document) => {
                    let changed = self
                        .cache
                        .put_environment(&pair.client_side_key, document)
                        .await;
                    if changed {
                        info!(
                            "Environment cache updated for key: {}",
                            pair.client_side_key
                        );
                        // Clear endpoint caches for this environment when it's updated
                        self.endpoint_cache.clear_environment(&pair.client_side_key).await;
                        // Also clear for server key
                        self.endpoint_cache.clear_environment(&pair.server_side_key).await;
                    }
                }
                Err(e) => {
                    error!(
                        "Failed to fetch environment for key {}: {}",
                        pair.server_side_key, e
                    );
                    all_success = false;
                }
            }
        }

        if all_success {
            let mut last_updated = self.last_updated_at.write().await;
            *last_updated = Some(Utc::now());
        }

        all_success
    }

    async fn fetch_environment(&self, pair: &EnvironmentKeyPair) -> Result<serde_json::Value> {
        let url = format!("{}/environment-document/", self.settings.api_url);

        // Check if we have a cached environment with updated_at timestamp
        let if_modified_since = if let Some(cached_doc) = self.cache.get_environment(&pair.client_side_key).await {
            // Extract updated_at from cached document
            cached_doc
                .get("updated_at")
                .and_then(|v| v.as_str())
                .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                .map(|dt| dt.to_rfc2822())
        } else {
            None
        };

        // Build request with optional If-Modified-Since header
        let mut request = self
            .client
            .get(&url)
            .header("X-Environment-Key", &pair.server_side_key);

        if let Some(if_modified_since_value) = if_modified_since {
            request = request.header("If-Modified-Since", if_modified_since_value);
        }

        let response = request.send().await?;

        // Handle 304 Not Modified
        if response.status() == reqwest::StatusCode::NOT_MODIFIED {
            // Return cached document (clone is fine here - this is rare, only on 304)
            return self.cache
                .get_environment(&pair.client_side_key)
                .await
                .map(|arc| (*arc).clone())
                .ok_or_else(|| {
                    EdgeProxyError::ServiceUnavailable("Cache inconsistency".to_string())
                });
        }

        response.error_for_status_ref()?;

        let document: serde_json::Value = response.json().await?;
        Ok(document)
    }

    pub async fn get_environment(&self, environment_key: &str) -> Result<Arc<serde_json::Value>> {
        // Verify the key is valid
        if !self.key_mapping.contains_key(environment_key) {
            return Err(EdgeProxyError::FlagsmithUnknownKey(
                environment_key.to_string(),
            ));
        }

        // Map server key to client key for cache lookup (cache stores by client key)
        let client_key = self
            .server_to_client
            .get(environment_key)
            .map(|s| s.as_str())
            .unwrap_or(environment_key);

        // Get from cache (returns Arc<Value> to avoid cloning)
        self.cache
            .get_environment(client_key)
            .await
            .ok_or_else(|| {
                EdgeProxyError::ServiceUnavailable("Environment not loaded".to_string())
            })
    }

    /// Get pre-serialized environment document bytes with endpoint caching
    pub async fn get_environment_bytes(&self, environment_key: &str) -> Result<Arc<[u8]>> {
        // Check endpoint cache first if enabled
        if self.endpoint_cache.is_environment_document_cache_enabled() {
            let cache_key = CacheKey::new(
                environment_key.to_string(),
                "environment_document".to_string(),
                "".to_string(),
            );

            if let Some(cached_bytes) = self.endpoint_cache.get_environment_document(&cache_key).await {
                return Ok(cached_bytes);
            }
        }

        // Get the document from main cache
        let document = self.get_environment(environment_key).await?;

        // Serialize to bytes
        let bytes: Arc<[u8]> = serde_json::to_vec(&*document)?.into();

        // Cache the serialized bytes if enabled
        if self.endpoint_cache.is_environment_document_cache_enabled() {
            let cache_key = CacheKey::new(
                environment_key.to_string(),
                "environment_document".to_string(),
                "".to_string(),
            );
            self.endpoint_cache.put_environment_document(cache_key, bytes.clone()).await;
        }

        Ok(bytes)
    }

    /// Extract server_key_only_feature_ids from cached document
    fn extract_server_key_only_ids(document: &serde_json::Value) -> Vec<u32> {
        document
            .get("project")
            .and_then(|p| p.get("server_key_only_feature_ids"))
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_u64().map(|n| n as u32))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub async fn get_flags_response_data(
        &self,
        environment_key: &str,
        feature_name: Option<&str>,
    ) -> Result<Vec<APIFeatureState>> {
        // Check cache if enabled
        if self.endpoint_cache.is_flags_cache_enabled() {
            let cache_key = CacheKey::new(
                environment_key.to_string(),
                "flags".to_string(),
                feature_name.unwrap_or("").to_string(),
            );

            if let Some(cached) = self.endpoint_cache.get_flags(&cache_key).await {
                if let Ok(flags) = serde_json::from_value::<Vec<APIFeatureState>>(cached) {
                    return Ok(flags);
                }
            }
        }

        // Verify the key is valid
        if !self.key_mapping.contains_key(environment_key) {
            return Err(EdgeProxyError::FlagsmithUnknownKey(
                environment_key.to_string(),
            ));
        }

        // Get pre-computed context from cache
        let context = self.cache
            .get_context(environment_key)
            .await
            .ok_or_else(|| {
                EdgeProxyError::ServiceUnavailable("Environment not loaded".to_string())
            })?;

        // Evaluate using cached context
        let evaluation_result = get_evaluation_result(&context);

        // Convert HashMap to Vec of FlagResults
        let mut flag_results: Vec<FlagResult> = evaluation_result.flags.into_values().collect();

        // Filter by feature name if specified
        if let Some(name) = feature_name {
            flag_results.retain(|fr| fr.name == name);
            if flag_results.is_empty() {
                return Err(EdgeProxyError::FeatureNotFound(name.to_string()));
            }
        }

        // Filter out server-key-only features if using client key
        if !environment_key.starts_with("ser.") {
            // Get server_key_only_feature_ids from cached document
            if let Some(document) = self.cache.get_environment(environment_key).await {
                let server_key_only_ids = Self::extract_server_key_only_ids(&document);
                flag_results = filter_out_server_key_only_flag_results(
                    flag_results,
                    &server_key_only_ids,
                );
            }
        }

        // Check if specific feature was filtered out
        if feature_name.is_some() && flag_results.is_empty() {
            return Err(EdgeProxyError::FeatureNotFound(
                feature_name.unwrap().to_string(),
            ));
        }

        let result: Vec<APIFeatureState> = flag_results.iter().map(Into::into).collect();

        // Cache the result if enabled
        if self.endpoint_cache.is_flags_cache_enabled() {
            let cache_key = CacheKey::new(
                environment_key.to_string(),
                "flags".to_string(),
                feature_name.unwrap_or("").to_string(),
            );

            if let Ok(value) = serde_json::to_value(&result) {
                self.endpoint_cache.put_flags(cache_key, value).await;
            }
        }

        Ok(result)
    }

    pub async fn get_identity_response_data(
        &self,
        identity: &IdentityWithTraits,
        environment_key: &str,
    ) -> Result<IdentityResponse> {
        // Check cache if enabled
        if self.endpoint_cache.is_identities_cache_enabled() {
            // Create cache key from identity data
            let cache_params = serde_json::to_string(&identity).unwrap_or_else(|_| identity.identifier.clone());
            let cache_key = CacheKey::new(
                environment_key.to_string(),
                "identities".to_string(),
                cache_params.clone(),
            );

            if let Some(cached) = self.endpoint_cache.get_identity(&cache_key).await {
                if let Ok(response) = serde_json::from_value::<IdentityResponse>(cached) {
                    return Ok(response);
                }
            }
        }

        // Verify the key is valid
        if !self.key_mapping.contains_key(environment_key) {
            return Err(EdgeProxyError::FlagsmithUnknownKey(
                environment_key.to_string(),
            ));
        }

        // Get pre-computed context from cache
        let context = self.cache
            .get_context(environment_key)
            .await
            .ok_or_else(|| {
                EdgeProxyError::ServiceUnavailable("Environment not loaded".to_string())
            })?;

        // Convert traits to Flagsmith traits
        let flagsmith_traits: Vec<FlagsmithTrait> = identity.traits.iter().map(Into::into).collect();

        // Add identity to context
        let context_with_identity = add_identity_to_context(
            &context,
            &identity.identifier,
            &flagsmith_traits,
        );

        // Get evaluation result
        let evaluation_result = get_evaluation_result(&context_with_identity);

        // Convert HashMap to Vec of FlagResults
        let mut flag_results: Vec<FlagResult> = evaluation_result.flags.into_values().collect();

        // Filter out server-key-only features if using client key
        if !environment_key.starts_with("ser.") {
            // Get server_key_only_feature_ids from cached document
            if let Some(document) = self.cache.get_environment(environment_key).await {
                let server_key_only_ids = Self::extract_server_key_only_ids(&document);
                flag_results = filter_out_server_key_only_flag_results(
                    flag_results,
                    &server_key_only_ids,
                );
            }
        }

        let result = IdentityResponse {
            flags: flag_results.iter().map(Into::into).collect(),
            traits: identity.traits.iter().map(|t| t.to_response_json()).collect(),
        };

        // Cache the result if enabled
        if self.endpoint_cache.is_identities_cache_enabled() {
            let cache_params = serde_json::to_string(&identity).unwrap_or_else(|_| identity.identifier.clone());
            let cache_key = CacheKey::new(
                environment_key.to_string(),
                "identities".to_string(),
                cache_params,
            );

            if let Ok(value) = serde_json::to_value(&result) {
                self.endpoint_cache.put_identity(cache_key, value).await;
            }
        }

        Ok(result)
    }

    pub async fn poll_environments(self: Arc<Self>) {
        let mut interval = tokio::time::interval(Duration::from_secs(
            self.settings.api_poll_frequency_seconds,
        ));

        loop {
            interval.tick().await;
            debug!("Polling environments...");
            self.refresh_environment_caches().await;
        }
    }
}
