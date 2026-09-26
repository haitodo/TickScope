//! Integration tests for CLI argument parsing and diagnostic logging.

use log::LevelFilter;
use std::path::PathBuf;
use tick_compare::cli::{parse_level_filter, CliArgs};
use tick_compare::logging::{
    format_log_line, init_disabled_logging, init_logging,
};

#[test]
fn test_cli_parsing_matrix() {
    // 1. Default (no args)
    let cli = CliArgs::parse(Vec::<&str>::new()).unwrap();
    assert!(!cli.console);
    assert_eq!(cli.config_path, None);
    assert_eq!(cli.log_level, None);
    assert!(!cli.show_help);

    // 2. Short and long console flags
    assert!(CliArgs::parse(["-c"]).unwrap().console);
    assert!(CliArgs::parse(["--console"]).unwrap().console);

    // 3. Short and long help flags
    assert!(CliArgs::parse(["-h"]).unwrap().show_help);
    assert!(CliArgs::parse(["--help"]).unwrap().show_help);

    // 4. Positional config path
    let cli = CliArgs::parse(["config/custom.toml"]).unwrap();
    assert_eq!(cli.config_path, Some(PathBuf::from("config/custom.toml")));
    assert!(!cli.console);

    // 5. Mixed flag and positional combinations (order independence)
    let cli1 = CliArgs::parse(["-c", "config/custom.toml"]).unwrap();
    let cli2 = CliArgs::parse(["config/custom.toml", "-c"]).unwrap();
    let cli3 = CliArgs::parse(["--console", "config/custom.toml"]).unwrap();
    let cli4 = CliArgs::parse(["config/custom.toml", "--console"]).unwrap();
    for cli in [cli1, cli2, cli3, cli4] {
        assert!(cli.console);
        assert_eq!(cli.config_path, Some(PathBuf::from("config/custom.toml")));
    }

    // 6. Explicit --config flag
    let cli = CliArgs::parse(["--config", "my.toml"]).unwrap();
    assert_eq!(cli.config_path, Some(PathBuf::from("my.toml")));

    let cli = CliArgs::parse(["--config=my.toml"]).unwrap();
    assert_eq!(cli.config_path, Some(PathBuf::from("my.toml")));

    // 7. Log level specifications
    let cli = CliArgs::parse(["-l", "warn"]).unwrap();
    assert_eq!(cli.log_level, Some(LevelFilter::Warn));

    let cli = CliArgs::parse(["--log-level", "debug"]).unwrap();
    assert_eq!(cli.log_level, Some(LevelFilter::Debug));

    let cli = CliArgs::parse(["--log-level=trace"]).unwrap();
    assert_eq!(cli.log_level, Some(LevelFilter::Trace));

    // 8. Full option combination
    let cli = CliArgs::parse(["-c", "--log-level=debug", "--config", "cfg.toml"]).unwrap();
    assert!(cli.console);
    assert_eq!(cli.log_level, Some(LevelFilter::Debug));
    assert_eq!(cli.config_path, Some(PathBuf::from("cfg.toml")));
    // 9. Version flags
    assert!(CliArgs::parse(["-V"]).unwrap().show_version);
    assert!(CliArgs::parse(["--version"]).unwrap().show_version);

    // 10. Option delimiter --
    let cli = CliArgs::parse(["-c", "--", "cfg.toml"]).unwrap();
    assert!(cli.console);
    assert_eq!(cli.config_path, Some(PathBuf::from("cfg.toml")));
}

#[test]
fn test_cli_error_cases() {
    // Unknown option
    assert!(CliArgs::parse(["--bogus"]).is_err());
    assert!(CliArgs::parse(["-x"]).is_err());

    // Missing option values
    assert!(CliArgs::parse(["--config"]).is_err());
    assert!(CliArgs::parse(["--config="]).is_err());
    assert!(CliArgs::parse(["--config", ""]).is_err());
    assert!(CliArgs::parse(["--config", "   "]).is_err());
    assert!(CliArgs::parse([""]).is_err());
    assert!(CliArgs::parse(["   "]).is_err());
    assert!(CliArgs::parse(["-l"]).is_err());
    assert!(CliArgs::parse(["--log-level"]).is_err());

    // Invalid log levels
    assert!(CliArgs::parse(["-l", "superdebug"]).is_err());
    assert!(CliArgs::parse(["--log-level=notalevel"]).is_err());

    // Conflicting multiple configs
    assert!(CliArgs::parse(["a.toml", "b.toml"]).is_err());
    assert!(CliArgs::parse(["--config", "a.toml", "b.toml"]).is_err());
    assert!(CliArgs::parse(["a.toml", "--config=b.toml"]).is_err());
    assert!(CliArgs::parse(["--", "a.toml", "b.toml"]).is_err());
}

#[test]
fn test_log_level_case_insensitivity_and_aliases() {
    assert_eq!(parse_level_filter("INFO").unwrap(), LevelFilter::Info);
    assert_eq!(parse_level_filter("info").unwrap(), LevelFilter::Info);
    assert_eq!(parse_level_filter("Debug").unwrap(), LevelFilter::Debug);
    assert_eq!(parse_level_filter("DEBUG").unwrap(), LevelFilter::Debug);
    assert_eq!(parse_level_filter("WARN").unwrap(), LevelFilter::Warn);
    assert_eq!(parse_level_filter("warning").unwrap(), LevelFilter::Warn);
    assert_eq!(parse_level_filter("ERROR").unwrap(), LevelFilter::Error);
    assert_eq!(parse_level_filter("err").unwrap(), LevelFilter::Error);
    assert_eq!(parse_level_filter("ERR").unwrap(), LevelFilter::Error);
    assert_eq!(parse_level_filter("Trace").unwrap(), LevelFilter::Trace);
    assert_eq!(parse_level_filter("off").unwrap(), LevelFilter::Off);
}

#[test]
fn test_logging_formatting_and_target_cleanup() {
    let msg = format_args!("Hello, {}!", "world");
    let line = format_log_line(
        "2026-09-26 12:34:56.789",
        log::Level::Info,
        "tick_compare::transport::router",
        &msg,
        false,
    );
    assert_eq!(
        line,
        "2026-09-26 12:34:56.789 [INFO ] [transport::router] Hello, world!\n"
    );

    let line_scoped = format_log_line(
        "2026-09-26 12:34:56.789",
        log::Level::Error,
        "tick_scope::coordinator",
        &msg,
        false,
    );
    assert_eq!(
        line_scoped,
        "2026-09-26 12:34:56.789 [ERROR] [coordinator] Hello, world!\n"
    );
}

static LOG_GLOBAL_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn test_log_suppression_and_activation() {
    let _guard = LOG_GLOBAL_TEST_LOCK.lock().unwrap();

    // 1. Suppression when disabled
    init_disabled_logging();
    assert_eq!(log::max_level(), LevelFilter::Off);

    let res = init_logging(false, Some(LevelFilter::Debug));
    assert!(res.is_ok());
    assert_eq!(log::max_level(), LevelFilter::Off);

    // 2. Activation when console enabled
    let res = init_logging(true, Some(LevelFilter::Warn));
    assert!(res.is_ok());
    assert_eq!(log::max_level(), LevelFilter::Warn);

    // 3. Reset back to off
    init_disabled_logging();
    assert_eq!(log::max_level(), LevelFilter::Off);
}

#[test]
fn test_is_console_allocated_in_test_env() {
    // Tests run in test runner console, so a separate console is not newly allocated
    assert!(!tick_compare::logging::is_console_allocated());
}
