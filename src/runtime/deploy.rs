//! MetaTrader 5 EA and Include Auto-Deployment Module.
//!
//! Automatically discovers MetaTrader 5 data folders on Windows (and user-configured custom paths),
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

use crate::contracts::config::{BrokerConfig, Mt5DeployConfig};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Embedded copies as reliable fallback when source repo is not on disk
pub const EMBEDDED_TICK_COLLECTOR: &str = include_str!("../../mt5/TickCollector.mq5");
pub const EMBEDDED_PROTOCOL_MQH: &str = include_str!("../../mt5/Include/TickScope/Protocol.mqh");
pub const EMBEDDED_SOCKET_CLIENT_MQH: &str = include_str!("../../mt5/Include/TickScope/SocketClient.mqh");

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeployFileStatus {
    Created,
    Updated,
    SkippedIdentical,
    Archived,
    Failed(String),
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
    pub broker_compile_results: Vec<(String, DeployCompileStatus)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeployCompileStatus {
    Compiled,
    UpToDate,
    MetaEditorNotFound,
    Failed(String),
}

#[derive(Debug, Clone, Default)]
pub struct DeployReport {
    pub enabled: bool,
    pub terminals: Vec<TerminalDeployReport>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct DiscoveredTerminal {
    pub friendly_name: String,
    pub terminal_dir: PathBuf,
    pub mql5_dir: PathBuf,
}

/// Discovered source files to deploy
#[derive(Debug, Clone)]
pub struct SourceFiles {
    pub tick_collector: String,
    pub protocol_mqh: String,
    pub socket_client_mqh: String,
    pub source_origin: String,
}

/// Locate source files, prioritizing live disk files and falling back to embedded code.
pub fn load_source_files() -> SourceFiles {
    let candidate_dirs = [
        PathBuf::from("mt5"),
        PathBuf::from("../mt5"),
        PathBuf::from("../../mt5"),
    ];

    for base in &candidate_dirs {
        let ea_path = base.join("TickCollector.mq5");
        let proto_path = base.join("Include").join("TickScope").join("Protocol.mqh");
        let socket_path = base.join("Include").join("TickScope").join("SocketClient.mqh");

        if ea_path.is_file() && proto_path.is_file() && socket_path.is_file() {
            if let (Ok(ea), Ok(proto), Ok(sock)) = (
                fs::read_to_string(&ea_path),
                fs::read_to_string(&proto_path),
                fs::read_to_string(&socket_path),
            ) {
                return SourceFiles {
                    tick_collector: ea,
                    protocol_mqh: proto,
                    socket_client_mqh: sock,
                    source_origin: format!("Disk: {}", base.display()),
                };
            }
        }
    }

    SourceFiles {
        tick_collector: EMBEDDED_TICK_COLLECTOR.to_string(),
        protocol_mqh: EMBEDDED_PROTOCOL_MQH.to_string(),
        socket_client_mqh: EMBEDDED_SOCKET_CLIENT_MQH.to_string(),
        source_origin: "Embedded binary default".to_string(),
    }
}

/// Discover all local MT5 terminal data folders.
pub fn discover_mt5_terminals(config: &Mt5DeployConfig) -> (Vec<DiscoveredTerminal>, Vec<String>) {
    let mut terminals = Vec::new();
    let mut warnings = Vec::new();
    let mut seen_canonical = std::collections::HashSet::new();

    // 1. Scan standard Windows APPDATA directory
    if let Ok(appdata) = std::env::var("APPDATA") {
        let terminal_root = PathBuf::from(appdata).join("MetaQuotes").join("Terminal");
        if terminal_root.is_dir() {
            if let Ok(entries) = fs::read_dir(&terminal_root) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if !path.is_dir() {
                        continue;
                    }

                    let mql5_dir = path.join("MQL5");
                    if mql5_dir.is_dir() {
                        let folder_name = entry.file_name().to_string_lossy().to_string();
                        // Common and Community folders do not represent standalone terminals
                        if folder_name.eq_ignore_ascii_case("Common")
                            || folder_name.eq_ignore_ascii_case("Community")
                        {
                            continue;
                        }

                        // Read origin.txt if available for friendly name
                        let friendly_name = resolve_terminal_friendly_name(&path, &folder_name);

                        let canonical = mql5_dir.canonicalize().unwrap_or_else(|_| mql5_dir.clone());
                        if seen_canonical.insert(canonical) {
                            terminals.push(DiscoveredTerminal {
                                friendly_name,
                                terminal_dir: path,
                                mql5_dir,
                            });
                        }
                    }
                }
            }
        }
    }

    // 2. Scan custom_data_dirs from config
    for custom_dir in &config.custom_data_dirs {
        let p = PathBuf::from(custom_dir);
        if !p.exists() {
            warnings.push(format!("Configured custom MT5 data directory does not exist: {}", custom_dir));
            continue;
        }

        let (terminal_dir, mql5_dir) = if p.join("MQL5").is_dir() {
            (p.clone(), p.join("MQL5"))
        } else if p.file_name().map(|n| n.to_string_lossy().eq_ignore_ascii_case("MQL5")).unwrap_or(false)
            || p.join("Experts").is_dir()
        {
            let term = p.parent().unwrap_or(&p).to_path_buf();
            (term, p)
        } else {
            warnings.push(format!(
                "Configured MT5 directory '{}' does not contain an MQL5 or Experts directory",
                custom_dir
            ));
            continue;
        };

        let canonical = mql5_dir.canonicalize().unwrap_or_else(|_| mql5_dir.clone());
        if seen_canonical.insert(canonical) {
            let folder_name = terminal_dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "Custom".to_string());
            let friendly_name = resolve_terminal_friendly_name(&terminal_dir, &folder_name);
            terminals.push(DiscoveredTerminal {
                friendly_name,
                terminal_dir,
                mql5_dir,
            });
        }
    }

    (terminals, warnings)
}

