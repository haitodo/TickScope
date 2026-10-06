//! `MetaTrader` 5 Terminal Process Lifecycle Manager.
//!
//! Provides discovery of broker MT5 executables, non-intrusive minimized launch,
//! running process detection, and safe graceful termination via `WM_CLOSE` with
//! timeout fallback.

use crate::config::BrokerConfig;
use crate::core::types::BrokerId;
use crate::deploy::discovery::{read_origin, DiscoveredTerminal};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalProcessStatus {
    Running { pid: u32 },
    Stopped,
    NotFound,
}

impl TerminalProcessStatus {
    #[must_use]
    pub const fn is_running(&self) -> bool {
        matches!(self, Self::Running { .. })
    }

    #[must_use]
    pub const fn pid(&self) -> Option<u32> {
        match self {
            Self::Running { pid } => Some(*pid),
            _ => None,
        }
    }
}

pub struct TerminalManager {
    broker_terminals: HashMap<BrokerId, Option<PathBuf>>,
    status_cache: HashMap<BrokerId, TerminalProcessStatus>,
    last_scan: Option<Instant>,
    scan_interval: Duration,
}

impl Default for TerminalManager {
    fn default() -> Self {
        Self::new()
    }
}

impl TerminalManager {
    #[must_use]
    pub fn new() -> Self {
        Self {
            broker_terminals: HashMap::new(),
            status_cache: HashMap::new(),
            last_scan: None,
            scan_interval: Duration::from_millis(1000),
        }
    }

    /// Resolve the MT5 executable path for a specific broker.
    #[must_use]
    pub fn resolve_terminal_path(
        broker: &BrokerConfig,
        discovered: &[DiscoveredTerminal],
    ) -> Option<PathBuf> {
        // 1. Explicit override from broker config
        if let Some(custom) = &broker.terminal_path {
            let p = PathBuf::from(custom);
            if p.is_file() {
                return Some(p);
            }
            if p.is_dir() {
                for exe in ["terminal64.exe", "terminal.exe"] {
                    let candidate = p.join(exe);
                    if candidate.is_file() {
                        return Some(candidate);
                    }
                }
            }
        }

        // 2. Scan discovered terminals
        let broker_name_lower = broker.name.to_lowercase();
        for term in discovered {
            let origin_str = read_origin(&term.terminal_dir).unwrap_or_default();
            let origin_lower = origin_str.to_lowercase();
            let friendly_lower = term.friendly_name.to_lowercase();
            let folder_lower = term.terminal_dir.to_string_lossy().to_lowercase();

            let matched = origin_lower.contains(&broker_name_lower)
                || friendly_lower.contains(&broker_name_lower)
                || folder_lower.contains(&broker_name_lower);

            if matched {
                let install_dir = if origin_str.is_empty() {
                    term.terminal_dir.clone()
                } else {
                    let p = PathBuf::from(&origin_str);
                    if p.is_file() {
                        return Some(p);
                    }
                    p
                };

                for exe in ["terminal64.exe", "terminal.exe"] {
                    let candidate = install_dir.join(exe);
                    if candidate.is_file() {
                        return Some(candidate);
                    }
                }
            }
        }

        None
    }

    /// Update terminal paths and process statuses for all configured brokers.
    pub fn poll_status(
        &mut self,
        brokers: &[BrokerConfig],
        discovered: &[DiscoveredTerminal],
        force: bool,
    ) {
        let now = Instant::now();
        if !force {
            if let Some(last) = self.last_scan {
                if now.duration_since(last) < self.scan_interval {
                    return;
                }
            }
        }
        self.last_scan = Some(now);

        // Update path mapping if needed
        for broker in brokers {
            self.broker_terminals
                .entry(broker.id)
                .or_insert_with(|| Self::resolve_terminal_path(broker, discovered));
        }

        let running_map = query_running_terminals();

        for broker in brokers {
            let path_opt = self
                .broker_terminals
                .get(&broker.id)
                .and_then(|p| p.as_ref());
            let status = match path_opt {
                None => TerminalProcessStatus::NotFound,
                Some(path) => {
                    let canon = path.canonicalize().unwrap_or_else(|_| path.clone());
                    if let Some(&pid) = running_map.get(&canon) {
                        TerminalProcessStatus::Running { pid }
                    } else {
                        TerminalProcessStatus::Stopped
                    }
                }
            };
            self.status_cache.insert(broker.id, status);
        }
    }

