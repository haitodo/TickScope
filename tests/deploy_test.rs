use tick_compare::contracts::config::{AppConfig, Mt5DeployConfig};
use tick_compare::runtime::deploy::{
    deploy_mt5_files_for_brokers, discover_mt5_terminals, DeployFileStatus,
};

#[test]
#[ignore = "Writes to installed MT5 terminals and runs MetaEditor; opt in explicitly"]
fn test_live_mt5_discovery_and_deployment_idempotency() {
    let config = Mt5DeployConfig::default();
    let app_config = tick_compare::config::load_config_from_file("config/default.toml")
        .unwrap_or_else(|_| AppConfig::default());
    let (terminals, _warnings) = discover_mt5_terminals(&config);
    println!("Discovered {} terminals", terminals.len());
    for t in &terminals {
        println!("Terminal: {} -> {}", t.friendly_name, t.terminal_dir.display());
    }

    // 1. First run: should create or match existing
    let report1 = deploy_mt5_files_for_brokers(&config, &app_config.brokers);
    assert!(report1.enabled);
    assert!(!report1.terminals.is_empty(), "Expected at least 1 MT5 terminal on this machine");

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
    let report2 = deploy_mt5_files_for_brokers(&config, &app_config.brokers);
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