fn read_origin(terminal_dir: &Path) -> Option<String> {
    let bytes = fs::read(terminal_dir.join("origin.txt")).ok()?;
    let content = if bytes.starts_with(&[0xff, 0xfe]) || bytes.get(1) == Some(&0) {
        let bytes = bytes.strip_prefix(&[0xff, 0xfe]).unwrap_or(&bytes);
        String::from_utf16(&bytes.chunks_exact(2).map(|b| u16::from_le_bytes([b[0], b[1]])).collect::<Vec<_>>()).ok()?
    } else {
        String::from_utf8(bytes).ok()?
    };
    Some(content.trim_matches(|c: char| c.is_whitespace() || c == '\0' || c == '\u{feff}').to_string())
}

fn resolve_terminal_friendly_name(terminal_dir: &Path, folder_name: &str) -> String {
    if let Some(origin_content) = read_origin(terminal_dir) {
        let origin = origin_content.trim();
        if !origin.is_empty() {
            let app_name = Path::new(origin)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| origin.to_string());
            let hash_short: String = folder_name.chars().take(8).collect();
            return format!("{} ({})", app_name, hash_short);
        }
    }
    folder_name.to_string()
}

pub fn broker_ea_name(broker: &BrokerConfig) -> String {
    let name: String = broker.name.chars().map(|c| {
        if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' }
    }).take(64).collect();
    format!("TickCollector_{}_{}.mq5", broker.id, name)
}

pub fn deploy_broker_eas(terminal: &DiscoveredTerminal, sources: &SourceFiles, brokers: &[BrokerConfig]) -> Vec<FileDeployResult> {
    brokers.iter().map(|broker| {
        let host = match broker.host.as_str() {
            "0.0.0.0" => "127.0.0.1",
            "::" => "::1",
            host => host,
        };
        let host = host.replace('\\', "\\\\").replace('"', "\\\"").replace('\r', "\\r").replace('\n', "\\n");
        let content = format!(
            "// Generated by TickScope. Connection settings come from the app config.\n#define TICKSCOPE_BROKER_ID {}\n#define TICKSCOPE_SERVER_HOST \"{}\"\n#define TICKSCOPE_SERVER_PORT {}\n{}",
            broker.id, host, broker.port, sources.tick_collector
        );
        deploy_file_idempotent(&terminal.mql5_dir.join("Experts/TickScope").join(broker_ea_name(broker)), &content)
    }).collect()
}

