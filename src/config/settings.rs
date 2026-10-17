use crate::error::{EdgeProxyError, Result};
use serde::de::{self, DeserializeOwned};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};
use std::fs;
use std::io;
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
    // Optional so the environment set can also come from runtime discovery
    #[serde(default, deserialize_with = "json_str_or_native")]
    #[validate(nested)]
    pub environment_key_pairs: Vec<EnvironmentKeyPair>,
    /// Credential for the proxy config endpoint; when set, the served
    /// environments are kept in sync with it at every poll.
    #[serde(default)]
    #[validate(length(min = 1))]
    pub proxy_key: Option<String>,
    #[serde(default = "default_api_url")]
    pub api_url: String,
    #[serde(
        default = "default_api_poll_frequency",
        deserialize_with = "json_str_or_native"
    )]
    pub api_poll_frequency_seconds: u64,
    #[serde(
        default = "default_api_poll_timeout",
        deserialize_with = "json_str_or_native"
    )]
    pub api_poll_timeout_seconds: u64,
    #[serde(
        default = "default_allow_origins",
        deserialize_with = "json_str_or_native"
    )]
    pub allow_origins: Vec<String>,
    #[serde(default, deserialize_with = "json_str_or_native")]
    pub server: ServerSettings,
    #[serde(default, deserialize_with = "json_str_or_native")]
    pub logging: LoggingSettings,
    #[serde(default, deserialize_with = "json_str_or_native")]
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

fn default_allow_origins() -> Vec<String> {
    vec!["*".to_string()]
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            environment_key_pairs: vec![],
            proxy_key: None,
            api_url: default_api_url(),
            api_poll_frequency_seconds: default_api_poll_frequency(),
            api_poll_timeout_seconds: default_api_poll_timeout(),
            allow_origins: default_allow_origins(),
            server: ServerSettings::default(),
            logging: LoggingSettings::default(),
            health_check: HealthCheckSettings::default(),
        }
    }
}

impl AppSettings {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_file(path: &PathBuf) -> Result<Self> {
        Self::from_value(read_config_file(path)?)
    }

