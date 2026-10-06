//! MT5 terminal and `MetaEditor` discovery.

use crate::config::Mt5DeployConfig;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct DiscoveredTerminal {
    pub friendly_name: String,
    pub terminal_dir: PathBuf,
    pub mql5_dir: PathBuf,
}

/// Discover all local MT5 terminal data folders.
#[must_use]
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

                        let canonical =
                            mql5_dir.canonicalize().unwrap_or_else(|_| mql5_dir.clone());
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
            warnings.push(format!(
                "Configured custom MT5 data directory does not exist: {custom_dir}"
            ));
            continue;
        }

        let (terminal_dir, mql5_dir) = if p.join("MQL5").is_dir() {
            (p.clone(), p.join("MQL5"))
        } else if p
            .file_name()
            .is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case("MQL5"))
            || p.join("Experts").is_dir()
        {
            let term = p.parent().unwrap_or(&p).to_path_buf();
            (term, p)
        } else {
            warnings.push(format!(
                "Configured MT5 directory '{custom_dir}' does not contain an MQL5 or Experts directory"
            ));
            continue;
        };

        let canonical = mql5_dir.canonicalize().unwrap_or_else(|_| mql5_dir.clone());
        if seen_canonical.insert(canonical) {
            let folder_name = terminal_dir
                .file_name()
                .map_or_else(|| "Custom".to_string(), |n| n.to_string_lossy().to_string());
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

#[must_use]
pub fn read_origin(terminal_dir: &Path) -> Option<String> {
    let bytes = fs::read(terminal_dir.join("origin.txt")).ok()?;
    let content = if bytes.starts_with(&[0xff, 0xfe]) || bytes.get(1) == Some(&0) {
        let u16s: Vec<u16> = bytes
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        String::from_utf16(&u16s).ok()?
    } else {
        String::from_utf8(bytes).ok()?
    };
    Some(
        content
            .trim_matches(|c: char| c.is_whitespace() || c == '\0' || c == '\u{feff}')
            .to_string(),
    )
}

#[must_use]
pub fn resolve_terminal_friendly_name(terminal_dir: &Path, folder_name: &str) -> String {
    if let Some(origin_content) = read_origin(terminal_dir) {
        let origin = origin_content.trim();
        if !origin.is_empty() {
            let app_name = Path::new(origin)
                .file_name()
                .map_or_else(|| origin.to_string(), |n| n.to_string_lossy().to_string());
            let hash_short: String = folder_name.chars().take(8).collect();
            return format!("{app_name} ({hash_short})");
        }
    }
    folder_name.to_string()
}

#[must_use]
pub fn find_metaeditor_in(dir: &Path, depth: u8) -> Option<PathBuf> {
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
        if entry.file_type().is_ok_and(|file_type| file_type.is_dir()) {
            if let Some(found) = find_metaeditor_in(&entry.path(), depth - 1) {
                return Some(found);
            }
        }
    }
    None
}

#[must_use]
pub fn find_metaeditor() -> Option<PathBuf> {
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

pub fn find_metaeditor_for_terminal(terminal_dir: &Path) -> Option<PathBuf> {
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
