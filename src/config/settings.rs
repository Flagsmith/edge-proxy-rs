use crate::error::{EdgeProxyError, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use validator::Validate;

#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct EnvironmentKeyPair {
    #[validate(length(min = 1))]
    pub server_side_key: String,
    #[validate(length(min = 1))]
    pub client_side_key: String,
}

impl EnvironmentKeyPair {
    pub fn validate_server_key(&self) -> Result<()> {
        if !self.server_side_key.starts_with("ser.") {
            return Err(EdgeProxyError::ConfigurationError(
                "server_side_key must start with 'ser.'".to_string(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerSettings {
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
}

fn default_host() -> String {
    "0.0.0.0".to_string()
}

fn default_port() -> u16 {
    8000
}

impl Default for ServerSettings {
    fn default() -> Self {
        Self {
            host: default_host(),
            port: default_port(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingSettings {
    #[serde(default = "default_log_level")]
    pub log_level: String,
    #[serde(default = "default_log_format")]
    pub log_format: String,
    #[serde(default)]
    pub use_colors: bool,
}

fn default_log_level() -> String {
    "INFO".to_string()
}

fn default_log_format() -> String {
    "generic".to_string()
}

impl Default for LoggingSettings {
    fn default() -> Self {
        Self {
            log_level: default_log_level(),
            log_format: default_log_format(),
            use_colors: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EndpointCacheSettings {
    #[serde(default)]
    pub use_cache: bool,
    #[serde(default = "default_cache_size")]
    pub cache_max_size: usize,
}

fn default_cache_size() -> usize {
    128
}

impl Default for EndpointCacheSettings {
    fn default() -> Self {
        Self {
            use_cache: false,
            cache_max_size: default_cache_size(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EndpointCachesSettings {
    #[serde(default)]
    pub flags: EndpointCacheSettings,
    #[serde(default)]
    pub identities: EndpointCacheSettings,
    #[serde(default)]
    pub environment_document: EndpointCacheSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheckSettings {
    pub environment_update_grace_period_seconds: Option<u64>,
}

impl Default for HealthCheckSettings {
    fn default() -> Self {
        Self {
            environment_update_grace_period_seconds: Some(120),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct AppSettings {
    #[validate(nested)]
    pub environment_key_pairs: Vec<EnvironmentKeyPair>,
    #[serde(default = "default_api_url")]
    pub api_url: String,
    #[serde(default = "default_api_poll_frequency")]
    pub api_poll_frequency_seconds: u64,
    #[serde(default = "default_api_poll_timeout")]
    pub api_poll_timeout_seconds: u64,
    #[serde(default)]
    pub server: ServerSettings,
    #[serde(default)]
    pub logging: LoggingSettings,
    #[serde(default)]
    pub endpoint_caches: EndpointCachesSettings,
    #[serde(default)]
    pub health_check: HealthCheckSettings,
}

fn default_api_url() -> String {
    "https://edge.api.flagsmith.com/api/v1".to_string()
}

fn default_api_poll_frequency() -> u64 {
    60
}

fn default_api_poll_timeout() -> u64 {
    5
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            environment_key_pairs: vec![],
            api_url: default_api_url(),
            api_poll_frequency_seconds: default_api_poll_frequency(),
            api_poll_timeout_seconds: default_api_poll_timeout(),
            server: ServerSettings::default(),
            logging: LoggingSettings::default(),
            endpoint_caches: EndpointCachesSettings::default(),
            health_check: HealthCheckSettings::default(),
        }
    }
}

impl AppSettings {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_file(path: &PathBuf) -> Result<Self> {
        let contents = fs::read_to_string(path).map_err(|e| {
            EdgeProxyError::ConfigurationError(format!("Failed to read config file: {}", e))
        })?;

        let settings: AppSettings = serde_json::from_str(&contents).map_err(|e| {
            EdgeProxyError::ConfigurationError(format!("Failed to parse config file: {}", e))
        })?;

        settings.validate().map_err(|e| {
            EdgeProxyError::ConfigurationError(format!("Invalid configuration: {}", e))
        })?;

        for pair in &settings.environment_key_pairs {
            pair.validate_server_key()?;
        }

        Ok(settings)
    }
}

pub fn get_settings() -> Result<AppSettings> {
    let config_path = std::env::var("CONFIG_PATH").unwrap_or_else(|_| "./config.json".to_string());
    let path = PathBuf::from(config_path);

    AppSettings::from_file(&path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_side_key_validation_valid() {
        // Given
        let pair = EnvironmentKeyPair {
            server_side_key: "ser.456".to_string(),
            client_side_key: "abc123".to_string(),
        };

        // When
        let result = pair.validate_server_key();

        // Then
        assert!(result.is_ok());
    }

    #[test]
    fn test_client_side_key_validation_invalid() {
        // Given
        let pair = EnvironmentKeyPair {
            server_side_key: "456".to_string(),
            client_side_key: "abc123".to_string(),
        };

        // When
        let result = pair.validate_server_key();

        // Then
        assert!(result.is_err());
    }

    #[test]
    fn test_client_side_key_validation_empty_server_key() {
        // Given
        let pair = EnvironmentKeyPair {
            server_side_key: "".to_string(),
            client_side_key: "abc123".to_string(),
        };

        // When
        let result = pair.validate();

        // Then
        assert!(result.is_err());
    }

    #[test]
    fn test_client_side_key_validation_empty_client_key() {
        // Given
        let pair = EnvironmentKeyPair {
            server_side_key: "ser.456".to_string(),
            client_side_key: "".to_string(),
        };

        // When
        let result = pair.validate();

        // Then
        assert!(result.is_err());
    }
}
