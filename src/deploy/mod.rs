//! `MetaTrader` 5 EA and Include Auto-Deployment Module.
//!
//! Automatically discovers `MetaTrader` 5 data folders on Windows (and user-configured custom paths),
//! and deploys:
//! - EA: `<TerminalDataDir>/MQL5/Experts/TickCollector.mq5`
//! - Includes:
//!   - `<TerminalDataDir>/MQL5/Include/TickScope/Protocol.mqh`
//!   - `<TerminalDataDir>/MQL5/Include/TickScope/SocketClient.mqh`
//!
//! Features:
//! - Idempotent deployment: skips copying if target file exists and content is identical.
//! - Live disk priority: loads latest files from `mt5/` directory if present on disk.
//! - Embedded fallback: falls back to `include_str!` if run from outside the source repository.
//! - Directory isolation: all include files are safely encapsulated in `Include/TickScope/`.

pub mod compiler;
pub mod discovery;
pub mod error;
pub mod installer;
pub mod sources;

pub use compiler::*;
pub use discovery::*;
pub use error::*;
pub use installer::*;
pub use sources::*;

use crate::config::{BrokerConfig, Mt5DeployConfig};

/// Deploy one common EA and a per-terminal map of broker symbols to the
/// listener ports reserved by this `TickScope` process.
#[must_use]
pub fn deploy_mt5_files_for_brokers(
    config: &Mt5DeployConfig,
    brokers: &[BrokerConfig],
    warmup_seconds: u32,
    session_epoch: u64,
) -> DeployReport {
    if !config.auto_deploy {
        return DeployReport {
            enabled: false,
            terminals: Vec::new(),
            warnings: Vec::new(),
        };
    }
    if brokers.is_empty() {
        return DeployReport {
            enabled: true,
            terminals: Vec::new(),
            warnings: vec![
                "Broker configuration is required to deploy the common TickCollector EA and its connection map.".to_string(),
            ],
        };
    }

    let sources = load_source_files();
    let (terminals, mut warnings) = discover_mt5_terminals(config);

    let mut terminal_reports = Vec::new();
    for term in &terminals {
        let mut rep = deploy_to_terminal(term, &sources);
        if !brokers.is_empty() {
            let connection_map = term
                .mql5_dir
                .join("Files")
                .join("TickScope")
                .join("connection.tsv");
            rep.results.push(deploy_file_idempotent(
                &connection_map,
                &connection_map_contents(brokers, warmup_seconds, session_epoch),
            ));
        }
        rep.compile_status = Some(compile_deployed_ea(
            &rep,
            &term.mql5_dir.join("Experts/TickCollector.mq5"),
        ));
        if rep.compile_status == Some(DeployCompileStatus::Compiled) {
            warnings.push(format!(
                "Updated the common TickCollector EA in '{}'. Remove and re-add any TickCollector already attached to a chart so MT5 loads the new version.",
                term.friendly_name
            ));
        }
        terminal_reports.push(rep);
    }

    DeployReport {
        enabled: true,
        terminals: terminal_reports,
        warnings,
    }
}

