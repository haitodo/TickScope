//! Command-line argument parsing for `TickScope`.

use log::LevelFilter;
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CliError {
    #[error("Configuration file path cannot be empty")]
    EmptyConfigPath,

    #[error("Multiple configuration files specified: '{first}' and '{second}'")]
    MultipleConfigFiles { first: String, second: String },

    #[error("Option '{option}' requires an argument: {message}")]
    MissingOptionValue { option: String, message: String },

    #[error("Unknown option '{0}'. Use --help for usage information.")]
    UnknownOption(String),

    #[error("Invalid log level '{level}'. Valid values are: error, warn, info, debug, trace")]
    InvalidLogLevel { level: String },

    #[error("{0}")]
    Other(String),
}

impl From<CliError> for String {
    fn from(e: CliError) -> Self {
        e.to_string()
    }
}

/// Parsed command-line arguments.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CliArgs {
    /// Whether to attach/allocate a console window and enable log output.
    pub console: bool,
    /// Whether to enable bounded, asynchronous performance diagnostics.
    pub diagnostics: bool,
    /// Whether to persist raw protocol frames for offline investigation.
    pub record_raw: bool,
    /// Optional explicit path to a configuration file.
    pub config_path: Option<PathBuf>,
    /// Optional log level filter specified via CLI.
    pub log_level: Option<LevelFilter>,
    /// Whether to display help and exit.
    pub show_help: bool,
    /// Whether to display version and exit.
    pub show_version: bool,
    /// Whether to reset window geometry to default size and position.
    pub reset_window: bool,
    /// Optional path to historical tick data directory (replay mode).
    pub tick_dir: Option<PathBuf>,
    /// Optional WebSocket URL for `TickReplay` synchronization (replay mode).
    pub ws_url: Option<String>,
    /// Optional replay currency pair / symbol (e.g. USDJPY).
    pub symbol: Option<String>,
}

