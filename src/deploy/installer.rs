//! File deployment and idempotency checks.

use crate::config::BrokerConfig;
use super::compiler::DeployCompileStatus;
use super::discovery::DiscoveredTerminal;
use super::error::DeployError;
use super::sources::SourceFiles;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeployFileStatus {
    Created,
    Updated,
    SkippedIdentical,
    Failed(DeployError),
}

#[derive(Debug, Clone)]
pub struct FileDeployResult {
    pub rel_name: String,
    pub target_path: PathBuf,
    pub status: DeployFileStatus,
}

#[derive(Debug, Clone)]
pub struct TerminalDeployReport {
    pub terminal_name: String,
    pub terminal_dir: PathBuf,
    pub mql5_dir: PathBuf,
    pub results: Vec<FileDeployResult>,
    pub compile_status: Option<DeployCompileStatus>,
}

#[derive(Debug, Clone, Default)]
pub struct DeployReport {
    pub enabled: bool,
    pub terminals: Vec<TerminalDeployReport>,
    pub warnings: Vec<String>,
}

#[must_use]
pub fn tsv_safe(value: &str) -> String {
    value
        .chars()
        .map(|character| match character {
            '\t' | '\r' | '\n' => ' ',
            character => character,
        })
        .collect()
}

#[must_use]
pub fn connection_map_contents(brokers: &[BrokerConfig]) -> String {
    let mut contents = String::from("TICKSCOPE\t1\n");
    for broker in brokers {
        let server_hint = tsv_safe(&broker.name);
        let symbol = tsv_safe(&broker.symbol);
        let _ = writeln!(contents,
            "{}\t{}\t{}\t{}",
            broker.id, server_hint, symbol, broker.port
        );
    }
    contents.push_str("END\n");
    contents
}

#[must_use]
pub fn deploy_file_idempotent(target_path: &Path, content: &str) -> FileDeployResult {
    let rel_name = target_path
        .file_name().map_or_else(|| "file".to_string(), |n| n.to_string_lossy().to_string());

    if target_path.exists() {
        match fs::read(target_path) {
            Ok(existing_bytes) => {
                if existing_bytes == content.as_bytes() {
                    return FileDeployResult {
                        rel_name,
                        target_path: target_path.to_path_buf(),
                        status: DeployFileStatus::SkippedIdentical,
                    };
                }
                // Different content: update
                if let Err(e) = fs::write(target_path, content.as_bytes()) {
                    return FileDeployResult {
                        rel_name,
                        target_path: target_path.to_path_buf(),
                        status: DeployFileStatus::Failed(DeployError::FileWrite {
                            path: target_path.to_path_buf(),
                            message: e.to_string(),
                        }),
                    };
                }
                return FileDeployResult {
                    rel_name,
                    target_path: target_path.to_path_buf(),
                    status: DeployFileStatus::Updated,
                };
            }
            Err(e) => {
                return FileDeployResult {
                    rel_name,
                    target_path: target_path.to_path_buf(),
                    status: DeployFileStatus::Failed(DeployError::FileRead {
                        path: target_path.to_path_buf(),
                        message: e.to_string(),
                    }),
                };
            }
        }
    }

    // Target does not exist: create
    if let Some(parent) = target_path.parent() {
        if !parent.exists() {
            if let Err(e) = fs::create_dir_all(parent) {
                return FileDeployResult {
                    rel_name,
                    target_path: target_path.to_path_buf(),
                    status: DeployFileStatus::Failed(DeployError::CreateDir {
                        path: parent.to_path_buf(),
                        message: e.to_string(),
                    }),
                };
            }
        }
    }

    match fs::write(target_path, content.as_bytes()) {
        Ok(()) => FileDeployResult {
            rel_name,
            target_path: target_path.to_path_buf(),
            status: DeployFileStatus::Created,
        },
        Err(e) => FileDeployResult {
            rel_name,
            target_path: target_path.to_path_buf(),
            status: DeployFileStatus::Failed(DeployError::FileWrite {
                path: target_path.to_path_buf(),
                message: e.to_string(),
            }),
        },
    }
}

#[must_use]
pub fn deploy_to_terminal(terminal: &DiscoveredTerminal, sources: &SourceFiles) -> TerminalDeployReport {
    let mut results = Vec::new();

    let ea_path = terminal.mql5_dir.join("Experts").join("TickCollector.mq5");
    results.push(deploy_file_idempotent(&ea_path, &sources.tick_collector));

    let proto_path = terminal
        .mql5_dir
        .join("Include")
        .join("TickScope")
        .join("Protocol.mqh");
    results.push(deploy_file_idempotent(&proto_path, &sources.protocol_mqh));

    let socket_path = terminal
        .mql5_dir
        .join("Include")
        .join("TickScope")
        .join("SocketClient.mqh");
    results.push(deploy_file_idempotent(&socket_path, &sources.socket_client_mqh));

    TerminalDeployReport {
        terminal_name: terminal.friendly_name.clone(),
        terminal_dir: terminal.terminal_dir.clone(),
        mql5_dir: terminal.mql5_dir.clone(),
        results,
        compile_status: None,
    }
}
