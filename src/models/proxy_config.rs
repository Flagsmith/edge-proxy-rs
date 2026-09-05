use chrono::{DateTime, Utc};
use serde::Deserialize;

use crate::environments::{EnvironmentKeys, ServerKey};

/// An environment as the proxy config endpoint reports it. The response
/// carries more fields (id, name, project/organisation ids, updated_at);
/// only what the proxy acts on is declared, serde ignores the rest.
#[derive(Debug, Clone, Deserialize)]
pub struct ProxyConfigEnvironment {
    pub client_side_key: String,
    #[serde(default)]
    pub server_side_keys: Vec<ProxyConfigServerKey>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProxyConfigServerKey {
    pub key: String,
    pub active: bool,
    pub expires_at: Option<DateTime<Utc>>,
}

impl From<ProxyConfigEnvironment> for EnvironmentKeys {
    fn from(environment: ProxyConfigEnvironment) -> Self {
        let mut server_keys: Vec<ServerKey> = environment
            .server_side_keys
            .into_iter()
            .map(|server_key| ServerKey {
                key: server_key.key,
                active: server_key.active,
                expires_at: server_key.expires_at,
            })
            .collect();
        // The endpoint does not guarantee key order; sort so a reordered
        // response is not mistaken for a changed environment.
        server_keys.sort_by(|a, b| a.key.cmp(&b.key));

        Self {
            client_key: environment.client_side_key,
            server_keys,
        }
    }
}
