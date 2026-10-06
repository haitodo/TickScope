//! Diagnostic logging and Windows console management.
//!
//! Provides structured, formatted terminal logging with configurable levels,
//! automatic Win32 console attachment/allocation for GUI builds, and zero-cost
//! log suppression when running normally without console flags.

use crate::core::civil_date::civil_from_days;
use log::{Level, LevelFilter, Metadata, Record, SetLoggerError};
use parking_lot::Mutex;
use std::io::Write;
use std::time::SystemTime;

/// Splits a `SystemTime` into `(days since epoch, seconds within the day, milliseconds)`.
fn utc_split(time: SystemTime) -> (i64, u32, u32) {
    let duration = time.duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    let total_secs = duration.as_secs() as i64;
    (
        total_secs.div_euclid(86_400),
        total_secs.rem_euclid(86_400) as u32,
        duration.subsec_millis(),
    )
}

/// Format a `SystemTime` as UTC date string `YYYY-MM-DD`.
#[must_use]
pub fn format_utc_date(time: SystemTime) -> String {
    let (year, month, day) = civil_from_days(utc_split(time).0);
    format!("{year:04}-{month:02}-{day:02}")
}

/// Format a `SystemTime` as UTC timestamp string `YYYY-MM-DD HH:MM:SS.mmm`.
#[must_use]
pub fn format_utc_timestamp(time: SystemTime) -> String {
    let (days, day_secs, millis) = utc_split(time);
    let (year, month, day) = civil_from_days(days);
    let hour = day_secs / 3600;
    let min = (day_secs % 3600) / 60;
    let sec = day_secs % 60;
    format!("{year:04}-{month:02}-{day:02} {hour:02}:{min:02}:{sec:02}.{millis:03}")
}

/// Formats a single log line into a string buffer.
pub fn format_log_line(
    timestamp: &str,
    level: Level,
    target: &str,
    message: &std::fmt::Arguments,
    use_color: bool,
) -> String {
    let level_str = if use_color {
        match level {
            Level::Error => "\x1b[31;1mERROR\x1b[0m",
            Level::Warn => "\x1b[33;1mWARN \x1b[0m",
            Level::Info => "\x1b[32;1mINFO \x1b[0m",
            Level::Debug => "\x1b[36;1mDEBUG\x1b[0m",
            Level::Trace => "\x1b[35;1mTRACE\x1b[0m",
        }
    } else {
        match level {
            Level::Error => "ERROR",
            Level::Warn => "WARN ",
            Level::Info => "INFO ",
            Level::Debug => "DEBUG",
            Level::Trace => "TRACE",
        }
    };

    let clean_target = target
        .strip_prefix("tick_scope::")
        .unwrap_or(target);

    format!("{timestamp} [{level_str}] [{clean_target}] {message}\n")
}

/// A lightweight, thread-safe console logger writing formatted lines to stderr.
pub struct ConsoleLogger {
    level: LevelFilter,
    use_color: bool,
    writer_lock: Mutex<()>,
}

impl ConsoleLogger {
    pub fn new(level: LevelFilter, use_color: bool) -> Self {
        Self {
            level,
            use_color,
            writer_lock: Mutex::new(()),
        }
    }
}

impl log::Log for ConsoleLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= self.level
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }

        let timestamp = format_utc_timestamp(SystemTime::now());
        let line = format_log_line(
            &timestamp,
            record.level(),
            record.target(),
            record.args(),
            self.use_color,
        );

        // Guard writes to prevent interleaved lines across threads
        let _guard = self.writer_lock.lock();
        let stderr = std::io::stderr();
        let mut handle = stderr.lock();
        let _ = handle.write_all(line.as_bytes());
        let _ = handle.flush();
    }

    fn flush(&self) {
        let _guard = self.writer_lock.lock();
        let _ = std::io::stderr().flush();
    }
}

/// Detect whether the console environment supports ANSI / VT color codes.
pub fn detect_color_support() -> bool {
    if std::env::var("NO_COLOR").is_ok() {
        return false;
    }
    #[cfg(windows)]
    {
        win_console::is_vt_color_supported()
    }
    #[cfg(not(windows))]
    {
        true
    }
}