impl CliArgs {
    /// Parse arguments from an iterator of strings (excluding the executable name).
    /// # Errors
    ///
    /// Returns [`CliError::EmptyConfigPath`] or [`CliError::MultipleConfigFiles`] when the
    /// configuration path is empty or given twice, [`CliError::MissingOptionValue`] when an option
    /// that takes a value is the last argument, [`CliError::UnknownOption`] for an unrecognised
    /// flag, and [`CliError::InvalidLogLevel`] for an invalid `--log-level` value.
    pub fn parse<I, T>(args: I) -> Result<Self, CliError>
    where
        I: IntoIterator<Item = T>,
        T: Into<String>,
    {
        let mut cli = Self::default();
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
                "-d" | "--diagnostics" => {
                    cli.diagnostics = true;
                }
                "-r" | "--record-raw" => {
                    cli.record_raw = true;
                }
                "-h" | "--help" => {
                    cli.show_help = true;
                }
                "-V" | "--version" => {
                    cli.show_version = true;
                }
                "--reset-window" => {
                    cli.reset_window = true;
                }
                "-t" | "--tick-dir" => {
                    let val = iter.next().ok_or_else(|| CliError::MissingOptionValue {
                        option: arg.clone(),
                        message: "requires a directory path argument".into(),
                    })?;
                    cli.tick_dir = Some(PathBuf::from(val));
                }
                opt if opt.starts_with("--tick-dir=") => {
                    let val = &opt["--tick-dir=".len()..];
                    cli.tick_dir = Some(PathBuf::from(val));
                }
                "--ws" => {
                    let val = iter.next().ok_or_else(|| CliError::MissingOptionValue {
                        option: arg.clone(),
                        message: "requires a WebSocket URL argument".into(),
                    })?;
                    cli.ws_url = Some(val);
                }
                opt if opt.starts_with("--ws=") => {
                    let val = &opt["--ws=".len()..];
                    cli.ws_url = Some(val.to_string());
                }
                "-s" | "--symbol" => {
                    let val = iter.next().ok_or_else(|| CliError::MissingOptionValue {
                        option: arg.clone(),
                        message: "requires a symbol argument (e.g. USDJPY)".into(),
                    })?;
                    cli.symbol = Some(val);
                }
                opt if opt.starts_with("--symbol=") => {
                    let val = &opt["--symbol=".len()..];
                    cli.symbol = Some(val.to_string());
                }
                "-l" | "--log-level" => {
                    let val = iter.next().ok_or_else(|| CliError::MissingOptionValue {
                        option: arg.clone(),
                        message: "requires a log level argument (error, warn, info, debug, trace)"
                            .into(),
                    })?;
                    cli.log_level = Some(parse_level_filter(&val)?);
                }
                opt if opt.starts_with("--log-level=") => {
                    let val = &opt["--log-level=".len()..];
                    cli.log_level = Some(parse_level_filter(val)?);
                }
                "--config" => {
                    let path = iter.next().ok_or_else(|| CliError::MissingOptionValue {
                        option: "--config".into(),
                        message: "requires a file path argument".into(),
                    })?;
                    if path.trim().is_empty() {
                        return Err(CliError::MissingOptionValue {
                            option: "--config".into(),
                            message: "requires a non-empty file path".into(),
                        });
                    }
                    Self::set_config_path(&mut cli, PathBuf::from(path))?;
                }
                opt if opt.starts_with("--config=") => {
                    let path = &opt["--config=".len()..];
                    if path.trim().is_empty() {
                        return Err(CliError::MissingOptionValue {
                            option: "--config=".into(),
                            message: "requires a non-empty file path".into(),
                        });
                    }
                    Self::set_config_path(&mut cli, PathBuf::from(path))?;
                }
                opt if opt.starts_with('-') => {
                    return Err(CliError::UnknownOption(opt.to_string()));
                }
                positional => {
                    Self::set_config_path(&mut cli, PathBuf::from(positional))?;
                }
            }
        }

        Ok(cli)
    }

    fn set_config_path(cli: &mut Self, path: PathBuf) -> Result<(), CliError> {
        if path.as_os_str().is_empty() || path.to_string_lossy().trim().is_empty() {
            return Err(CliError::EmptyConfigPath);
        }
        if let Some(existing) = &cli.config_path {
            return Err(CliError::MultipleConfigFiles {
                first: existing.display().to_string(),
                second: path.display().to_string(),
            });
        }
        cli.config_path = Some(path);
        Ok(())
    }

    /// Help text displayed for `--help`.
    #[must_use]
    pub const fn help_text() -> &'static str {
        concat!(
            "TickScope - Multi-Broker Real-time FX Tick Comparison\n\n",
            "USAGE:\n",
            "    tick-scope [OPTIONS] [CONFIG_PATH]\n\n",
            "ARGS:\n",
            "    <CONFIG_PATH>           Path to custom TOML configuration file (optional)\n\n",
            "OPTIONS:\n",
            "    -c, --console           Attach or allocate a console window and enable log output\n",
            "    -d, --diagnostics       Measure pipeline latency and write one-second summaries\n",
            "    -r, --record-raw        Persist raw protocol frames for offline investigation\n",
            "    -l, --log-level <LVL>   Set log level (error, warn, info, debug, trace) [default: info]\n",
            "        --config <PATH>     Alternative way to specify custom configuration file path\n",
            "        --reset-window      Reset window geometry (size 1100x750, unmaximized) to defaults\n",
            "    -t, --tick-dir <PATH>   Path to historical tick dataset directory (replay)\n",
            "        --ws <URL>          WebSocket URL for TickReplay synchronization (replay)\n",
            "    -s, --symbol <SYMBOL>   Replay currency pair / symbol (e.g. USDJPY)\n",
            "    -V, --version           Print version information and exit\n",
            "    -h, --help              Print this help information and exit\n"
        )
    }
}

