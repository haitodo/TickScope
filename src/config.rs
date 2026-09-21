//! Configuration loader and validator.

use crate::contracts::config::AppConfig;
use std::fs;
use std::path::Path;

pub fn load_config_from_file<P: AsRef<Path>>(path: P) -> Result<AppConfig, String> {
    let content = fs::read_to_string(path.as_ref())
        .map_err(|e| format!("Failed to read config file '{}': {}", path.as_ref().display(), e))?;
    load_config_from_str(&content)
}

pub fn load_config_from_str(s: &str) -> Result<AppConfig, String> {
    let config: AppConfig = toml::from_str(s)
        .map_err(|e| format!("Failed to parse TOML config: {}", e))?;
    config.validate()?;
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_valid() {
        let cfg = AppConfig::default();
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn test_duplicate_broker_id_rejected() {
        let mut cfg = AppConfig::default();
        cfg.brokers[1].id = cfg.brokers[0].id;
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn test_duplicate_broker_port_rejected() {
        let mut cfg = AppConfig::default();
        cfg.brokers[1].port = cfg.brokers[0].port;
        assert!(cfg.validate().is_err());
    }
}