    #[must_use]
    pub fn get_status(&self, broker_id: BrokerId) -> TerminalProcessStatus {
        self.status_cache
            .get(&broker_id)
            .cloned()
            .unwrap_or(TerminalProcessStatus::NotFound)
    }

    #[must_use]
    pub fn get_exe_path(&self, broker_id: BrokerId) -> Option<&PathBuf> {
        self.broker_terminals
            .get(&broker_id)
            .and_then(|p| p.as_ref())
    }

    /// Launch terminal for a single broker.
    /// # Errors
    ///
    /// Returns a message when no executable path is known for the broker, or when the terminal
    /// process cannot be spawned.
    pub fn launch(&mut self, broker_id: BrokerId, minimized: bool) -> Result<u32, String> {
        let exe_path = self
            .get_exe_path(broker_id)
            .ok_or_else(|| format!("MT5 executable path not found for broker ID {broker_id}"))?
            .clone();

        let pid = launch_terminal_process(&exe_path, minimized)?;
        self.status_cache
            .insert(broker_id, TerminalProcessStatus::Running { pid });
        Ok(pid)
    }

    /// Launch terminals for multiple brokers, allowing a specific broker to launch in normal (non-minimized) window mode.
    pub fn launch_multiple_with_normal(
        &mut self,
        broker_ids: &[BrokerId],
        normal_broker_id: Option<BrokerId>,
        default_minimized: bool,
    ) -> Vec<(BrokerId, Result<u32, String>)> {
        let mut results = Vec::new();
        for &id in broker_ids {
            let is_normal = normal_broker_id == Some(id);
            let minimized = if is_normal { false } else { default_minimized };
            results.push((id, self.launch(id, minimized)));
        }
        results
    }

    /// Launch terminals for multiple brokers.
    pub fn launch_multiple(
        &mut self,
        broker_ids: &[BrokerId],
        minimized: bool,
    ) -> Vec<(BrokerId, Result<u32, String>)> {
        self.launch_multiple_with_normal(broker_ids, None, minimized)
    }

    /// Request graceful close of terminal for a single broker.
    pub fn stop(&mut self, broker_id: BrokerId, timeout: Duration) {
        if let Some(pid) = self.get_status(broker_id).pid() {
            stop_terminal_process_async(pid, timeout);
        }
    }

    /// Request graceful close of terminals for multiple brokers.
    pub fn stop_multiple(&mut self, broker_ids: &[BrokerId], timeout: Duration) {
        for &id in broker_ids {
            self.stop(id, timeout);
        }
    }
}

// ---------------------------------------------------------------------------
// Platform-specific process management (Windows implementation)
// ---------------------------------------------------------------------------

#[cfg(windows)]
fn query_running_terminals() -> HashMap<PathBuf, u32> {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    use windows_sys::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    let mut result = HashMap::new();

    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return result;
        }

        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;

        if Process32FirstW(snapshot, &raw mut entry) != 0 {
            loop {
                // Convert szExeFile to string
                let len = entry
                    .szExeFile
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(entry.szExeFile.len());
                let exe_name = String::from_utf16_lossy(&entry.szExeFile[..len]);

                if exe_name.eq_ignore_ascii_case("terminal64.exe")
                    || exe_name.eq_ignore_ascii_case("terminal.exe")
                {
                    let pid = entry.th32ProcessID;
                    let process_handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
                    if !process_handle.is_null() {
                        let mut path_buf = [0u16; 1024];
                        let mut path_len = path_buf.len() as u32;
                        if QueryFullProcessImageNameW(
                            process_handle,
                            0,
                            path_buf.as_mut_ptr(),
                            &raw mut path_len,
                        ) != 0
                        {
                            let full_path = PathBuf::from(std::ffi::OsString::from_wide(
                                &path_buf[..path_len as usize],
                            ));
                            let canon = full_path
                                .canonicalize()
                                .unwrap_or_else(|_| full_path.clone());
                            result.insert(canon, pid);
                        }
                        CloseHandle(process_handle);
                    }
                }

                if Process32NextW(snapshot, &raw mut entry) == 0 {
                    break;
                }
            }
        }
        CloseHandle(snapshot);
    }

    result
}