/// Initializes the logging system based on CLI options and environment.
///
/// If `console_enabled` is false, `log::max_level` is set to `LevelFilter::Off`,
/// ensuring all subsequent log macro evaluations become instantaneous no-ops
/// without allocating strings or formatting parameters.
pub fn init_logging(
    console_enabled: bool,
    explicit_level: Option<LevelFilter>,
) -> Result<(), SetLoggerError> {
    if !console_enabled {
        log::set_max_level(LevelFilter::Off);
        return Ok(());
    }

    // Attach to existing console or allocate new one on Windows
    setup_console();

    // Determine log level: CLI option > RUST_LOG environment > default (Info)
    let level = explicit_level
        .or_else(|| {
            std::env::var("RUST_LOG").ok().and_then(|v| {
                crate::cli::parse_level_filter(&v).ok()
            })
        })
        .unwrap_or(LevelFilter::Info);

    let use_color = detect_color_support();
    let logger = ConsoleLogger::new(level, use_color);

    // If logger was already initialized (e.g. earlier in process or in test),
    // set_boxed_logger returns an error which we gracefully absorb while updating max_level.
    let _ = log::set_boxed_logger(Box::new(logger));
    log::set_max_level(level);

    Ok(())
}

/// Explicitly disable all logging output.
pub fn init_disabled_logging() {
    log::set_max_level(LevelFilter::Off);
}

/// Attaches to the parent console or allocates a new console window if needed.
pub fn setup_console() -> bool {
    #[cfg(windows)]
    {
        win_console::setup_console()
    }
    #[cfg(not(windows))]
    {
        true
    }
}

/// Cleans up any allocated console window upon shutdown.
pub fn cleanup_console() {
    #[cfg(windows)]
    {
        win_console::cleanup_console();
    }
}

/// Returns true if a new console window was allocated specifically for this process.
pub fn is_console_allocated() -> bool {
    #[cfg(windows)]
    {
        win_console::is_allocated()
    }
    #[cfg(not(windows))]
    {
        false
    }
}

#[cfg(windows)]
pub mod win_console {
    use std::fs::OpenOptions;
    use std::os::windows::io::IntoRawHandle;
    use std::sync::atomic::{AtomicBool, Ordering};

    const ATTACH_PARENT_PROCESS: u32 = 0xFFFFFFFF;
    const STD_INPUT_HANDLE: u32 = (-10i32) as u32;
    const STD_OUTPUT_HANDLE: u32 = (-11i32) as u32;
    const STD_ERROR_HANDLE: u32 = (-12i32) as u32;
    const ENABLE_VIRTUAL_TERMINAL_PROCESSING: u32 = 0x0004;
    const INVALID_HANDLE_VALUE: *mut std::ffi::c_void = (-1isize) as *mut std::ffi::c_void;

    extern "system" {
        fn AttachConsole(dwProcessId: u32) -> i32;
        fn AllocConsole() -> i32;
        fn FreeConsole() -> i32;
        fn GetStdHandle(nStdHandle: u32) -> *mut std::ffi::c_void;
        fn SetStdHandle(nStdHandle: u32, hHandle: *mut std::ffi::c_void) -> i32;
        fn SetConsoleTitleW(lpConsoleTitle: *const u16) -> i32;
        fn GetConsoleMode(hConsoleHandle: *mut std::ffi::c_void, lpMode: *mut u32) -> i32;
        fn SetConsoleMode(hConsoleHandle: *mut std::ffi::c_void, dwMode: u32) -> i32;
    }

    static ALLOCATED_CONSOLE: AtomicBool = AtomicBool::new(false);

    /// Attach to the caller's console if available, or allocate a new console window.
    pub fn setup_console() -> bool {
        let attached = unsafe { AttachConsole(ATTACH_PARENT_PROCESS) != 0 };
        let allocated = if !attached {
            let ok = unsafe { AllocConsole() != 0 };
            if ok {
                ALLOCATED_CONSOLE.store(true, Ordering::SeqCst);
                let title: Vec<u16> = "TickScope Console\0".encode_utf16().collect();
                unsafe {
                    SetConsoleTitleW(title.as_ptr());
                }
            }
            ok
        } else {
            false
        };

        if attached || allocated {
            redirect_std_handles();
            true
        } else {
            false
        }
    }

    /// Re-bind standard handles to CONOUT$ and CONIN$ if they were not already redirected.
    fn redirect_std_handles() {
        let curr_out = unsafe { GetStdHandle(STD_OUTPUT_HANDLE) };
        if curr_out.is_null() || curr_out == INVALID_HANDLE_VALUE {
            if let Ok(conout) = OpenOptions::new().write(true).open("CONOUT$") {
                let raw = conout.into_raw_handle();
                unsafe {
                    SetStdHandle(STD_OUTPUT_HANDLE, raw);
                }
            }
        }

        let curr_err = unsafe { GetStdHandle(STD_ERROR_HANDLE) };
        if curr_err.is_null() || curr_err == INVALID_HANDLE_VALUE {
            if let Ok(conout) = OpenOptions::new().write(true).open("CONOUT$") {
                let raw = conout.into_raw_handle();
                unsafe {
                    SetStdHandle(STD_ERROR_HANDLE, raw);
                }
            }
        }

        let curr_in = unsafe { GetStdHandle(STD_INPUT_HANDLE) };
        if curr_in.is_null() || curr_in == INVALID_HANDLE_VALUE {
            if let Ok(conin) = OpenOptions::new().read(true).open("CONIN$") {
                let raw = conin.into_raw_handle();
                unsafe {
                    SetStdHandle(STD_INPUT_HANDLE, raw);
                }
            }
        }

        // Enable Virtual Terminal Processing for ANSI colors if supported
        for handle_id in [STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
            let handle = unsafe { GetStdHandle(handle_id) };
            if !handle.is_null() && handle != INVALID_HANDLE_VALUE {
                let mut mode: u32 = 0;
                if unsafe { GetConsoleMode(handle, &mut mode) } != 0 {
                    unsafe {
                        SetConsoleMode(handle, mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING);
                    }
                }
            }
        }
    }

