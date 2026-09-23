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

use crate::contracts::config::Mt5DeployConfig;
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

fn resolve_terminal_friendly_name(terminal_dir: &Path, folder_name: &str) -> String {
    let origin_file = terminal_dir.join("origin.txt");
    if let Ok(origin_content) = fs::read_to_string(&origin_file) {
        let origin = origin_content.trim();
        if !origin.is_empty() {
            let app_name = Path::new(origin)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| origin.to_string());
            let hash_short = if folder_name.len() > 8 {
                &folder_name[..8]
            } else {
                folder_name
            };
            return format!("{} ({})", app_name, hash_short);
        }
    }
    folder_name.to_string()
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

fn compile_deployed_ea(terminal: &TerminalDeployReport) -> DeployCompileStatus {
    let ea_path = terminal.mql5_dir.join("Experts").join("TickCollector.mq5");
    let ex5_path = terminal.mql5_dir.join("Experts").join("TickCollector.ex5");
    let source_changed = terminal.results.iter().any(|result| {
        result.rel_name == "TickCollector.mq5"
            && matches!(result.status, DeployFileStatus::Created | DeployFileStatus::Updated)
    });
    if !source_changed && ex5_path.is_file() {
        return DeployCompileStatus::UpToDate;
    }

    let metaeditor = match find_metaeditor() {
        Some(path) => path,
        None => return DeployCompileStatus::MetaEditorNotFound,
    };
    match Command::new(metaeditor)
        .arg(format!("/compile:{}", ea_path.display()))
        .arg("/log")
        .status()
    {
        Ok(status) if status.success() => DeployCompileStatus::Compiled,
        Ok(status) => DeployCompileStatus::Failed(format!("MetaEditor exited with {}", status)),
        Err(error) => DeployCompileStatus::Failed(error.to_string()),
    }
}

/// Execute full MT5 auto-deployment according to configuration.
pub fn deploy_mt5_files(config: &Mt5DeployConfig) -> DeployReport {
    if !config.auto_deploy {
        return DeployReport {
            enabled: false,
            terminals: Vec::new(),
            warnings: Vec::new(),
        };
    }

    let sources = load_source_files();
    let (terminals, warnings) = discover_mt5_terminals(config);

    let mut terminal_reports = Vec::new();
    for term in &terminals {
        let mut rep = deploy_to_terminal(term, &sources);
        rep.compile_status = Some(compile_deployed_ea(&rep));
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