    pub fn from_value(value: Value) -> Result<Self> {
        let settings: AppSettings = serde_path_to_error::deserialize(value).map_err(|e| {
            EdgeProxyError::ConfigurationError(format!(
                "Failed to parse config at {}: {}",
                e.path(),
                e.inner()
            ))
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

fn read_config_file(path: &PathBuf) -> Result<Value> {
    let config_str = match fs::read_to_string(path) {
        Ok(config_str) => config_str,
        // Settings may come entirely from environment variables
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Value::Object(Map::new())),
        Err(e) => {
            return Err(EdgeProxyError::ConfigurationError(format!(
                "Failed to read config file: {}",
                e
            )));
        }
    };

    serde_json::from_str(&config_str).map_err(|e| {
        EdgeProxyError::ConfigurationError(format!("Failed to parse config file: {}", e))
    })
}

fn json_str_or_native<'de, D, T>(deserializer: D) -> std::result::Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: DeserializeOwned,
{
    match Value::deserialize(deserializer)? {
        Value::String(json) => serde_json::from_str(&json).map_err(de::Error::custom),
        value => T::deserialize(value).map_err(de::Error::custom),
    }
}

fn apply_env_overrides(
    mut config: Value,
    env: impl IntoIterator<Item = (String, String)>,
) -> Result<Value> {
    let defaults = serde_json::to_value(AppSettings::default())?;
    let Value::Object(fields) = &mut config else {
        return Err(EdgeProxyError::ConfigurationError(
            "Config file must contain a JSON object".to_string(),
        ));
    };

    for (name, raw) in env {
        let key = name.to_lowercase();
        if defaults.get(&key).is_none() {
            continue;
        }
        fields.insert(key, Value::String(raw));
    }

    Ok(config)
}

pub fn get_settings() -> Result<AppSettings> {
    let config_path = std::env::var("CONFIG_PATH").unwrap_or_else(|_| "./config.json".to_string());
    let path = PathBuf::from(config_path);

    // vars() panics on non-UTF-8 entries, even ones unrelated to the proxy
    let env = std::env::vars_os()
        .filter_map(|(name, value)| Some((name.into_string().ok()?, value.into_string().ok()?)));
    let config = apply_env_overrides(read_config_file(&path)?, env)?;
    AppSettings::from_value(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_with_empty_proxy_key_is_invalid() {
        // Given an empty proxy_key, which would silently fail every sync
        let settings: AppSettings = serde_json::from_str(r#"{"proxy_key": ""}"#).unwrap();

        // Then
        assert!(settings.validate().is_err());
    }

    #[test]
    fn test_config_with_only_a_proxy_key_is_valid() {
        // Given a config file that relies entirely on the proxy config
        let settings: AppSettings = serde_json::from_str(r#"{"proxy_key": "pk.secret"}"#).unwrap();

        // Then
        assert_eq!(settings.proxy_key.as_deref(), Some("pk.secret"));
        assert!(settings.environment_key_pairs.is_empty());
        assert!(settings.validate().is_ok());
    }

    #[test]
    fn test_config_without_environment_key_pairs_parses_to_empty_valid_set() {
        // Given a config file omitting environment_key_pairs entirely
        let settings: AppSettings = serde_json::from_str("{}").unwrap();

        // Then it behaves like an explicitly empty list and validates
        assert!(settings.environment_key_pairs.is_empty());
        assert!(settings.validate().is_ok());
    }

    #[test]
    fn test_missing_config_file_loads_defaults() {
        // Given
        let path = PathBuf::from("/nonexistent/config.json");

        // When
        let settings = AppSettings::from_file(&path).unwrap();

        // Then
        assert_eq!(settings.api_url, default_api_url());
        assert!(settings.environment_key_pairs.is_empty());
    }

    fn env(vars: &[(&str, &str)]) -> Vec<(String, String)> {
        vars.iter()
            .map(|(name, value)| (name.to_string(), value.to_string()))
            .collect()
    }

    fn settings_from(config: Value, vars: &[(&str, &str)]) -> Result<AppSettings> {
        AppSettings::from_value(apply_env_overrides(config, env(vars))?)
    }

    #[test]
    fn test_env_only_config_loads_environment_key_pairs() {
        // Given
        let vars = [(
            "ENVIRONMENT_KEY_PAIRS",
            r#"[{"server_side_key":"ser.abc","client_side_key":"abc"}]"#,
        )];

        // When
        let settings = settings_from(serde_json::json!({}), &vars).unwrap();

        // Then
        assert_eq!(settings.environment_key_pairs.len(), 1);
        assert_eq!(settings.environment_key_pairs[0].client_side_key, "abc");
    }

    #[test]
    fn test_env_takes_precedence_over_config_file() {
        // Given
        let config = serde_json::json!({
            "api_url": "https://file.example.com/api/v1",
            "api_poll_frequency_seconds": 30,
        });
        let vars = [
            ("API_URL", "https://env.example.com/api/v1"),
            ("API_POLL_FREQUENCY_SECONDS", "5"),
        ];

        // When
        let settings = settings_from(config, &vars).unwrap();

        // Then
        assert_eq!(settings.api_url, "https://env.example.com/api/v1");
        assert_eq!(settings.api_poll_frequency_seconds, 5);
    }

    #[test]
    fn test_env_raw_field_keeps_numeric_looking_value_as_string() {
        // Given
        let vars = [("PROXY_KEY", "123")];

        // When
        let settings = settings_from(serde_json::json!({}), &vars).unwrap();

        // Then
        assert_eq!(settings.proxy_key.as_deref(), Some("123"));
    }

    #[test]
    fn test_env_names_are_case_insensitive() {
        // Given
        let vars = [("api_poll_timeout_seconds", "9")];

        // When
        let settings = settings_from(serde_json::json!({}), &vars).unwrap();

        // Then
        assert_eq!(settings.api_poll_timeout_seconds, 9);
    }

    #[test]
    fn test_env_object_replaces_config_file_object() {
        // Given
        let config = serde_json::json!({
            "logging": {"log_level": "INFO", "log_format": "json"},
        });
        let vars = [("LOGGING", r#"{"log_level":"DEBUG"}"#)];

        // When
        let settings = settings_from(config, &vars).unwrap();

        // Then
        assert_eq!(settings.logging.log_level, "DEBUG");
        assert_eq!(settings.logging.log_format, default_log_format());
    }

    #[test]
    fn test_env_string_field_starting_with_brace_stays_a_string() {
        // Given
        let vars = [("PROXY_KEY", "{abc")];

        // When
        let settings = settings_from(serde_json::json!({}), &vars).unwrap();

        // Then
        assert_eq!(settings.proxy_key.as_deref(), Some("{abc"));
    }

    #[test]
    fn test_env_invalid_json_errors_naming_the_field() {
        // Given
        let vars = [("ENVIRONMENT_KEY_PAIRS", "[{oops")];

        // When
        let result = settings_from(serde_json::json!({}), &vars);

        // Then
        let error = result.unwrap_err().to_string();
        assert!(error.contains("environment_key_pairs"), "{error}");
    }

    #[test]
    fn test_config_file_numbers_still_parse() {
        // Given
        let config = serde_json::json!({"api_poll_timeout_seconds": 7});

        // When
        let settings = settings_from(config, &[]).unwrap();

        // Then
        assert_eq!(settings.api_poll_timeout_seconds, 7);
    }

    #[test]
    fn test_env_non_numeric_value_for_number_field_is_rejected() {
        // Given
        let vars = [("API_POLL_FREQUENCY_SECONDS", "soon")];

        // When
        let result = settings_from(serde_json::json!({}), &vars);

        // Then
        assert!(result.is_err());
    }

    #[test]
    fn test_env_unrelated_variables_are_ignored() {
        // Given
        let vars = [("PATH", "/usr/bin"), ("HOME", "not json")];

        // When
        let config = apply_env_overrides(serde_json::json!({}), env(&vars)).unwrap();

        // Then
        assert_eq!(config, serde_json::json!({}));
    }

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
