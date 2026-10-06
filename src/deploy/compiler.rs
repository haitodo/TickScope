//! `MetaEditor` compilation and verification.

use super::discovery::find_metaeditor_for_terminal;
use super::error::DeployError;
use super::installer::{DeployFileStatus, TerminalDeployReport};
use std::fs;
use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeployCompileStatus {
    Compiled,
    UpToDate,
    MetaEditorNotFound,
    Failed(DeployError),
}

#[must_use]
pub fn compile_deployed_ea(terminal: &TerminalDeployReport, ea_path: &Path) -> DeployCompileStatus {
    let ex5_path = ea_path.with_extension("ex5");
    if terminal.results.iter().any(|r| matches!(r.status, DeployFileStatus::Failed(_))) {
        return DeployCompileStatus::Failed(DeployError::Other("Source deployment failed".into()));
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
    let output_is_current = output_time.is_some_and(|output| dependencies.iter().all(|path| {
        fs::metadata(path).and_then(|m| m.modified()).is_ok_and(|source| source <= output)
    }));
    if !source_changed && output_is_current {
        return DeployCompileStatus::UpToDate;
    }

    let Some(metaeditor) = find_metaeditor_for_terminal(&terminal.terminal_dir) else { return DeployCompileStatus::MetaEditorNotFound };
    match Command::new(metaeditor)
        .arg(format!("/compile:{}", ea_path.display()))
        .arg("/log")
        .status()
    {
        Ok(status) if status.success() => {
            let compiled_is_current = fs::metadata(&ex5_path)
                .and_then(|m| m.modified())
                .is_ok_and(|output| dependencies.iter().all(|path| {
                    fs::metadata(path).and_then(|m| m.modified()).is_ok_and(|source| source <= output)
                }));
            if compiled_is_current {
                DeployCompileStatus::Compiled
            } else {
                DeployCompileStatus::Failed(DeployError::CompilationFailed(format!("MetaEditor did not produce a current {}. Check the compiler log.", ex5_path.display())))
            }
        }
        Ok(status) => DeployCompileStatus::Failed(DeployError::CompilationFailed(format!("MetaEditor exited with {status}"))),
        Err(error) => DeployCompileStatus::Failed(DeployError::CompilationFailed(error.to_string())),
    }
}