/// Create the per-terminal routing table read by the single shared EA.
/// The END record lets the EA reject a partially-written table and retry it.
fn tsv_safe(value: &str) -> String {
    value
        .chars()
        .map(|character| match character {
            '\t' | '\r' | '\n' => ' ',
            character => character,
        })
        .collect()
}

pub fn connection_map_contents(brokers: &[BrokerConfig]) -> String {
    let mut contents = String::from("TICKSCOPE\t1\n");
    for broker in brokers {
        let server_hint = tsv_safe(&broker.name);
        let symbol = tsv_safe(&broker.symbol);
        contents.push_str(&format!(
            "{}\t{}\t{}\t{}\n",
            broker.id, server_hint, symbol, broker.port
        ));
    }
    contents.push_str("END\n");
    contents
}

/// Deploy a single file idempotently.
/// If file already exists and byte contents match, returns SkippedIdentical.
pub fn deploy_file_idempotent(target_path: &Path, content: &str) -> FileDeployResult {
    let rel_name = target_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "file".to_string());

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
                        status: DeployFileStatus::Failed(e.to_string()),
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
                    status: DeployFileStatus::Failed(e.to_string()),
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
                    status: DeployFileStatus::Failed(format!("Failed to create parent dir: {}", e)),
                };
            }
        }
    }

    match fs::write(target_path, content.as_bytes()) {
        Ok(_) => FileDeployResult {
            rel_name,
            target_path: target_path.to_path_buf(),
            status: DeployFileStatus::Created,
        },
        Err(e) => FileDeployResult {
            rel_name,
            target_path: target_path.to_path_buf(),
            status: DeployFileStatus::Failed(e.to_string()),
        },
    }
}

/// Archive only the broker-specific EA files that this app previously generated.
/// The marker check protects hand-written files, and moving them out of Experts
/// keeps old binaries recoverable without showing them as attachable EAs.
fn archive_legacy_generated_broker_eas(terminal: &DiscoveredTerminal) -> Vec<FileDeployResult> {
    const GENERATED_MARKER: &str = "// Generated by TickScope. Connection settings come from the app config.";

    let legacy_dir = terminal.mql5_dir.join("Experts").join("TickScope");
    let canonical_mql5 = match terminal.mql5_dir.canonicalize() {
        Ok(path) => path,
        Err(_) => return Vec::new(),
    };
    let canonical_legacy_dir = match legacy_dir.canonicalize() {
        Ok(path) if path.starts_with(&canonical_mql5) => path,
        _ => return Vec::new(),
    };
    let entries = match fs::read_dir(&canonical_legacy_dir) {
        Ok(entries) => entries,
        Err(_) => return Vec::new(),
    };
    let mut legacy_sources = Vec::new();
    for entry in entries.flatten() {
        let source_path = entry.path();
        let is_legacy_name = source_path
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| name.starts_with("TickCollector_") && name.ends_with(".mq5"))
            .unwrap_or(false);
        if !is_legacy_name || !source_path.is_file() {
            continue;
        }
        if fs::read_to_string(&source_path)
            .map(|contents| contents.contains(GENERATED_MARKER))
            .unwrap_or(false)
        {
            legacy_sources.push(source_path);
        }
    }
    if legacy_sources.is_empty() {
        return Vec::new();
    }

    let archive_dir = terminal
        .mql5_dir
        .join("Files")
        .join("TickScope")
        .join("legacy_broker_eas");
    if let Err(error) = fs::create_dir_all(&archive_dir) {
        return vec![FileDeployResult {
            rel_name: "legacy_broker_eas".to_string(),
            target_path: archive_dir,
            status: DeployFileStatus::Failed(error.to_string()),
        }];
    }
    let canonical_archive_dir = match archive_dir.canonicalize() {
        Ok(path) if path.starts_with(&canonical_mql5) => path,
        _ => {
            return vec![FileDeployResult {
                rel_name: "legacy_broker_eas".to_string(),
                target_path: archive_dir,
                status: DeployFileStatus::Failed(
                    "Archive directory resolves outside the MT5 data directory".to_string(),
                ),
            }];
        }
    };
    let mut results = Vec::new();
    for source_path in legacy_sources {
        for source in [source_path.clone(), source_path.with_extension("ex5")] {
            let Some(file_name) = source.file_name() else {
                continue;
            };
            let path = canonical_archive_dir.join(file_name);
            if !source.is_file() {
                continue;
            }
            let rel_name = path
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_else(|| "legacy EA".to_string());
            let status = if path.exists() {
                DeployFileStatus::Failed("Archive destination already exists; original file was kept".to_string())
            } else {
                match fs::rename(&source, &path) {
                    Ok(()) => DeployFileStatus::Archived,
                    Err(error) => DeployFileStatus::Failed(error.to_string()),
                }
            };
            results.push(FileDeployResult {
                rel_name,
                target_path: path,
                status,
            });
        }
    }
    results
}

