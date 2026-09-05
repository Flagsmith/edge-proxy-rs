use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use parking_lot::RwLock;

use crate::config::settings::EnvironmentKeyPair;

/// A server-side (`ser.`) key together with the validity metadata the
/// proxy config endpoint reports. Statically configured keys carry no
/// metadata and are always valid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerKey {
    pub key: String,
    pub active: bool,
    pub expires_at: Option<DateTime<Utc>>,
}

impl ServerKey {
    pub fn is_valid(&self) -> bool {
        self.active && self.expires_at.is_none_or(|at| at > Utc::now())
    }
}

/// The key set of one environment the proxy serves: its client-side key
/// and every server-side key that can authenticate for it upstream
/// (multiple during rotation).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvironmentKeys {
    pub client_key: String,
    pub server_keys: Vec<ServerKey>,
}

impl EnvironmentKeys {
    /// The first server-side key still usable for upstream fetches.
    pub fn valid_server_key(&self) -> Option<&ServerKey> {
        self.server_keys.iter().find(|key| key.is_valid())
    }
}

/// The runtime-mutable set of environments the proxy serves.
///
/// Every environment is indexed under its client key *and* each of its
/// server keys, so a single lookup resolves whichever kind of key a request
/// presents.
///
/// Server keys are assumed unique across environments: a key duplicated in
/// the config is last-one-wins on insert, and removing either environment
/// un-indexes the shared key for both.
///
/// Uses `parking_lot::RwLock`, not tokio's: guards are held only for a map
/// operation, never across an await, and lookups stay callable from
/// synchronous code.
#[derive(Default)]
pub struct EnvironmentIndex {
    environment_keys_by_any_key: RwLock<HashMap<String, Arc<EnvironmentKeys>>>,
}

impl EnvironmentIndex {
    pub fn from_settings(pairs: &[EnvironmentKeyPair]) -> Self {
        let index = Self::default();
        for pair in pairs {
            index.insert(EnvironmentKeys {
                client_key: pair.client_side_key.clone(),
                server_keys: vec![ServerKey {
                    key: pair.server_side_key.clone(),
                    active: true,
                    expires_at: None,
                }],
            });
        }
        index
    }

    /// Resolve a presented key — client- or server-side — to its
    /// environment's keys.
    pub fn resolve(&self, key: &str) -> Option<Arc<EnvironmentKeys>> {
        self.environment_keys_by_any_key.read().get(key).cloned()
    }

    /// Insert or replace an environment's keys. Server keys the
    /// environment no longer has stop resolving.
    pub fn insert(&self, keys: EnvironmentKeys) {
        let keys = Arc::new(keys);
        let mut environment_keys_by_any_key = self.environment_keys_by_any_key.write();

        if let Some(previous) = environment_keys_by_any_key.get(&keys.client_key).cloned() {
            for server_key in &previous.server_keys {
                environment_keys_by_any_key.remove(&server_key.key);
            }
        }

        for server_key in &keys.server_keys {
            environment_keys_by_any_key.insert(server_key.key.clone(), Arc::clone(&keys));
        }
        environment_keys_by_any_key.insert(keys.client_key.clone(), keys);
    }

    /// Remove the environment `key` resolves to (any of its keys works),
    /// returning its keys so the caller can clear per-key caches.
    pub fn remove(&self, key: &str) -> Option<Arc<EnvironmentKeys>> {
        let mut environment_keys_by_any_key = self.environment_keys_by_any_key.write();
        let keys = environment_keys_by_any_key.get(key).cloned()?;

        environment_keys_by_any_key.remove(&keys.client_key);
        for server_key in &keys.server_keys {
            environment_keys_by_any_key.remove(&server_key.key);
        }

        Some(keys)
    }

