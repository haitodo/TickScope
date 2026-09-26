//! Command-line argument parsing for TickScope.

use log::LevelFilter;
use std::path::PathBuf;

/// Parsed command-line arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliArgs {
    /// Whether to attach/allocate a console window and enable log output.
    pub console: bool,
    /// Optional explicit path to a configuration file.
    pub config_path: Option<PathBuf>,
    /// Optional log level filter specified via CLI.
    pub log_level: Option<LevelFilter>,
    /// Whether to display help and exit.
    pub show_help: bool,
    /// Whether to display version and exit.
    pub show_version: bool,
}

impl Default for CliArgs {
    fn default() -> Self {
        Self {
            console: false,
            config_path: None,
            log_level: None,
            show_help: false,
            show_version: false,
        }
    }
}

impl CliArgs {
    /// Parse arguments from an iterator of strings (excluding the executable name).
    pub fn parse<I, T>(args: I) -> Result<Self, String>
    where
        I: IntoIterator<Item = T>,
        T: Into<String>,
    {
        let mut cli = CliArgs::default();
        let mut iter = args.into_iter().map(Into::into).peekable();

        while let Some(arg) = iter.next() {
            if arg == "--" {
                // Positional arguments only after `--` delimiter
                for pos in iter {
                    Self::set_config_path(&mut cli, PathBuf::from(pos))?;
                }
                break;
            }

            match arg.as_str() {
                "-c" | "--console" => {
                    cli.console = true;
                }
                "-h" | "--help" => {
                    cli.show_help = true;
                }
                "-V" | "--version" => {
                    cli.show_version = true;
                }
                "-l" | "--log-level" => {
                    let val = iter.next().ok_or_else(|| {
                        format!("Option '{arg}' requires a log level argument (error, warn, info, debug, trace)")
                    })?;
                    cli.log_level = Some(parse_level_filter(&val)?);
                }
                opt if opt.starts_with("--log-level=") => {
                    let val = &opt["--log-level=".len()..];
                    cli.log_level = Some(parse_level_filter(val)?);
                }
                "--config" => {
                    let path = iter.next().ok_or_else(|| {
                        "Option '--config' requires a file path argument".to_string()
                    })?;
                    if path.trim().is_empty() {
                        return Err("Option '--config' requires a non-empty file path".to_string());
                    }
                    Self::set_config_path(&mut cli, PathBuf::from(path))?;
                }
                opt if opt.starts_with("--config=") => {
                    let path = &opt["--config=".len()..];
                    if path.trim().is_empty() {
                        return Err("Option '--config=' requires a non-empty file path".to_string());
                    }
                    Self::set_config_path(&mut cli, PathBuf::from(path))?;
                }
                opt if opt.starts_with('-') => {
                    return Err(format!("Unknown option '{opt}'. Use --help for usage information."));
                }
                positional => {
                    Self::set_config_path(&mut cli, PathBuf::from(positional))?;
                }
            }
        }

        Ok(cli)
    }

    fn set_config_path(cli: &mut CliArgs, path: PathBuf) -> Result<(), String> {
        if path.as_os_str().is_empty() || path.to_string_lossy().trim().is_empty() {
            return Err("Configuration file path cannot be empty".to_string());
        }
        if let Some(existing) = &cli.config_path {
            return Err(format!(
                "Multiple configuration files specified: '{}' and '{}'",
                existing.display(),
                path.display()
            ));
        }
        cli.config_path = Some(path);
        Ok(())
    }

    /// Help text displayed for `--help`.
    pub fn help_text() -> &'static str {
        concat!(
            "TickScope - Multi-Broker Real-time FX Tick Comparison\n\n",
            "USAGE:\n",
            "    tick-scope [OPTIONS] [CONFIG_PATH]\n\n",
            "ARGS:\n",
            "    <CONFIG_PATH>           Path to custom TOML configuration file (optional)\n\n",
            "OPTIONS:\n",
            "    -c, --console           Attach or allocate a console window and enable log output\n",
            "    -l, --log-level <LVL>   Set log level (error, warn, info, debug, trace) [default: info]\n",
            "        --config <PATH>     Alternative way to specify custom configuration file path\n",
            "    -V, --version           Print version information and exit\n",
            "    -h, --help              Print this help information and exit\n"
        )
    }
}