/// Deploy EA and Include files to a single discovered MT5 terminal directory.
pub fn deploy_to_terminal(terminal: &DiscoveredTerminal, sources: &SourceFiles) -> TerminalDeployReport {
    let mut results = Vec::new();

    // 1. EA: <MQL5>/Experts/TickCollector.mq5
    let ea_path = terminal.mql5_dir.join("Experts").join("TickCollector.mq5");
    results.push(deploy_file_idempotent(&ea_path, &sources.tick_collector));

    // 2. Includes: <MQL5>/Include/TickScope/Protocol.mqh
    let proto_path = terminal
        .mql5_dir
        .join("Include")
        .join("TickScope")
        .join("Protocol.mqh");
    results.push(deploy_file_idempotent(&proto_path, &sources.protocol_mqh));

    // 3. Includes: <MQL5>/Include/TickScope/SocketClient.mqh
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
        broker_compile_results: Vec::new(),
    }
}

fn find_metaeditor_in(dir: &Path, depth: u8) -> Option<PathBuf> {
    for name in ["MetaEditor64.exe", "MetaEditor.exe"] {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    if depth == 0 {
        return None;
    }
    let entries = fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        if entry.file_type().map(|file_type| file_type.is_dir()).unwrap_or(false) {
            if let Some(found) = find_metaeditor_in(&entry.path(), depth - 1) {
                return Some(found);
            }
        }
    }
    None
}

fn find_metaeditor() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("TICKSCOPE_METAEDITOR_PATH") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Some(path);
        }
    }

    for variable in ["ProgramFiles", "ProgramFiles(x86)"] {
        if let Ok(root) = std::env::var(variable) {
            if let Some(found) = find_metaeditor_in(Path::new(&root), 2) {
                return Some(found);
            }
        }
    }
    None
}

fn find_metaeditor_for_terminal(terminal_dir: &Path) -> Option<PathBuf> {
    if let Some(origin) = read_origin(terminal_dir) {
        let origin = PathBuf::from(origin);
        // origin.txt usually contains terminal64.exe. It may also be a directory
        // on installations that use a custom launcher.
        let install_dir = if origin.extension().is_some() {
            origin.parent().map(Path::to_path_buf)
        } else {
            Some(origin)
        };
        if let Some(install_dir) = install_dir {
            if let Some(editor) = find_metaeditor_in(&install_dir, 0) {
                return Some(editor);
            }
        }
    }
    find_metaeditor_in(terminal_dir, 0).or_else(find_metaeditor)
}