/// Print formatted deploy report to standard output.
pub fn print_deploy_report(report: &DeployReport) {
    if !report.enabled {
        log::info!("[MT5 Auto-Deploy] Auto-deployment is disabled in config.");
        return;
    }

    if report.terminals.is_empty() {
        log::info!("[MT5 Auto-Deploy] No MetaTrader 5 terminal directories discovered.");
    } else {
        log::info!(
            "[MT5 Auto-Deploy] Discovered {} MetaTrader 5 terminal(s):",
            report.terminals.len()
        );
        for term in &report.terminals {
            log::info!("  * {}", term.terminal_name);
            for f in &term.results {
                let status_str = match &f.status {
                    DeployFileStatus::Created => "Deployed (new)",
                    DeployFileStatus::Updated => "Deployed (updated)",
                    DeployFileStatus::SkippedIdentical => "Up to date (skipped)",
                    DeployFileStatus::Failed(err) => {
                        log::error!("    - {}: FAILED ({})", f.rel_name, err);
                        continue;
                    }
                };
                let display_path = if let Ok(rel) = f.target_path.strip_prefix(&term.mql5_dir) {
                    rel.display().to_string()
                } else {
                    f.target_path.display().to_string()
                };
                log::info!("    - {display_path}: {status_str}");
            }
            if let Some(status) = &term.compile_status {
                log::info!("    - TickCollector.ex5: {status:?}");
            }
        }
    }

    for w in &report.warnings {
        log::warn!("[MT5 Auto-Deploy Warning] {w}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn deploy_common_files_and_connection_map_are_idempotent() {
        let dir = tempdir().unwrap();
        let terminal = DiscoveredTerminal {
            friendly_name: "test".into(),
            terminal_dir: dir.path().into(),
            mql5_dir: dir.path().join("MQL5"),
        };
        let sources = SourceFiles {
            tick_collector: EMBEDDED_TICK_COLLECTOR.into(),
            protocol_mqh: EMBEDDED_PROTOCOL_MQH.into(),
            socket_client_mqh: EMBEDDED_SOCKET_CLIENT_MQH.into(),
            source_origin: "test".into(),
        };
        let brokers = crate::config::AppConfig::default().brokers;

        // Deploy common EA and includes
        let report = deploy_to_terminal(&terminal, &sources);
        assert_eq!(report.results.len(), 3);
        assert!(report
            .results
            .iter()
            .all(|r| r.status == DeployFileStatus::Created));

        // Deploy connection map
        let map_path = terminal.mql5_dir.join("Files/TickScope/connection.tsv");
        let map_content = connection_map_contents(&brokers, 7200, 1);
        let map_res = deploy_file_idempotent(&map_path, &map_content);
        assert_eq!(map_res.status, DeployFileStatus::Created);

        // Second deploy: idempotent
        let report2 = deploy_to_terminal(&terminal, &sources);
        assert!(report2
            .results
            .iter()
            .all(|r| r.status == DeployFileStatus::SkippedIdentical));
        let map_res2 = deploy_file_idempotent(&map_path, &map_content);
        assert_eq!(map_res2.status, DeployFileStatus::SkippedIdentical);
    }

    #[test]
    fn reads_utf16_terminal_origin() {
        let dir = tempdir().unwrap();
        let expected = "C:\\Trading\\端末";
        let bytes: Vec<u8> = std::iter::once(0xfeff)
            .chain(expected.encode_utf16())
            .flat_map(u16::to_le_bytes)
            .collect();
        fs::write(dir.path().join("origin.txt"), bytes).unwrap();
        assert_eq!(read_origin(dir.path()).as_deref(), Some(expected));
    }

    #[test]
    fn finds_metaeditor_next_to_executable_named_in_origin() {
        let dir = tempdir().unwrap();
        let install_dir = dir.path().join("install");
        fs::create_dir(&install_dir).unwrap();
        fs::write(install_dir.join("MetaEditor64.exe"), "").unwrap();
        fs::write(
            dir.path().join("origin.txt"),
            install_dir
                .join("terminal64.exe")
                .to_string_lossy()
                .as_bytes(),
        )
        .unwrap();
        assert_eq!(
            find_metaeditor_for_terminal(dir.path()),
            Some(install_dir.join("MetaEditor64.exe"))
        );
    }

    #[test]
    fn test_deploy_file_idempotent_lifecycle() {
        let dir = tempdir().unwrap();
        let target = dir
            .path()
            .join("Include")
            .join("TickScope")
            .join("Test.mqh");

        // 1. First deploy: Created
        let res1 = deploy_file_idempotent(&target, "content v1");
        assert_eq!(res1.status, DeployFileStatus::Created);
        assert_eq!(fs::read_to_string(&target).unwrap(), "content v1");

        // 2. Second deploy with same content: SkippedIdentical
        let res2 = deploy_file_idempotent(&target, "content v1");
        assert_eq!(res2.status, DeployFileStatus::SkippedIdentical);

        // 3. Third deploy with modified content: Updated
        let res3 = deploy_file_idempotent(&target, "content v2");
        assert_eq!(res3.status, DeployFileStatus::Updated);
        assert_eq!(fs::read_to_string(&target).unwrap(), "content v2");
    }

    #[test]
    fn test_deploy_to_terminal_creates_dedicated_tickscope_folder() {
        let dir = tempdir().unwrap();
        let mql5_dir = dir.path().join("MQL5");
        fs::create_dir_all(&mql5_dir).unwrap();

        let terminal = DiscoveredTerminal {
            friendly_name: "TestTerminal".to_string(),
            terminal_dir: dir.path().to_path_buf(),
            mql5_dir: mql5_dir.clone(),
        };

        let sources = SourceFiles {
            tick_collector: "// EA".to_string(),
            protocol_mqh: "// Protocol".to_string(),
            socket_client_mqh: "// SocketClient".to_string(),
            source_origin: "Test".to_string(),
        };

        let report = deploy_to_terminal(&terminal, &sources);
        assert_eq!(report.results.len(), 3);
        assert!(report
            .results
            .iter()
            .all(|r| r.status == DeployFileStatus::Created));

        // Verify EA is in Experts
        assert!(mql5_dir.join("Experts").join("TickCollector.mq5").is_file());
        // Verify Includes are in Include/TickScope
        assert!(mql5_dir
            .join("Include")
            .join("TickScope")
            .join("Protocol.mqh")
            .is_file());
        assert!(mql5_dir
            .join("Include")
            .join("TickScope")
            .join("SocketClient.mqh")
            .is_file());

        // Run again -> should all be SkippedIdentical
        let report2 = deploy_to_terminal(&terminal, &sources);
        assert_eq!(report2.results.len(), 3);
        assert!(report2
            .results
            .iter()
            .all(|r| r.status == DeployFileStatus::SkippedIdentical));
    }
}