#[cfg(not(windows))]
fn query_running_terminals() -> HashMap<PathBuf, u32> {
    HashMap::new()
}

#[cfg(windows)]
fn launch_terminal_process(exe_path: &Path, minimized: bool) -> Result<u32, String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, BOOL};
    use windows_sys::Win32::System::Threading::{
        PROCESS_INFORMATION, STARTF_USESHOWWINDOW, STARTUPINFOW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWMINNOACTIVE;

    #[link(name = "kernel32")]
    extern "system" {
        fn CreateProcessW(
            lpApplicationName: *const u16,
            lpCommandLine: *mut u16,
            lpProcessAttributes: *const std::ffi::c_void,
            lpThreadAttributes: *const std::ffi::c_void,
            bInheritHandles: BOOL,
            dwCreationFlags: u32,
            lpEnvironment: *const std::ffi::c_void,
            lpCurrentDirectory: *const u16,
            lpStartupInfo: *const STARTUPINFOW,
            lpProcessInformation: *mut PROCESS_INFORMATION,
        ) -> BOOL;
    }

    let mut si: STARTUPINFOW = unsafe { std::mem::zeroed() };
    si.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
    si.dwFlags = STARTF_USESHOWWINDOW;
    si.wShowWindow = if minimized {
        SW_SHOWMINNOACTIVE as u16
    } else {
        1 // SW_SHOWNORMAL
    };

    let mut pi: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };

    let cmd_str = format!("\"{}\"", exe_path.display());
    let mut cmd_wide: Vec<u16> = cmd_str.encode_utf16().chain(std::iter::once(0)).collect();

    let work_dir_wide: Option<Vec<u16>> = exe_path.parent().map(|p| {
        p.as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    });
    let work_dir_ptr = work_dir_wide
        .as_ref()
        .map_or(std::ptr::null(), std::vec::Vec::as_ptr);

    let success = unsafe {
        CreateProcessW(
            std::ptr::null(),
            cmd_wide.as_mut_ptr(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            0,
            0,
            std::ptr::null_mut(),
            work_dir_ptr,
            &raw const si,
            &raw mut pi,
        )
    };

    if success != 0 {
        let pid = pi.dwProcessId;
        unsafe {
            CloseHandle(pi.hProcess);
            CloseHandle(pi.hThread);
        }
        log::info!(
            "[TerminalManager] Launched MT5 terminal (PID: {}) at '{}' (minimized: {})",
            pid,
            exe_path.display(),
            minimized
        );
        Ok(pid)
    } else {
        let err = unsafe { GetLastError() };
        Err(format!(
            "Failed to launch MT5 process at '{}' (Windows error code: {})",
            exe_path.display(),
            err
        ))
    }
}

#[cfg(not(windows))]
fn launch_terminal_process(exe_path: &Path, _minimized: bool) -> Result<u32, String> {
    Err(format!(
        "Terminal process launching is only supported on Windows: {}",
        exe_path.display()
    ))
}

