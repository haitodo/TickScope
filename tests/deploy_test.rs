use tick_scope::config::{AppConfig, Mt5DeployConfig};
use tick_scope::deploy::{deploy_mt5_files_for_brokers, discover_mt5_terminals, DeployFileStatus};

#[test]
#[ignore = "Writes to installed MT5 terminals and runs MetaEditor; opt in explicitly"]
fn test_live_mt5_discovery_and_deployment_idempotency() {
    let config = Mt5DeployConfig::default();
    let app_config = tick_scope::config::load_config_from_file("config/default.toml")
        .unwrap_or_else(|_| AppConfig::default());
    let (terminals, _warnings) = discover_mt5_terminals(&config);
    println!("Discovered {} terminals", terminals.len());
    for t in &terminals {
        println!(
            "Terminal: {} -> {}",
            t.friendly_name,
            t.terminal_dir.display()
        );
    }

    // 1. First run: should create or match existing
    let report1 = deploy_mt5_files_for_brokers(&config, &app_config.brokers, app_config.history.warmup_seconds, 1);
    assert!(report1.enabled);
    assert!(
        !report1.terminals.is_empty(),
        "Expected at least 1 MT5 terminal on this machine"
    );

    for term in &report1.terminals {
        for res in &term.results {
            println!("Run 1 file: {} -> {:?}", res.rel_name, res.status);
            assert!(
                res.status == DeployFileStatus::Created
                    || res.status == DeployFileStatus::SkippedIdentical
                    || res.status == DeployFileStatus::Updated,
                "File deploy failed: {:?}",
                res.status
            );
            assert!(res.target_path.is_file());
        }
    }

    // 2. Second run: MUST all be SkippedIdentical (user requirement: skip if unchanged)
    let report2 = deploy_mt5_files_for_brokers(&config, &app_config.brokers, app_config.history.warmup_seconds, 1);
    for term in &report2.terminals {
        for res in &term.results {
            println!("Run 2 file: {} -> {:?}", res.rel_name, res.status);
            assert_eq!(
                res.status,
                DeployFileStatus::SkippedIdentical,
                "Expected file to be skipped on second run: {}",
                res.target_path.display()
            );
        }
    }
}

#[test]
fn test_live_mt5_terminal_path_resolution() {
    let config = Mt5DeployConfig::default();
    let app_config = tick_scope::config::load_config_from_file("config/default.toml")
        .unwrap_or_else(|_| AppConfig::default());
    let (terminals, _warnings) = discover_mt5_terminals(&config);

    if terminals.is_empty() {
        return; // Skip on machines without MT5 installed
    }

    println!(
        "Testing live MT5 path resolution for {} brokers...",
        app_config.brokers.len()
    );
    let mut resolved_count = 0;
    for broker in &app_config.brokers {
        let path = tick_scope::runtime::TerminalManager::resolve_terminal_path(broker, &terminals);
        println!("Broker '{}' -> {:?}", broker.name, path);
        if let Some(p) = path {
            assert!(
                p.is_file(),
                "Resolved path must be an existing executable: {p:?}"
            );
            resolved_count += 1;
        }
    }
    println!(
        "Successfully resolved {}/{} broker terminal executables.",
        resolved_count,
        app_config.brokers.len()
    );
    assert!(
        resolved_count > 0,
        "At least one broker terminal should be resolved on this system"
    );
}

#[test]
fn test_connection_map_contents_with_session_epoch_and_warmup() {
    let brokers = vec![tick_scope::config::BrokerConfig {
        id: 1,
        name: "Axiory".to_string(),
        host: "127.0.0.1".to_string(),
        port: 39001,
        symbol: "USDJPY".to_string(),
        pip_size: 0.01,
        point_size: 0.001,
        timezone_rule: tick_scope::config::TimezoneRule::NyClose,
        utc_offset_sec: 10800,
        utc_verified: true,
        auto_utc_offset: false,
        terminal_path: None,
        receive_delay_ms: None,
    }];
    let contents = tick_scope::deploy::connection_map_contents(&brokers, 7200, 1760000000);
    assert!(contents.starts_with("TICKSCOPE\t1\t1760000000\n"));
    assert!(contents.contains("1\tAxiory\tUSDJPY\t39001\t7200\n"));
    assert!(contents.ends_with("END\n"));
}
