//! High-precision OS timer guard.
//! On Windows, raises the OS timer interrupt resolution to 1ms using timeBeginPeriod(1)
//! and restores it on Drop to eliminate sleep/wait quantization jitter.

#[cfg(windows)]
#[link(name = "winmm")]
extern "system" {
    fn timeBeginPeriod(uperiod: u32) -> u32;
    fn timeEndPeriod(uperiod: u32) -> u32;
}

pub struct PrecisionTimerGuard {
    #[cfg(windows)]
    period_ms: u32,
    #[cfg(windows)]
    active: bool,
}

impl PrecisionTimerGuard {
    pub fn new(period_ms: u32) -> Self {
        #[cfg(windows)]
        {
            unsafe {
                let res = timeBeginPeriod(period_ms);
                let active = res == 0; // TIMERR_NOERROR = 0
                if active {
                    log::info!(
                        "Windows multimedia high-resolution timer enabled ({period_ms}ms resolution)"
                    );
                } else {
                    log::warn!(
                        "Failed to enable Windows multimedia high-resolution timer (code: {res})"
                    );
                }
                Self { period_ms, active }
            }
        }
        #[cfg(not(windows))]
        {
            let _ = period_ms;
            Self {}
        }
    }
}

impl Drop for PrecisionTimerGuard {
    fn drop(&mut self) {
        #[cfg(windows)]
        {
            if self.active {
                unsafe {
                    timeEndPeriod(self.period_ms);
                    log::info!("Windows multimedia high-resolution timer restored");
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_precision_timer_guard_lifecycle() {
        let guard = PrecisionTimerGuard::new(1);
        #[cfg(windows)]
        assert!(guard.active);
        drop(guard);
    }
}

