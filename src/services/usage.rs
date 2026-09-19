use crate::config::settings::AppSettings;
use crate::usage::{Resource, UsageBatch, UsageCounts};
use parking_lot::Mutex;
use reqwest::Client;
use std::sync::Arc;
use std::time::Duration;
use tracing::error;

enum Outcome {
    Accepted,
    Rejected,
    Failed,
}

pub struct UsageProcessor {
    counts: UsageCounts,
    pending: Mutex<Vec<UsageBatch>>,
    client: Client,
    api_url: String,
    proxy_key: Option<String>,
    flush_interval: Duration,
}

impl UsageProcessor {
    /// Mirrors MAX_USAGE_ROWS on the usage endpoint.
    const MAX_ROWS_PER_FLUSH: usize = 1000;

    pub fn new(settings: &AppSettings) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(settings.api_poll_timeout_seconds))
            .build()
            .expect("Failed to create HTTP client");
        Self {
            counts: UsageCounts::default(),
            pending: Mutex::default(),
            client,
            api_url: settings.api_url.clone(),
            proxy_key: settings.proxy_key.clone(),
            flush_interval: Duration::from_secs(settings.usage_flush_interval_seconds),
        }
    }

    pub fn track(&self, client_key: &str, resource: Resource) {
        if self.proxy_key.is_none() {
            return;
        }
        self.counts.increment(client_key, resource);
    }

    /// Returns false when any batch was not accepted.
    pub async fn flush(&self) -> bool {
        let Some(proxy_key) = &self.proxy_key else {
            return true;
        };

        let mut batches = std::mem::take(&mut *self.pending.lock());
        if batches.is_empty() {
            let mut rows = self.counts.drain();
            while !rows.is_empty() {
                let chunk = rows.drain(..rows.len().min(Self::MAX_ROWS_PER_FLUSH));
                batches.push(UsageBatch::new(chunk.collect()));
            }
        }

        let mut all_success = true;
        for batch in batches {
            match self.post(proxy_key, &batch).await {
                Outcome::Accepted => {}
                Outcome::Rejected => all_success = false,
                Outcome::Failed => {
                    self.pending.lock().push(batch);
                    all_success = false;
                }
            }
        }
        all_success
    }

    async fn post(&self, proxy_key: &str, batch: &UsageBatch) -> Outcome {
        let url = format!("{}/proxy/usage/", self.api_url);
        let result = self
            .client
            .post(&url)
            .header("X-Proxy-Key", proxy_key)
            .header("Idempotency-Key", &batch.id)
            .json(&batch.rows)
            .send()
            .await;
        match result {
            Ok(response) if response.status().is_success() => Outcome::Accepted,
            // Retrying cannot heal a rejection: drop the batch rather than
            // resend it forever.
            Ok(response) if response.status().is_client_error() => {
                error!(
                    "Usage report rejected with {}: dropping {} rows",
                    response.status(),
                    batch.rows.len()
                );
                Outcome::Rejected
            }
            Ok(response) => {
                error!("Failed to report usage: {}", response.status());
                Outcome::Failed
            }
            Err(e) => {
                error!("Failed to report usage: {}", e);
                Outcome::Failed
            }
        }
    }

    pub async fn flush_periodically(self: Arc<Self>) {
        if self.proxy_key.is_none() {
            return;
        }
        let mut interval = tokio::time::interval(self.flush_interval);
        // A flush that overruns the interval (core slow or down) must not be
        // followed by a burst of catch-up flushes hammering it further.
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        // The first tick completes immediately, before anything is counted.
        interval.tick().await;

        loop {
            interval.tick().await;
            self.flush().await;
        }
    }
}
