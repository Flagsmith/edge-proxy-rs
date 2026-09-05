use std::collections::HashMap;

use parking_lot::Mutex;
use serde::Serialize;

/// An SDK endpoint as the usage endpoint names it.
#[derive(Serialize, Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[serde(rename_all = "kebab-case")]
pub enum Resource {
    Flags,
    Identities,
    EnvironmentDocument,
}

/// One row of the `POST /proxy/usage/` body.
#[derive(Serialize, Debug, PartialEq)]
pub struct UsageRow {
    pub client_side_key: String,
    pub resource: Resource,
    pub count: u64,
}

/// Requests served since the last flush, aggregated per environment and
/// resource. Keyed by resolved client keys only, so the map is bounded by
/// the served environment set.
///
/// Uses `parking_lot::Mutex`, not tokio's: every operation writes, and
/// guards are held only for a map operation, never across an await.
#[derive(Default)]
pub struct UsageCounts {
    count_by_environment_and_resource: Mutex<HashMap<(String, Resource), u64>>,
}

impl UsageCounts {
    pub fn increment(&self, client_key: &str, resource: Resource) {
        let mut count_by_environment_and_resource = self.count_by_environment_and_resource.lock();
        *count_by_environment_and_resource
            .entry((client_key.to_string(), resource))
            .or_default() += 1;
    }

    /// Take everything counted so far, leaving the map empty.
    pub fn drain(&self) -> Vec<UsageRow> {
        let count_by_environment_and_resource =
            std::mem::take(&mut *self.count_by_environment_and_resource.lock());
        count_by_environment_and_resource
            .into_iter()
            .map(|((client_side_key, resource), count)| UsageRow {
                client_side_key,
                resource,
                count,
            })
            .collect()
    }

    /// Add drained counts back, on top of anything counted since — for
    /// when a flush fails and the rows must survive until the next one.
    pub fn merge(&self, counts: Vec<UsageRow>) {
        let mut count_by_environment_and_resource = self.count_by_environment_and_resource.lock();
        for row in counts {
            let entry = count_by_environment_and_resource
                .entry((row.client_side_key, row.resource))
                .or_default();
            *entry = entry.saturating_add(row.count);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn increment_aggregates_by_key_and_resource() {
        // Given
        let counts = UsageCounts::default();

        // When
        counts.increment("client", Resource::Flags);
        counts.increment("client", Resource::Flags);
        counts.increment("client", Resource::Identities);

        // Then
        let mut rows = counts.drain();
        rows.sort_by_key(|row| format!("{:?}", row.resource));
        assert_eq!(
            rows,
            vec![
                UsageRow {
                    client_side_key: "client".to_string(),
                    resource: Resource::Flags,
                    count: 2,
                },
                UsageRow {
                    client_side_key: "client".to_string(),
                    resource: Resource::Identities,
                    count: 1,
                },
            ]
        );
    }

    #[test]
    fn drain_leaves_nothing_behind() {
        // Given
        let counts = UsageCounts::default();
        counts.increment("client", Resource::Flags);

        // When
        counts.drain();

        // Then
        assert!(counts.drain().is_empty());
    }

    #[test]
    fn merge_adds_drained_counts_back() {
        // Given a drained batch and a request counted since
        let counts = UsageCounts::default();
        counts.increment("client", Resource::Flags);
        let drained = counts.drain();
        counts.increment("client", Resource::Flags);

        // When
        counts.merge(drained);

        // Then
        let rows = counts.drain();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].count, 2);
    }

    #[test]
    fn usage_row_serializes_to_the_contract_shape() {
        // Given
        let row = UsageRow {
            client_side_key: "client".to_string(),
            resource: Resource::EnvironmentDocument,
            count: 3,
        };

        // Then
        assert_eq!(
            serde_json::to_value(&row).unwrap(),
            serde_json::json!({
                "client_side_key": "client",
                "resource": "environment-document",
                "count": 3,
            })
        );
    }
}