#[cfg(windows)]
fn stop_terminal_process_async(pid: u32, timeout: Duration) {
    use windows_sys::Win32::Foundation::{CloseHandle, BOOL, HWND, LPARAM, WAIT_TIMEOUT};
    use windows_sys::Win32::System::Threading::{
        OpenProcess, TerminateProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowThreadProcessId, PostMessageW, WM_CLOSE,
    };

    unsafe extern "system" fn enum_windows_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let target_pid = lparam as u32;
        let mut win_pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, &raw mut win_pid);
        if win_pid == target_pid {
            PostMessageW(hwnd, WM_CLOSE, 0, 0);
        }
        1 // Continue enumeration
    }

    thread::spawn(move || unsafe {
        log::info!("[TerminalManager] Posting WM_CLOSE to MT5 terminal (PID: {pid})...");
        // Post WM_CLOSE to all top-level windows of target process
        EnumWindows(Some(enum_windows_proc), pid as LPARAM);

        let process_handle = OpenProcess(PROCESS_SYNCHRONIZE | PROCESS_TERMINATE, 0, pid);
        if process_handle.is_null() {
            return;
        }

        let timeout_ms = timeout.as_millis().min(u128::from(u32::MAX)) as u32;
        let wait_res = WaitForSingleObject(process_handle, timeout_ms);

        if wait_res == WAIT_TIMEOUT {
            log::warn!(
                "[TerminalManager] MT5 terminal (PID: {pid}) did not close within {timeout:?}; force terminating..."
            );
            TerminateProcess(process_handle, 1);
        } else {
            log::info!("[TerminalManager] MT5 terminal (PID: {pid}) exited cleanly.");
        }
        CloseHandle(process_handle);
    });
}

#[cfg(not(windows))]
fn stop_terminal_process_async(_pid: u32, _timeout: Duration) {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_resolve_terminal_path_override() {
        let dir = tempdir().unwrap();
        let exe = dir.path().join("terminal64.exe");
        fs::write(&exe, "").unwrap();

        let broker = BrokerConfig {
            terminal_path: Some(exe.to_string_lossy().to_string()),
            ..BrokerConfig::default()
        };

        let discovered = Vec::new();
        let resolved = TerminalManager::resolve_terminal_path(&broker, &discovered);
        assert_eq!(resolved, Some(exe));
    }

    #[test]
    fn test_resolve_terminal_path_discovery() {
        let dir = tempdir().unwrap();
        let term_dir = dir.path().join("terminal_folder");
        fs::create_dir_all(&term_dir).unwrap();
        let exe = term_dir.join("terminal64.exe");
        fs::write(&exe, "").unwrap();

        // Write origin.txt pointing to term_dir
        fs::write(
            term_dir.join("origin.txt"),
            term_dir.to_string_lossy().as_bytes(),
        )
        .unwrap();

        let broker = BrokerConfig {
            name: "MyBroker".to_string(),
            ..BrokerConfig::default()
        };

        let discovered = vec![DiscoveredTerminal {
            friendly_name: "MyBroker MT5 (12345678)".to_string(),
            terminal_dir: term_dir.clone(),
            mql5_dir: term_dir.join("MQL5"),
        }];

        let resolved = TerminalManager::resolve_terminal_path(&broker, &discovered);
        assert_eq!(resolved, Some(exe));
    }

    #[test]
    fn test_terminal_process_status_helpers() {
        let running = TerminalProcessStatus::Running { pid: 42 };
        assert!(running.is_running());
        assert_eq!(running.pid(), Some(42));

        let stopped = TerminalProcessStatus::Stopped;
        assert!(!stopped.is_running());
        assert_eq!(stopped.pid(), None);

        let not_found = TerminalProcessStatus::NotFound;
        assert!(!not_found.is_running());
        assert_eq!(not_found.pid(), None);
    }

    #[test]
    fn test_launch_multiple_with_normal_broker() {
        let mut tm = TerminalManager::new();
        let broker_ids = vec![1, 2, 3];
        // Terminals are not resolved yet, so launch will return Err for each
        let results = tm.launch_multiple_with_normal(&broker_ids, Some(1), true);
        assert_eq!(results.len(), 3);
        assert_eq!(results[0].0, 1);
        assert_eq!(results[1].0, 2);
        assert_eq!(results[2].0, 3);
        assert!(results[0].1.is_err());
        assert!(results[1].1.is_err());
        assert!(results[2].1.is_err());

        // Normal launch_multiple delegates with None
        let results2 = tm.launch_multiple(&broker_ids, true);
        assert_eq!(results2.len(), 3);
    }
}