    /// Free newly allocated console window if one was created.
    pub fn cleanup_console() {
        if ALLOCATED_CONSOLE.swap(false, Ordering::SeqCst) {
            unsafe {
                FreeConsole();
            }
        }
    }

    /// Check if stderr console mode has virtual terminal processing enabled.
    pub fn is_vt_color_supported() -> bool {
        let err_handle = unsafe { GetStdHandle(STD_ERROR_HANDLE) };
        if err_handle.is_null() || err_handle == INVALID_HANDLE_VALUE {
            return false;
        }
        let mut mode: u32 = 0;
        if unsafe { GetConsoleMode(err_handle, &mut mode) } != 0 {
            (mode & ENABLE_VIRTUAL_TERMINAL_PROCESSING) != 0
        } else {
            false
        }
    }

    /// Returns true if a console window was newly allocated for this process.
    pub fn is_allocated() -> bool {
        ALLOCATED_CONSOLE.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_format_utc_timestamp_known_epoch() {
        // Unix epoch = 1970-01-01 00:00:00.000
        let epoch = SystemTime::UNIX_EPOCH;
        assert_eq!(format_utc_timestamp(epoch), "1970-01-01 00:00:00.000");

        // 1 day + 3 hours + 25 mins + 45 secs + 123 millis later:
        // 86400 + 3*3600 + 25*60 + 45 = 86400 + 10800 + 1500 + 45 = 98745
        let test_time = epoch + Duration::from_millis(98_745_123);
        assert_eq!(format_utc_timestamp(test_time), "1970-01-02 03:25:45.123");
    }

    #[test]
    fn test_format_utc_timestamp_late_february() {
        // 2026-02-28 23:59:59.999 UTC: a late-February date, where a broken leap-year
        // correction in the shared calendar silently returns the wrong month.
        let time = SystemTime::UNIX_EPOCH + Duration::from_millis(1_772_323_199_999);
        assert_eq!(format_utc_timestamp(time), "2026-02-28 23:59:59.999");
    }

    #[test]
    fn test_format_utc_date_known_values() {
        assert_eq!(format_utc_date(SystemTime::UNIX_EPOCH), "1970-01-01");
        let late_february = SystemTime::UNIX_EPOCH + Duration::from_millis(1_772_323_199_999);
        assert_eq!(format_utc_date(late_february), "2026-02-28");
    }

    #[test]
    fn test_format_log_line_plain() {
        let msg = format_args!("Test message: {}", 42);
        let line = format_log_line(
            "2026-09-26 12:00:00.000",
            Level::Info,
            "tick_scope::transport::tcp",
            &msg,
            false,
        );
        assert_eq!(
            line,
            "2026-09-26 12:00:00.000 [INFO ] [transport::tcp] Test message: 42\n"
        );
    }

    #[test]
    fn test_format_log_line_with_color() {
        let msg = format_args!("Warning occurred");
        let line = format_log_line(
            "2026-09-26 12:00:00.000",
            Level::Warn,
            "tick_scope::main",
            &msg,
            true,
        );
        assert!(line.contains("\x1b[33;1mWARN \x1b[0m"));
        assert!(line.contains("[main] Warning occurred"));
    }

    #[test]
    fn test_console_logger_enabled() {
        let logger = ConsoleLogger::new(LevelFilter::Warn, false);
        assert!(log::Log::enabled(&logger, &Metadata::builder().level(Level::Error).build()));
        assert!(log::Log::enabled(&logger, &Metadata::builder().level(Level::Warn).build()));
        assert!(!log::Log::enabled(&logger, &Metadata::builder().level(Level::Info).build()));
        assert!(!log::Log::enabled(&logger, &Metadata::builder().level(Level::Debug).build()));
    }

    #[test]
    fn test_init_disabled_logging_sets_max_level_off() {
        init_disabled_logging();
        assert_eq!(log::max_level(), LevelFilter::Off);
    }
}