fn compile_deployed_ea(terminal: &TerminalDeployReport, ea_path: &Path) -> DeployCompileStatus {
    let ex5_path = ea_path.with_extension("ex5");
    if terminal.results.iter().any(|r| matches!(r.status, DeployFileStatus::Failed(_))) {
        return DeployCompileStatus::Failed("Source deployment failed".into());
    }
    // Includes affect the binary just as much as the EA itself.
    let source_changed = terminal.results.iter().any(|result| {
        matches!(result.status, DeployFileStatus::Created | DeployFileStatus::Updated)
            && matches!(
                result.target_path.extension().and_then(|extension| extension.to_str()),
                Some("mq5" | "mqh")
            )
    });
    let output_time = fs::metadata(&ex5_path).and_then(|m| m.modified()).ok();
    let dependencies = [
        ea_path.to_path_buf(),
        terminal.mql5_dir.join("Include/TickScope/Protocol.mqh"),
        terminal.mql5_dir.join("Include/TickScope/SocketClient.mqh"),
    ];
    let output_is_current = output_time.map(|output| dependencies.iter().all(|path| {
        fs::metadata(path).and_then(|m| m.modified()).map(|source| source <= output).unwrap_or(false)
    })).unwrap_or(false);
    if !source_changed && output_is_current {
        return DeployCompileStatus::UpToDate;
    }

    let metaeditor = match find_metaeditor_for_terminal(&terminal.terminal_dir) {
        Some(path) => path,
        None => return DeployCompileStatus::MetaEditorNotFound,
    };
    match Command::new(metaeditor)
        .arg(format!("/compile:{}", ea_path.display()))
        .arg("/log")
        .status()
    {
        Ok(status) if status.success() => {
            let compiled_is_current = fs::metadata(&ex5_path)
                .and_then(|m| m.modified())
                .map(|output| dependencies.iter().all(|path| {
                    fs::metadata(path).and_then(|m| m.modified()).map(|source| source <= output).unwrap_or(false)
                }))
                .unwrap_or(false);
            if compiled_is_current {
                DeployCompileStatus::Compiled
            } else {
                DeployCompileStatus::Failed(format!("MetaEditor did not produce a current {}. Check the compiler log.", ex5_path.display()))
            }
        }
        Ok(status) => DeployCompileStatus::Failed(format!("MetaEditor exited with {}", status)),
        Err(error) => DeployCompileStatus::Failed(error.to_string()),
    }
}

/// Execute full MT5 auto-deployment according to configuration.
pub fn deploy_mt5_files(config: &Mt5DeployConfig) -> DeployReport {
    deploy_mt5_files_for_brokers(config, &[])
}