/// Parse a log level string into a `LevelFilter`.
/// # Errors
///
/// Returns [`CliError::InvalidLogLevel`] when `s` is not one of `off`, `error`, `warn`, `info`,
/// `debug` or `trace`. Matching is case-insensitive, and `err` / `warning` are accepted aliases.
pub fn parse_level_filter(s: &str) -> Result<LevelFilter, CliError> {
    match s.trim().to_ascii_lowercase().as_str() {
        "off" => Ok(LevelFilter::Off),
        "error" | "err" => Ok(LevelFilter::Error),
        "warn" | "warning" => Ok(LevelFilter::Warn),
        "info" => Ok(LevelFilter::Info),
        "debug" => Ok(LevelFilter::Debug),
        "trace" => Ok(LevelFilter::Trace),
        other => Err(CliError::InvalidLogLevel {
            level: other.to_string(),
        }),
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
        assert!(!cli.reset_window);
    }

    #[test]
    fn test_reset_window_flag() {
        let cli = CliArgs::parse(["--reset-window"]).unwrap();
        assert!(cli.reset_window);
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
        assert!(err
            .to_string()
            .contains("Multiple configuration files specified"));

        let err = CliArgs::parse(["--config", "foo.toml", "bar.toml"]).unwrap_err();
        assert!(err
            .to_string()
            .contains("Multiple configuration files specified"));
    }

    #[test]
    fn test_unknown_option_error() {
        let err = CliArgs::parse(["--invalid-option"]).unwrap_err();
        assert!(err
            .to_string()
            .contains("Unknown option '--invalid-option'"));
    }

    #[test]
    fn test_missing_argument_error() {
        let err = CliArgs::parse(["--config"]).unwrap_err();
        assert!(err.to_string().contains("requires a file path argument"));

        let err = CliArgs::parse(["--log-level"]).unwrap_err();
        assert!(err.to_string().contains("requires a log level argument"));

        let err = CliArgs::parse(["--config="]).unwrap_err();
        assert!(err.to_string().contains("requires a non-empty file path"));
    }

    #[test]
    fn test_invalid_log_level() {
        let err = CliArgs::parse(["-l", "superverbose"]).unwrap_err();
        assert!(err.to_string().contains("Invalid log level 'superverbose'"));
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
        assert!(err
            .to_string()
            .contains("Multiple configuration files specified"));
    }

    #[test]
    fn test_empty_config_path_rejected() {
        let err = CliArgs::parse([""]).unwrap_err();
        assert!(err
            .to_string()
            .contains("Configuration file path cannot be empty"));

        let err = CliArgs::parse(["   "]).unwrap_err();
        assert!(err
            .to_string()
            .contains("Configuration file path cannot be empty"));

        let err = CliArgs::parse(["--config", ""]).unwrap_err();
        assert!(err.to_string().contains("requires a non-empty file path"));

        let err = CliArgs::parse(["--config", "   "]).unwrap_err();
        assert!(err.to_string().contains("requires a non-empty file path"));
    }

    #[test]
    fn test_help_text_content() {
        let help = CliArgs::help_text();
        assert!(help.contains("--console"));
        assert!(help.contains("--log-level"));
        assert!(help.contains("--version"));
        assert!(help.contains("--help"));
        assert!(help.contains("--symbol"));
    }

    #[test]
    fn test_symbol_flags() {
        let cli = CliArgs::parse(["-s", "EURUSD"]).unwrap();
        assert_eq!(cli.symbol, Some("EURUSD".to_string()));

        let cli = CliArgs::parse(["--symbol", "GBPJPY"]).unwrap();
        assert_eq!(cli.symbol, Some("GBPJPY".to_string()));

        let cli = CliArgs::parse(["--symbol=USDCHF"]).unwrap();
        assert_eq!(cli.symbol, Some("USDCHF".to_string()));

        let err = CliArgs::parse(["--symbol"]).unwrap_err();
        assert!(err.to_string().contains("requires a symbol argument"));
    }
}