/// Parse a log level string into a `LevelFilter`.
pub fn parse_level_filter(s: &str) -> Result<LevelFilter, String> {
    match s.trim().to_ascii_lowercase().as_str() {
        "off" => Ok(LevelFilter::Off),
        "error" | "err" => Ok(LevelFilter::Error),
        "warn" | "warning" => Ok(LevelFilter::Warn),
        "info" => Ok(LevelFilter::Info),
        "debug" => Ok(LevelFilter::Debug),
        "trace" => Ok(LevelFilter::Trace),
        other => Err(format!(
            "Invalid log level '{other}'. Valid values are: error, warn, info, debug, trace"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_empty_args() {
        let empty: Vec<&str> = vec![];
        let cli = CliArgs::parse(empty).unwrap();
        assert!(!cli.console);
        assert_eq!(cli.config_path, None);
        assert_eq!(cli.log_level, None);
        assert!(!cli.show_help);
    }

    #[test]
    fn test_console_flags() {
        let cli = CliArgs::parse(["-c"]).unwrap();
        assert!(cli.console);

        let cli = CliArgs::parse(["--console"]).unwrap();
        assert!(cli.console);
    }

    #[test]
    fn test_help_flags() {
        let cli = CliArgs::parse(["-h"]).unwrap();
        assert!(cli.show_help);

        let cli = CliArgs::parse(["--help"]).unwrap();
        assert!(cli.show_help);
    }

    #[test]
    fn test_positional_config_path() {
        let cli = CliArgs::parse(["config/custom.toml"]).unwrap();
        assert_eq!(cli.config_path, Some(PathBuf::from("config/custom.toml")));
        assert!(!cli.console);

        // Order independence: config before flag
        let cli = CliArgs::parse(["config/custom.toml", "--console"]).unwrap();
        assert!(cli.console);
        assert_eq!(cli.config_path, Some(PathBuf::from("config/custom.toml")));

        // Order independence: flag before config
        let cli = CliArgs::parse(["-c", "config/custom.toml"]).unwrap();
        assert!(cli.console);
        assert_eq!(cli.config_path, Some(PathBuf::from("config/custom.toml")));
    }

    #[test]
    fn test_named_config_option() {
        let cli = CliArgs::parse(["--config", "config/custom.toml"]).unwrap();
        assert_eq!(cli.config_path, Some(PathBuf::from("config/custom.toml")));

        let cli = CliArgs::parse(["--config=config/custom.toml"]).unwrap();
        assert_eq!(cli.config_path, Some(PathBuf::from("config/custom.toml")));

        // Combining named config with console
        let cli = CliArgs::parse(["-c", "--config", "my.toml"]).unwrap();
        assert!(cli.console);
        assert_eq!(cli.config_path, Some(PathBuf::from("my.toml")));
    }

    #[test]
    fn test_log_level_flags() {
        let cli = CliArgs::parse(["-l", "debug"]).unwrap();
        assert_eq!(cli.log_level, Some(LevelFilter::Debug));

        let cli = CliArgs::parse(["--log-level", "warn"]).unwrap();
        assert_eq!(cli.log_level, Some(LevelFilter::Warn));

        let cli = CliArgs::parse(["--log-level=trace"]).unwrap();
        assert_eq!(cli.log_level, Some(LevelFilter::Trace));

        let cli = CliArgs::parse(["-l", "error", "-c"]).unwrap();
        assert!(cli.console);
        assert_eq!(cli.log_level, Some(LevelFilter::Error));
    }

    #[test]
    fn test_duplicate_config_error() {
        let err = CliArgs::parse(["foo.toml", "bar.toml"]).unwrap_err();
        assert!(err.contains("Multiple configuration files specified"));

        let err = CliArgs::parse(["--config", "foo.toml", "bar.toml"]).unwrap_err();
        assert!(err.contains("Multiple configuration files specified"));
    }

    #[test]
    fn test_unknown_option_error() {
        let err = CliArgs::parse(["--invalid-option"]).unwrap_err();
        assert!(err.contains("Unknown option '--invalid-option'"));
    }

    #[test]
    fn test_missing_argument_error() {
        let err = CliArgs::parse(["--config"]).unwrap_err();
        assert!(err.contains("requires a file path argument"));

        let err = CliArgs::parse(["--log-level"]).unwrap_err();
        assert!(err.contains("requires a log level argument"));

        let err = CliArgs::parse(["--config="]).unwrap_err();
        assert!(err.contains("requires a non-empty file path"));
    }

    #[test]
    fn test_invalid_log_level() {
        let err = CliArgs::parse(["-l", "superverbose"]).unwrap_err();
        assert!(err.contains("Invalid log level 'superverbose'"));
    }

    #[test]
    fn test_version_flags() {
        let cli = CliArgs::parse(["-V"]).unwrap();
        assert!(cli.show_version);

        let cli = CliArgs::parse(["--version"]).unwrap();
        assert!(cli.show_version);
    }

    #[test]
    fn test_option_delimiter() {
        let cli = CliArgs::parse(["-c", "--", "my_config.toml"]).unwrap();
        assert!(cli.console);
        assert_eq!(cli.config_path, Some(PathBuf::from("my_config.toml")));

        // Flags after `--` are treated as positional arguments
        let err = CliArgs::parse(["--", "first.toml", "second.toml"]).unwrap_err();
        assert!(err.contains("Multiple configuration files specified"));
    }

    #[test]
    fn test_empty_config_path_rejected() {
        let err = CliArgs::parse([""]).unwrap_err();
        assert!(err.contains("Configuration file path cannot be empty"));

        let err = CliArgs::parse(["   "]).unwrap_err();
        assert!(err.contains("Configuration file path cannot be empty"));

        let err = CliArgs::parse(["--config", ""]).unwrap_err();
        assert!(err.contains("requires a non-empty file path"));

        let err = CliArgs::parse(["--config", "   "]).unwrap_err();
        assert!(err.contains("requires a non-empty file path"));
    }

    #[test]
    fn test_help_text_content() {
        let help = CliArgs::help_text();
        assert!(help.contains("--console"));
        assert!(help.contains("--log-level"));
        assert!(help.contains("--version"));
        assert!(help.contains("--help"));
    }
}