/// Deploy one common EA and a per-terminal map of broker symbols to the
/// listener ports reserved by this TickScope process.
pub fn deploy_mt5_files_for_brokers(config: &Mt5DeployConfig, brokers: &[BrokerConfig]) -> DeployReport {
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
                &connection_map_contents(brokers),
            ));
        }
        rep.compile_status = Some(compile_deployed_ea(&rep, &term.mql5_dir.join("Experts/TickCollector.mq5")));
        if rep.compile_status == Some(DeployCompileStatus::Compiled) {
            warnings.push(format!(
                "Updated the common TickCollector EA in '{}'. Remove and re-add any TickCollector already attached to a chart so MT5 loads the new version.",
                term.friendly_name
            ));
        }
        if !brokers.is_empty() {
            let retired_eas = archive_legacy_generated_broker_eas(term);
            if retired_eas.iter().any(|result| {
                matches!(result.status, DeployFileStatus::Archived | DeployFileStatus::Failed(_))
            }) {
                warnings.push(format!(
                    "Archived old broker-specific EA files from '{}'. Remove any old EA instance still attached to a chart, then add the common TickCollector EA. A file that could not be archived is listed in the deployment report.",
                    term.friendly_name
                ));
            }
            rep.results.extend(retired_eas);
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
        println!("[MT5 Auto-Deploy] Auto-deployment is disabled in config.");
        return;
    }

    if report.terminals.is_empty() {
        println!("[MT5 Auto-Deploy] No MetaTrader 5 terminal directories discovered.");
    } else {
        println!(
            "[MT5 Auto-Deploy] Discovered {} MetaTrader 5 terminal(s):",
            report.terminals.len()
        );
        for term in &report.terminals {
            println!("  * {}", term.terminal_name);
            for f in &term.results {
                let status_str = match &f.status {
                    DeployFileStatus::Created => "Deployed (new)",
                    DeployFileStatus::Updated => "Deployed (updated)",
                    DeployFileStatus::SkippedIdentical => "Up to date (skipped)",
                    DeployFileStatus::Archived => "Archived legacy duplicate",
                    DeployFileStatus::Failed(err) => {
                        println!("    - {}: FAILED ({})", f.rel_name, err);
                        continue;
                    }
                };
                let display_path = if let Ok(rel) = f.target_path.strip_prefix(&term.mql5_dir) {
                    rel.display().to_string()
                } else {
                    f.target_path.display().to_string()
                };
                println!("    - {}: {}", display_path, status_str);
            }
            if let Some(status) = &term.compile_status {
                println!("    - TickCollector.ex5: {:?}", status);
            }
        }
    }

    for w in &report.warnings {
        eprintln!("[MT5 Auto-Deploy Warning] {}", w);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn broker_eas_follow_config_and_are_idempotent() {
        let dir = tempdir().unwrap();
        let terminal = DiscoveredTerminal {
            friendly_name: "test".into(), terminal_dir: dir.path().into(),
            mql5_dir: dir.path().join("MQL5"),
        };
        let sources = SourceFiles {
            tick_collector: EMBEDDED_TICK_COLLECTOR.into(),
            protocol_mqh: EMBEDDED_PROTOCOL_MQH.into(),
            socket_client_mqh: EMBEDDED_SOCKET_CLIENT_MQH.into(),
            source_origin: "test".into(),
        };
        let mut brokers = crate::contracts::config::AppConfig::default().brokers;
        brokers[0].name = "../Broker/A".into();
        brokers[0].host = "0.0.0.0".into();
        let first = deploy_broker_eas(&terminal, &sources, &brokers);
        assert_eq!(first.len(), 2);
        for (result, broker) in first.iter().zip(&brokers) {
            assert_eq!(result.status, DeployFileStatus::Created);
            assert_eq!(result.target_path.parent().unwrap(), terminal.mql5_dir.join("Experts/TickScope"));
            let content = fs::read_to_string(&result.target_path).unwrap();
            assert!(content.contains(&format!("#define TICKSCOPE_BROKER_ID {}\n", broker.id)));
            assert!(content.contains(&format!("#define TICKSCOPE_SERVER_PORT {}\n", broker.port)));
            assert!(content.contains("#define TICKSCOPE_SERVER_HOST \"127.0.0.1\""));
        }
        assert!(deploy_broker_eas(&terminal, &sources, &brokers).iter().all(|r| r.status == DeployFileStatus::SkippedIdentical));
        brokers[1].port = 40123;
        let updated = deploy_broker_eas(&terminal, &sources, &brokers);
        assert_eq!(updated[0].status, DeployFileStatus::SkippedIdentical);
        assert_eq!(updated[1].status, DeployFileStatus::Updated);
        assert!(fs::read_to_string(&updated[1].target_path).unwrap().contains("#define TICKSCOPE_SERVER_PORT 40123"));
    }

    #[test]
    fn reads_utf16_terminal_origin() {
        let dir = tempdir().unwrap();
        let expected = "C:\\Trading\\端末";
        let bytes: Vec<u8> = std::iter::once(0xfeff).chain(expected.encode_utf16()).flat_map(u16::to_le_bytes).collect();
        fs::write(dir.path().join("origin.txt"), bytes).unwrap();
        assert_eq!(read_origin(dir.path()).as_deref(), Some(expected));
    }

    #[test]
    fn finds_metaeditor_next_to_executable_named_in_origin() {
        let dir = tempdir().unwrap();
        let install_dir = dir.path().join("install");
        fs::create_dir(&install_dir).unwrap();
        fs::write(install_dir.join("MetaEditor64.exe"), "").unwrap();
        fs::write(dir.path().join("origin.txt"), install_dir.join("terminal64.exe").to_string_lossy().as_bytes()).unwrap();
        assert_eq!(find_metaeditor_for_terminal(dir.path()), Some(install_dir.join("MetaEditor64.exe")));
    }

    #[test]
    fn test_deploy_file_idempotent_lifecycle() {
        let dir = tempdir().unwrap();
        let target = dir.path().join("Include").join("TickScope").join("Test.mqh");

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
        assert!(report.results.iter().all(|r| r.status == DeployFileStatus::Created));

        // Verify EA is in Experts
        assert!(mql5_dir.join("Experts").join("TickCollector.mq5").is_file());
        // Verify Includes are in Include/TickScope
        assert!(mql5_dir.join("Include").join("TickScope").join("Protocol.mqh").is_file());
        assert!(mql5_dir.join("Include").join("TickScope").join("SocketClient.mqh").is_file());

        // Run again -> should all be SkippedIdentical
        let report2 = deploy_to_terminal(&terminal, &sources);
        assert_eq!(report2.results.len(), 3);
        assert!(report2.results.iter().all(|r| r.status == DeployFileStatus::SkippedIdentical));
    }
}
