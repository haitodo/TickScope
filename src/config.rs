//! Configuration loader and validator.

use crate::contracts::config::AppConfig;
use std::fs;
use std::path::Path;

/// An explicit path always wins; portable installs need no external config.
pub fn load_startup_config(explicit: Option<&Path>, exe_dir: &Path, cwd: &Path) -> Result<AppConfig, String> {
    if let Some(path) = explicit {
        return load_config_from_file(path);
    }
    for path in [exe_dir.join("config/default.toml"), cwd.join("config/default.toml")] {
        if path.try_exists().map_err(|e| format!("{}: {}", path.display(), e))? {
            return load_config_from_file(path);
        }
    }
    load_config_from_str(include_str!("../config/default.toml"))
}

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
    fn portable_startup_and_config_precedence() {
        let dir = tempfile::tempdir().unwrap();
        let exe_dir = dir.path().join("app");
        let cwd = dir.path().join("cwd");
        assert_eq!(load_startup_config(None, &exe_dir, &cwd).unwrap(),
            load_config_from_str(include_str!("../config/default.toml")).unwrap());
        assert!(load_startup_config(Some(&dir.path().join("missing.toml")), &exe_dir, &cwd).is_err());
        fs::create_dir_all(cwd.join("config")).unwrap();
        fs::write(cwd.join("config/default.toml"), toml::to_string(&AppConfig::default()).unwrap()).unwrap();
        assert_eq!(load_startup_config(None, &exe_dir, &cwd).unwrap(), AppConfig::default());
        fs::create_dir_all(exe_dir.join("config")).unwrap();
        fs::write(exe_dir.join("config/default.toml"), "invalid config").unwrap();
        // A broken user config must not silently fall back to embedded settings.
        assert!(load_startup_config(None, &exe_dir, &cwd).is_err());
    }

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
        cfg.mt5.auto_deploy = false;
        cfg.brokers[1].port = cfg.brokers[0].port;
        assert!(cfg.validate().is_err());
    }
}
