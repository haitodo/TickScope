//! Configuration loader and validator.

use super::schema::{AppConfig, ConfigError};
use std::fs;
use std::path::Path;

/// An explicit path always wins; portable installs need no external config.
/// # Errors
///
/// Returns [`ConfigError::PathCheck`] when a candidate path cannot be inspected, and any error
/// from [`load_config_from_file`] or [`load_config_from_str`] for the first source that exists.
pub fn load_startup_config(
    explicit: Option<&Path>,
    exe_dir: &Path,
    cwd: &Path,
) -> Result<AppConfig, ConfigError> {
    if let Some(path) = explicit {
        return load_config_from_file(path);
    }
    for path in [
        exe_dir.join("config/default.toml"),
        cwd.join("config/default.toml"),
    ] {
        let exists = path.try_exists().map_err(|e| ConfigError::PathCheck {
            path: path.clone(),
            source: e,
        })?;
        if exists {
            return load_config_from_file(path);
        }
    }
    load_config_from_str(include_str!("../../config/default.toml"))
}

/// Read and validate a configuration file.
///
/// # Errors
///
/// Returns [`ConfigError::Io`] when the file cannot be read, [`ConfigError::Parse`] when it is
/// not valid TOML, and [`ConfigError::Validation`] when the parsed values are invalid.
pub fn load_config_from_file<P: AsRef<Path>>(path: P) -> Result<AppConfig, ConfigError> {
    let p = path.as_ref();
    let content = fs::read_to_string(p).map_err(|e| ConfigError::Io {
        path: p.to_path_buf(),
        source: e,
    })?;
    load_config_from_str(&content)
}

/// Parse and validate a configuration from a TOML string.
///
/// # Errors
///
/// Returns [`ConfigError::Parse`] when `s` does not match the configuration schema, and
/// [`ConfigError::Validation`] when [`AppConfig::validate`] rejects the parsed values.
pub fn load_config_from_str(s: &str) -> Result<AppConfig, ConfigError> {
    let config: AppConfig = toml::from_str(s)?;
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
        assert_eq!(
            load_startup_config(None, &exe_dir, &cwd).unwrap(),
            load_config_from_str(include_str!("../../config/default.toml")).unwrap()
        );
        assert!(
            load_startup_config(Some(&dir.path().join("missing.toml")), &exe_dir, &cwd).is_err()
        );
        fs::create_dir_all(cwd.join("config")).unwrap();
        fs::write(
            cwd.join("config/default.toml"),
            toml::to_string(&AppConfig::default()).unwrap(),
        )
        .unwrap();
        assert_eq!(
            load_startup_config(None, &exe_dir, &cwd).unwrap(),
            AppConfig::default()
        );
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
