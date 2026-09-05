use std::collections::{HashMap, HashSet};
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

/// What a `sync_to` call did, for logging and cache invalidation.
#[derive(Debug, Default)]
pub struct SyncResult {
    /// Environments inserted or updated.
    pub changed: usize,
    /// Environments no longer in the config.
    pub removed: Vec<Arc<EnvironmentKeys>>,
}

/// The runtime-mutable set of environments the proxy serves.
///
/// Uses `parking_lot::RwLock`, not tokio's: guards are held only for a map
/// operation, never across an await, and lookups stay callable from
/// synchronous code.
#[derive(Default)]
pub struct EnvironmentIndex {
    /// One entry per key an environment owns, client and server alike, all
    /// pointing at the same record, so a lookup takes whichever key a
    /// request presents.
    environment_keys_by_any_key: RwLock<HashMap<String, Arc<EnvironmentKeys>>>,
    /// Every key of the statically configured environments. Immutable
    /// after construction; `sync_to` never overrides or removes an
    /// environment whose keys appear here.
    protected: HashSet<String>,
}

impl EnvironmentIndex {
    pub fn from_settings(pairs: &[EnvironmentKeyPair]) -> Self {
        let mut index = Self::default();
        for pair in pairs {
            index.protected.insert(pair.client_side_key.clone());
            index.protected.insert(pair.server_side_key.clone());
        }
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
    /// environment's keys. A server-side key resolves only while it is
    /// valid, so a deactivation delivered by the proxy config and an
    /// expiry passing between polls both take effect on the next request.
    pub fn resolve(&self, key: &str) -> Option<Arc<EnvironmentKeys>> {
        let keys = self.environment_keys_by_any_key.read().get(key).cloned()?;

        if key != keys.client_key {
            let presented = keys
                .server_keys
                .iter()
                .find(|server_key| server_key.key == key)?;
            if !presented.is_valid() {
                return None;
            }
        }

        Some(keys)
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

    /// Bring the index in line with `desired` (what the proxy config
    /// reports). Statically configured environments are never overridden
    /// or removed, and a desired environment whose keys collide with a
    /// static environment's keys is skipped entirely. Safe against
    /// concurrent readers; assumes a single writer — the poll task is the
    /// sole caller — so per-operation locking suffices. The caller clears
    /// the document cache for everything in `removed`.
    pub fn sync_to(&self, desired: Vec<EnvironmentKeys>) -> SyncResult {
        let mut result = SyncResult::default();

        let desired_clients: HashSet<String> =
            desired.iter().map(|keys| keys.client_key.clone()).collect();

        for keys in desired {
            if self.is_protected(&keys) {
                continue;
            }
            let unchanged = self
                .resolve(&keys.client_key)
                .is_some_and(|current| *current == keys);
            if unchanged {
                continue;
            }
            result.changed += 1;
            self.insert(keys);
        }

        for current in self.snapshot() {
            if desired_clients.contains(&current.client_key)
                || self.protected.contains(&current.client_key)
            {
                continue;
            }
            if let Some(removed) = self.remove(&current.client_key) {
                result.removed.push(removed);
            }
        }

        result
    }

    /// True when the environment is statically configured, or any of its
    /// keys collides with a static environment's key namespace.
    fn is_protected(&self, keys: &EnvironmentKeys) -> bool {
        self.protected.contains(&keys.client_key)
            || keys
                .server_keys
                .iter()
                .any(|server_key| self.protected.contains(&server_key.key))
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

    fn environment(client: &str, server: &str) -> EnvironmentKeys {
        EnvironmentKeys {
            client_key: client.to_string(),
            server_keys: vec![server_key(server)],
        }
    }

    #[test]
    fn sync_to_inserts_new_and_removes_absent_environments() {
        // Given an index serving env_a while the config now says env_b
        let index = EnvironmentIndex::default();
        index.insert(environment("client_a", "ser.a"));

        // When
        let result = index.sync_to(vec![environment("client_b", "ser.b")]);

        // Then
        assert_eq!(result.changed, 1);
        assert_eq!(result.removed.len(), 1);
        assert_eq!(result.removed[0].client_key, "client_a");
        assert!(index.resolve("client_a").is_none());
        assert!(index.resolve("ser.a").is_none());
        assert!(index.resolve("client_b").is_some());
        assert!(index.resolve("ser.b").is_some());
    }

    #[test]
    fn sync_to_never_touches_protected_environments() {
        // Given a statically configured environment the config omits —
        // and also claims with different keys
        let index = EnvironmentIndex::from_settings(&[pair("client_a", "ser.a")]);

        // When the config omits it entirely
        let result = index.sync_to(vec![]);

        // Then it survives
        assert_eq!(result.removed.len(), 0);
        assert!(index.resolve("ser.a").is_some());

        // When the config claims it with a different server key
        let result = index.sync_to(vec![environment("client_a", "ser.other")]);

        // Then the static pairing wins
        assert_eq!(result.changed, 0);
        assert!(index.resolve("ser.a").is_some());
        assert!(index.resolve("ser.other").is_none());

        // When the config claims a different environment reusing the
        // static environment's server key
        let result = index.sync_to(vec![EnvironmentKeys {
            client_key: "client_b".to_string(),
            server_keys: vec![server_key("ser.a")],
        }]);

        // Then it is skipped entirely rather than hijacking the key
        assert_eq!(result.changed, 0);
        assert!(index.resolve("client_b").is_none());
        assert_eq!(index.resolve("ser.a").unwrap().client_key, "client_a");
    }

    #[test]
    fn sync_to_rotation_replaces_the_server_key() {
        // Given
        let index = EnvironmentIndex::default();
        index.insert(environment("client_a", "ser.old"));

        // When the config rotates the server key
        let result = index.sync_to(vec![environment("client_a", "ser.new")]);

        // Then only the new server key resolves
        assert_eq!(result.changed, 1);
        assert!(index.resolve("ser.old").is_none());
        assert!(index.resolve("ser.new").is_some());
    }

    #[test]
    fn sync_to_leaves_unchanged_environments_alone() {
        // Given
        let index = EnvironmentIndex::default();
        index.insert(environment("client_a", "ser.a"));
        let before = index.resolve("client_a").unwrap();

        // When the config reports the same keys
        let result = index.sync_to(vec![environment("client_a", "ser.a")]);

        // Then nothing changed — not even the Arc identity
        assert_eq!(result.changed, 0);
        assert!(result.removed.is_empty());
        assert!(Arc::ptr_eq(&before, &index.resolve("client_a").unwrap()));
    }

    #[test]
    fn resolve_rejects_invalid_server_keys_but_keeps_the_client_key() {
        // Given an environment whose server keys are deactivated or expired
        let index = EnvironmentIndex::default();
        index.insert(EnvironmentKeys {
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
            ],
        });

        // Then the invalid keys stop authenticating, the client key doesn't
        assert!(index.resolve("client_a").is_some());
        assert!(index.resolve("ser.inactive").is_none());
        assert!(index.resolve("ser.expired").is_none());
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
