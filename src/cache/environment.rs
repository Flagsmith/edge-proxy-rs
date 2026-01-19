use async_trait::async_trait;
use flagsmith_flag_engine::engine_eval::{EngineEvaluationContext, environment_to_context};
use flagsmith_flag_engine::environments::Environment as FlagsmithEnvironment;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::models::engine::Environment;

#[async_trait]
pub trait EnvironmentsCache: Send + Sync {
    /// Get the raw environment document (for /environment-document endpoint)
    /// Returns Arc to avoid cloning large JSON on every request
    async fn get_environment(&self, environment_key: &str) -> Option<Arc<Value>>;

    /// Get the pre-computed evaluation context (for flag evaluation)
    async fn get_context(&self, environment_key: &str) -> Option<EngineEvaluationContext>;

    /// Store environment document and compute context. Returns true if changed.
    async fn put_environment(&self, environment_key: &str, document: Value) -> bool;

    /// Get identity override data
    async fn get_identity(&self, environment_api_key: &str, identifier: &str) -> Option<Value>;
}

#[derive(Clone, Default)]
pub struct LocalMemEnvironmentsCache {
    /// Raw environment documents (for /environment-document endpoint)
    /// Stored as Arc<Value> to avoid cloning large JSON on every request
    environments: Arc<RwLock<HashMap<String, Arc<Value>>>>,
    /// Pre-computed evaluation contexts (for flag evaluation)
    contexts: Arc<RwLock<HashMap<String, EngineEvaluationContext>>>,
    /// Identity overrides extracted from environments
    identity_overrides: Arc<RwLock<HashMap<String, HashMap<String, Value>>>>,
}

impl LocalMemEnvironmentsCache {
    pub fn new() -> Self {
        Self {
            environments: Arc::new(RwLock::new(HashMap::new())),
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

    async fn get_context(&self, environment_key: &str) -> Option<EngineEvaluationContext> {
        let contexts = self.contexts.read().await;
        contexts.get(environment_key).cloned()
    }

    async fn put_environment(&self, environment_key: &str, document: Value) -> bool {
        let mut environments = self.environments.write().await;
        let mut contexts = self.contexts.write().await;
        let mut identity_overrides = self.identity_overrides.write().await;

        // Check if document changed
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