    /// Point-in-time snapshot of every environment's keys, ordered by
    /// client key so callers iterate deterministically.
    pub fn snapshot(&self) -> Vec<Arc<EnvironmentKeys>> {
        let environment_keys_by_any_key = self.environment_keys_by_any_key.read();
        let mut snapshot: Vec<Arc<EnvironmentKeys>> = environment_keys_by_any_key
            .iter()
            .filter(|(key, keys)| key.as_str() == keys.client_key)
            .map(|(_, keys)| Arc::clone(keys))
            .collect();
        snapshot.sort_by(|a, b| a.client_key.cmp(&b.client_key));
        snapshot
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeDelta;

    fn pair(client: &str, server: &str) -> EnvironmentKeyPair {
        EnvironmentKeyPair {
            client_side_key: client.to_string(),
            server_side_key: server.to_string(),
        }
    }

    fn server_key(key: &str) -> ServerKey {
        ServerKey {
            key: key.to_string(),
            active: true,
            expires_at: None,
        }
    }

    #[test]
    fn from_settings_resolves_both_keys_to_the_same_environment() {
        // Given
        let index = EnvironmentIndex::from_settings(&[pair("client_a", "ser.a")]);

        // When
        let by_client = index.resolve("client_a").unwrap();
        let by_server = index.resolve("ser.a").unwrap();

        // Then
        assert!(Arc::ptr_eq(&by_client, &by_server));
        assert_eq!(by_client.client_key, "client_a");
        assert!(by_client.valid_server_key().is_some());
    }

    #[test]
    fn resolve_unknown_key_returns_none() {
        let index = EnvironmentIndex::from_settings(&[pair("client_a", "ser.a")]);
        assert!(index.resolve("nope").is_none());
    }

    #[test]
    fn insert_replaces_keys_and_drops_stale_server_key_index() {
        // Given
        let index = EnvironmentIndex::from_settings(&[pair("client_a", "ser.old")]);

        // When the environment's server key is rotated
        index.insert(EnvironmentKeys {
            client_key: "client_a".to_string(),
            server_keys: vec![server_key("ser.new")],
        });

        // Then only the new server key resolves
        assert!(index.resolve("ser.old").is_none());
        assert_eq!(index.resolve("ser.new").unwrap().client_key, "client_a");
        assert_eq!(index.snapshot().len(), 1);
    }

    #[test]
    fn remove_by_any_key_clears_every_index_entry() {
        // Given an environment with two server keys
        let index = EnvironmentIndex::default();
        index.insert(EnvironmentKeys {
            client_key: "client_a".to_string(),
            server_keys: vec![server_key("ser.one"), server_key("ser.two")],
        });

        // When removed via one of its server keys
        let removed = index.remove("ser.two").unwrap();

        // Then
        assert_eq!(removed.client_key, "client_a");
        assert!(index.resolve("client_a").is_none());
        assert!(index.resolve("ser.one").is_none());
        assert!(index.resolve("ser.two").is_none());
        assert!(index.remove("client_a").is_none());
    }

    #[test]
    fn snapshot_returns_one_entry_per_environment_sorted_by_client_key() {
        // Given
        let index = EnvironmentIndex::from_settings(&[
            pair("client_b", "ser.b"),
            pair("client_a", "ser.a"),
        ]);

        // When
        let snapshot = index.snapshot();

        // Then
        let client_keys: Vec<&str> = snapshot.iter().map(|r| r.client_key.as_str()).collect();
        assert_eq!(client_keys, vec!["client_a", "client_b"]);
    }

    #[test]
    fn valid_server_key_skips_inactive_and_expired_keys() {
        // Given
        let keys = EnvironmentKeys {
            client_key: "client_a".to_string(),
            server_keys: vec![
                ServerKey {
                    key: "ser.inactive".to_string(),
                    active: false,
                    expires_at: None,
                },
                ServerKey {
                    key: "ser.expired".to_string(),
                    active: true,
                    expires_at: Some(Utc::now() - TimeDelta::days(1)),
                },
                ServerKey {
                    key: "ser.valid".to_string(),
                    active: true,
                    expires_at: Some(Utc::now() + TimeDelta::days(1)),
                },
            ],
        };

        // When / Then
        assert_eq!(keys.valid_server_key().unwrap().key, "ser.valid");
    }

    #[test]
    fn valid_server_key_returns_none_when_no_key_is_usable() {
        let keys = EnvironmentKeys {
            client_key: "client_a".to_string(),
            server_keys: vec![ServerKey {
                key: "ser.inactive".to_string(),
                active: false,
                expires_at: None,
            }],
        };
        assert!(keys.valid_server_key().is_none());
    }
}
