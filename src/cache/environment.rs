use async_trait::async_trait;
use bytes::Bytes;
use flagsmith_flag_engine::engine_eval::{EngineEvaluationContext, environment_to_context};
use flagsmith_flag_engine::environments::Environment as FlagsmithEnvironment;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::error;

use crate::models::engine::Environment;

#[async_trait]
pub trait EnvironmentsCache: Send + Sync {
    /// Get the raw environment document (for flag evaluation paths that
    /// need the parsed JSON tree). Returns Arc to avoid cloning.
    async fn get_environment(&self, environment_key: &str) -> Option<Arc<Value>>;

    /// Get the pre-serialized JSON bytes for the /environment-document
    /// endpoint. These are produced once when the document is stored, so
    /// the request path is a refcount bump.
    async fn get_environment_bytes(&self, environment_key: &str) -> Option<Bytes>;

    /// Get the pre-computed evaluation context (for flag evaluation)
    async fn get_context(&self, environment_key: &str) -> Option<EngineEvaluationContext>;

    /// Store environment document and compute context. Returns true if changed.
    async fn put_environment(&self, environment_key: &str, document: Value) -> bool;

    /// Get identity override data
    async fn get_identity(&self, environment_api_key: &str, identifier: &str) -> Option<Value>;
}

#[derive(Clone, Default)]
pub struct LocalMemEnvironmentsCache {
    /// Raw environment documents (kept parsed for evaluation paths).
    environments: Arc<RwLock<HashMap<String, Arc<Value>>>>,
    /// Pre-serialized JSON bytes for `/environment-document` responses.
    /// Populated on `put_environment`; cheap to clone (refcounted).
    environment_bytes: Arc<RwLock<HashMap<String, Bytes>>>,
    /// Pre-computed evaluation contexts (for flag evaluation)
    contexts: Arc<RwLock<HashMap<String, EngineEvaluationContext>>>,
    /// Identity overrides extracted from environments
    identity_overrides: Arc<RwLock<HashMap<String, HashMap<String, Value>>>>,
}

impl LocalMemEnvironmentsCache {
    pub fn new() -> Self {
        Self {
            environments: Arc::new(RwLock::new(HashMap::new())),
            environment_bytes: Arc::new(RwLock::new(HashMap::new())),
            contexts: Arc::new(RwLock::new(HashMap::new())),
            identity_overrides: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

#[async_trait]
impl EnvironmentsCache for LocalMemEnvironmentsCache {
    async fn get_environment(&self, environment_key: &str) -> Option<Arc<Value>> {
        let environments = self.environments.read().await;
        environments.get(environment_key).cloned() // Clones Arc (cheap), not Value
    }

    async fn get_environment_bytes(&self, environment_key: &str) -> Option<Bytes> {
        let environment_bytes = self.environment_bytes.read().await;
        environment_bytes.get(environment_key).cloned() // Bytes clone is a refcount bump
    }

    async fn get_context(&self, environment_key: &str) -> Option<EngineEvaluationContext> {
        let contexts = self.contexts.read().await;
        contexts.get(environment_key).cloned()
    }

    async fn put_environment(&self, environment_key: &str, document: Value) -> bool {
        // Skip work entirely when the document is unchanged. A short-lived
        // read guard on `environments` is enough — the equality check itself
        // can be expensive on multi-MB Values, but it doesn't block readers.
        {
            let environments = self.environments.read().await;
            if let Some(existing) = environments.get(environment_key) {
                if existing.as_ref() == &document {
                    return false;
                }
            }
        }

        // Heavy CPU work runs while `document` is still uniquely owned —
        // outside any lock — so concurrent flag-evaluation requests aren't
        // blocked while we serialize a multi-MB document.
        let bytes_result = serde_json::to_vec(&document);

        let mut environments = self.environments.write().await;
        let mut environment_bytes = self.environment_bytes.write().await;
        let mut contexts = self.contexts.write().await;
        let mut identity_overrides = self.identity_overrides.write().await;

        // Re-check under the write guards in case a concurrent put landed
        // an identical document between the read and write guards.
        let changed = environments
            .get(environment_key)
            .map(|existing| existing.as_ref() != &document)
            .unwrap_or(true);

        if changed {
            // Extract identity overrides
            if let Some(overrides_array) = document
                .get("identity_overrides")
                .and_then(|v| v.as_array())
            {
                let mut env_identities = HashMap::new();
                for override_obj in overrides_array {
                    if let Some(identifier) =
                        override_obj.get("identifier").and_then(|v| v.as_str())
                    {
                        env_identities.insert(identifier.to_string(), override_obj.clone());
                    }
                }
                identity_overrides.insert(environment_key.to_string(), env_identities);
            }

            // Pre-compute the evaluation context
            if let Ok(environment) = serde_json::from_value::<Environment>(document.clone()) {
                let flagsmith_env: FlagsmithEnvironment = environment.to_flagsmith_environment();
                let context = environment_to_context(flagsmith_env);
                contexts.insert(environment_key.to_string(), context);
            }

            // Pre-serialized so /environment-document requests are a refcount bump.
            // Failure leaves the byte cache empty for this key — handler returns 503.
            match bytes_result {
                Ok(bytes) => {
                    environment_bytes.insert(environment_key.to_string(), Bytes::from(bytes));
                }
                Err(err) => {
                    error!(
                        environment_key,
                        error = %err,
                        "failed to serialize environment document for byte cache"
                    );
                    environment_bytes.remove(environment_key);
                }
            }

            environments.insert(environment_key.to_string(), Arc::new(document));
        }

        changed
    }

    async fn get_identity(&self, environment_api_key: &str, identifier: &str) -> Option<Value> {
        let identity_overrides = self.identity_overrides.read().await;
        identity_overrides
            .get(environment_api_key)
            .and_then(|identities| identities.get(identifier).cloned())
    }
}
