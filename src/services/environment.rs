use crate::cache::{EnvironmentsCache, LocalMemEnvironmentsCache};
use crate::config::settings::AppSettings;
use crate::environments::{EnvRecord, EnvironmentIndex};
use crate::error::{EdgeProxyError, Result};
use crate::models::{APIFeatureState, IdentityResponse, IdentityWithTraits};
use crate::services::feature_utils::filter_out_server_key_only_flag_results;
use chrono::{DateTime, Utc};
use flagsmith_flag_engine::engine::get_evaluation_result;
use flagsmith_flag_engine::engine_eval::{FlagResult, add_identity_to_context};
use flagsmith_flag_engine::identities::Trait as FlagsmithTrait;
use reqwest::header::HeaderMap;
use reqwest::{Client, Url};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

pub struct EnvironmentService {
    pub cache: Arc<dyn EnvironmentsCache>,
    client: Client,
    pub settings: AppSettings,
    pub last_updated_at: Arc<RwLock<Option<DateTime<Utc>>>>,
    environments: EnvironmentIndex,
}

impl EnvironmentService {
    pub fn new(settings: AppSettings) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(settings.api_poll_timeout_seconds))
            .gzip(true)
            .build()
            .expect("Failed to create HTTP client");

        let environments = EnvironmentIndex::from_settings(&settings.environment_key_pairs);

        Self {
            cache: Arc::new(LocalMemEnvironmentsCache::new()),
            client,
            settings,
            last_updated_at: Arc::new(RwLock::new(None)),
            environments,
        }
    }

    pub fn with_cache(settings: AppSettings, cache: Arc<dyn EnvironmentsCache>) -> Self {
        let mut service = Self::new(settings);
        service.cache = cache;
        service
    }

    pub async fn refresh_environment_caches(&self) -> bool {
        let mut all_success = true;

        for record in self.environments.records() {
            match self.fetch_environment(&record).await {
                Ok(document) => {
                    let changed = self
                        .cache
                        .put_environment(&record.client_key, document)
                        .await;

                    if changed {
                        info!("Environment cache updated for key: {}", record.client_key);
                    }
                }
                Err(e) => {
                    error!(
                        "Failed to fetch environment for key {}: {}",
                        record.client_key, e
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

    /// Forget an environment at runtime, so requests presenting any of its
    /// keys are rejected and nothing stale is served from a cache.
    pub async fn evict_environment(&self, environment_key: &str) {
        let Some(record) = self.environments.remove(environment_key) else {
            return;
        };
        self.cache.remove_environment(&record.client_key).await;
        info!("Environment evicted for key: {}", record.client_key);
    }

    fn resolve_key(&self, environment_key: &str) -> Result<Arc<EnvRecord>> {
        self.environments
            .resolve(environment_key)
            .ok_or_else(|| EdgeProxyError::FlagsmithUnknownKey(environment_key.to_string()))
    }

    async fn fetch_environment(&self, record: &EnvRecord) -> Result<serde_json::Value> {
        let server_key = record.valid_server_key().ok_or_else(|| {
            EdgeProxyError::ServiceUnavailable(format!(
                "no active server-side key for environment {}",
                record.client_key
            ))
        })?;

        let if_modified_since = self
            .cache
            .get_environment(&record.client_key)
            .await
            .as_deref()
            .and_then(compute_if_modified_since);

        match self
            .fetch_document(&server_key.key, if_modified_since)
            .await?
        {
            Some(document) => Ok(document),
            // 304: upstream confirmed the cached copy is current.
            None => self
                .cache
                .get_environment(&record.client_key)
                .await
                .map(|arc| (*arc).clone())
                .ok_or_else(|| {
                    EdgeProxyError::ServiceUnavailable("Cache inconsistency".to_string())
                }),
        }
    }

    /// Fetch the (paginated) environment document authenticated by
    /// `server_side_key`. Returns `Ok(None)` on 304 Not Modified.
    async fn fetch_document(
        &self,
        server_side_key: &str,
        if_modified_since: Option<String>,
    ) -> Result<Option<serde_json::Value>> {
        let mut next_url = format!("{}/environment-document/", self.settings.api_url);
        let mut document: Option<serde_json::Value> = None;
        let started_at = Instant::now();
        let mut warned_slow = false;

        loop {
            self.warn_if_slow(started_at, &mut warned_slow);

            let mut request = self
                .client
                .get(&next_url)
                .header("X-Environment-Key", server_side_key);
            // If-Modified-Since is meaningful only on the first request; the
            // upstream pagination cursor (page_id) drives subsequent fetches.
            if document.is_none() {
                if let Some(ref value) = if_modified_since {
                    request = request.header("If-Modified-Since", value);
                }
            }

            let response = request.send().await?;

            if document.is_none() && response.status() == reqwest::StatusCode::NOT_MODIFIED {
                return Ok(None);
            }

            response.error_for_status_ref()?;

            let next_link = parse_next_link(response.headers(), &self.settings.api_url);
            let body: serde_json::Value = response.json().await?;

            match document.as_mut() {
                None => document = Some(body),
                Some(base) => merge_paginated_overrides(base, body),
            }

            match next_link {
                Some(url) => next_url = url,
                None => break,
            }
        }

        document.map(Some).ok_or_else(|| {
            EdgeProxyError::ServiceUnavailable("environment-document returned no pages".to_string())
        })
    }

    fn warn_if_slow(&self, started_at: Instant, warned: &mut bool) {
        if *warned {
            return;
        }
        let poll_interval = Duration::from_secs(self.settings.api_poll_frequency_seconds);
        let elapsed = started_at.elapsed();
        if elapsed > poll_interval {
            warn!(
                elapsed_seconds = elapsed.as_secs_f64(),
                poll_frequency_seconds = self.settings.api_poll_frequency_seconds,
                "environment-document fetch exceeded the configured poll interval; \
                 raise api_poll_frequency_seconds or trim the environment"
            );
            *warned = true;
        }
    }

    pub async fn get_environment(&self, environment_key: &str) -> Result<Arc<serde_json::Value>> {
        let record = self.resolve_key(environment_key)?;

        // Documents are cached under the client key, whichever key was presented
        self.cache
            .get_environment(&record.client_key)
            .await
            .ok_or_else(|| EdgeProxyError::ServiceUnavailable("Environment not loaded".to_string()))
    }

    /// Get pre-serialized environment document bytes
    pub async fn get_environment_bytes(&self, environment_key: &str) -> Result<Arc<[u8]>> {
        let document = self.get_environment(environment_key).await?;
        Ok(serde_json::to_vec(&*document)?.into())
    }

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
        // Validation only — lookups below still use the raw presented key, so
        // a server-side key 503s on this endpoint (contexts are stored under
        // the client key)
        self.resolve_key(environment_key)?;

        let context = self
            .cache
            .get_context(environment_key)
            .await
            .ok_or_else(|| {
                EdgeProxyError::ServiceUnavailable("Environment not loaded".to_string())
            })?;

        let evaluation_result = get_evaluation_result(&context);

        let mut flag_results: Vec<FlagResult> = evaluation_result.flags.into_values().collect();

        if let Some(name) = feature_name {
            flag_results.retain(|fr| fr.name == name);
            if flag_results.is_empty() {
                return Err(EdgeProxyError::FeatureNotFound(name.to_string()));
            }
        }

        if !environment_key.starts_with("ser.") {
            if let Some(document) = self.cache.get_environment(environment_key).await {
                let server_key_only_ids = Self::extract_server_key_only_ids(&document);
                flag_results =
                    filter_out_server_key_only_flag_results(flag_results, &server_key_only_ids);
            }
        }

        // Check if specific feature was filtered out
        if let Some(name) = feature_name {
            if flag_results.is_empty() {
                return Err(EdgeProxyError::FeatureNotFound(name.to_string()));
            }
        }

        Ok(flag_results.iter().map(Into::into).collect())
    }

    pub async fn get_identity_response_data(
        &self,
        identity: &IdentityWithTraits,
        environment_key: &str,
    ) -> Result<IdentityResponse> {
        // Validation only — same server-side-key caveat as get_flags_response_data
        self.resolve_key(environment_key)?;

        // Get pre-computed context from cache
        let context = self
            .cache
            .get_context(environment_key)
            .await
            .ok_or_else(|| {
                EdgeProxyError::ServiceUnavailable("Environment not loaded".to_string())
            })?;

        let flagsmith_traits: Vec<FlagsmithTrait> =
            identity.traits.iter().map(Into::into).collect();

        let context_with_identity =
            add_identity_to_context(&context, &identity.identifier, &flagsmith_traits);

        let evaluation_result = get_evaluation_result(&context_with_identity);

        let mut flag_results: Vec<FlagResult> = evaluation_result.flags.into_values().collect();

        // Filter out server-key-only features if using client key
        if !environment_key.starts_with("ser.") {
            // Get server_key_only_feature_ids from cached document
            if let Some(document) = self.cache.get_environment(environment_key).await {
                let server_key_only_ids = Self::extract_server_key_only_ids(&document);
                flag_results =
                    filter_out_server_key_only_flag_results(flag_results, &server_key_only_ids);
            }
        }

        Ok(IdentityResponse {
            flags: flag_results.iter().map(Into::into).collect(),
            traits: identity
                .traits
                .iter()
                .map(|t| t.to_response_json())
                .collect(),
        })
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

/// Format the cached document's `updated_at` as an RFC 2822 `If-Modified-Since`
/// value. Returns `None` if the field is missing or unparseable.
fn compute_if_modified_since(cached_doc: &serde_json::Value) -> Option<String> {
    cached_doc
        .get("updated_at")
        .and_then(|v| v.as_str())
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.to_rfc2822())
}

/// Append `identity_overrides` from a follow-up page onto the page-1 base.
///
/// Subsequent pages echo back the same `project`, `feature_states`, etc.,
/// so merging is purely an extend of the override array.
fn merge_paginated_overrides(base: &mut serde_json::Value, page: serde_json::Value) {
    let serde_json::Value::Object(mut page_obj) = page else {
        return;
    };
    let Some(serde_json::Value::Array(mut new_overrides)) = page_obj.remove("identity_overrides")
    else {
        return;
    };
    if new_overrides.is_empty() {
        return;
    }

    let Some(base_obj) = base.as_object_mut() else {
        return;
    };
    let entry = base_obj
        .entry("identity_overrides")
        .or_insert_with(|| serde_json::Value::Array(Vec::new()));

    if let serde_json::Value::Array(arr) = entry {
        arr.append(&mut new_overrides);
    }
}

/// Parse the `Link` response header for the next-page URL (RFC 5988).
///
/// Returns an absolute URL, resolving relative targets against `api_url`.
fn parse_next_link(headers: &HeaderMap, api_url: &str) -> Option<String> {
    let base = Url::parse(api_url).ok()?;

    for header_value in headers.get_all(reqwest::header::LINK).iter() {
        let raw = header_value.to_str().ok()?;
        for segment in raw.split(',') {
            let segment = segment.trim();
            let target = match (segment.find('<'), segment.find('>')) {
                (Some(start), Some(end)) if end > start + 1 => &segment[start + 1..end],
                _ => continue,
            };
            let params = &segment[segment.find('>').unwrap() + 1..];
            if !is_next_rel(params) {
                continue;
            }
            if let Ok(absolute) = base.join(target) {
                return Some(absolute.into());
            }
        }
    }
    None
}

fn is_next_rel(params: &str) -> bool {
    params.split(';').any(|param| {
        let param = param.trim();
        let Some(value) = param.strip_prefix("rel") else {
            return false;
        };
        let value = value.trim_start();
        let Some(value) = value.strip_prefix('=') else {
            return false;
        };
        let value = value.trim().trim_matches(|c| c == '"' || c == '\'');
        value
            .split_ascii_whitespace()
            .any(|rel| rel.eq_ignore_ascii_case("next"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::{HeaderMap, HeaderValue, LINK};

    fn link(value: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(LINK, HeaderValue::from_str(value).unwrap());
        h
    }

    #[test]
    fn parse_next_link_relative() {
        let headers = link(
            "</api/v1/environment-document/?page_id=identity_override%3A1%3Aabc>; rel=\"next\"",
        );
        let next = parse_next_link(&headers, "https://edge.api.flagsmith.com/api/v1").unwrap();
        assert_eq!(
            next,
            "https://edge.api.flagsmith.com/api/v1/environment-document/?page_id=identity_override%3A1%3Aabc"
        );
    }

    #[test]
    fn parse_next_link_absolute() {
        let headers =
            link("<https://example.test/api/v1/environment-document/?page_id=x>; rel=\"next\"");
        let next = parse_next_link(&headers, "https://edge.api.flagsmith.com/api/v1").unwrap();
        assert_eq!(
            next,
            "https://example.test/api/v1/environment-document/?page_id=x"
        );
    }

    #[test]
    fn parse_next_link_picks_next_among_multiple_rels() {
        let headers = link("</api/v1/page/prev>; rel=\"prev\", </api/v1/page/next>; rel=\"next\"");
        let next = parse_next_link(&headers, "https://edge.api.flagsmith.com/api/v1").unwrap();
        assert_eq!(next, "https://edge.api.flagsmith.com/api/v1/page/next");
    }

    #[test]
    fn parse_next_link_returns_none_when_only_other_rels() {
        let headers = link("</prev>; rel=\"prev\", </self>; rel=\"self\"");
        assert!(parse_next_link(&headers, "https://edge.api.flagsmith.com/api/v1").is_none());
    }

    #[test]
    fn parse_next_link_handles_unquoted_rel() {
        let headers = link("</api/v1/page/next>; rel=next");
        let next = parse_next_link(&headers, "https://edge.api.flagsmith.com/api/v1").unwrap();
        assert_eq!(next, "https://edge.api.flagsmith.com/api/v1/page/next");
    }

    #[test]
    fn merge_paginated_overrides_appends() {
        let mut base = serde_json::json!({
            "identity_overrides": [{"identifier": "a"}],
            "feature_states": []
        });
        let page = serde_json::json!({
            "identity_overrides": [{"identifier": "b"}, {"identifier": "c"}]
        });
        merge_paginated_overrides(&mut base, page);
        let arr = base["identity_overrides"].as_array().unwrap();
        assert_eq!(arr.len(), 3);
        assert_eq!(arr[0]["identifier"], "a");
        assert_eq!(arr[1]["identifier"], "b");
        assert_eq!(arr[2]["identifier"], "c");
    }

    #[test]
    fn merge_paginated_overrides_creates_array_when_missing() {
        let mut base = serde_json::json!({"feature_states": []});
        let page = serde_json::json!({"identity_overrides": [{"identifier": "a"}]});
        merge_paginated_overrides(&mut base, page);
        assert_eq!(base["identity_overrides"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn merge_paginated_overrides_noop_when_page_has_none() {
        let mut base = serde_json::json!({"identity_overrides": [{"identifier": "a"}]});
        let page = serde_json::json!({"identity_overrides": []});
        merge_paginated_overrides(&mut base, page);
        assert_eq!(base["identity_overrides"].as_array().unwrap().len(), 1);
    }
}
