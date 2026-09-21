//! Fake Ingress and Log sinks for testing backpressure and lifecycle.

use tick_compare::contracts::ports::{AppendResult, LogSinkPort, RawIngressSink, SubmitResult};
use tick_compare::contracts::types::{IngressItem, LogRecord};
use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[allow(dead_code)]
pub struct FakeIngressSink {
    items: Mutex<Vec<IngressItem>>,
    capacity: usize,
    is_closed: AtomicBool,
}

impl FakeIngressSink {
    pub fn new(capacity: usize) -> Self {
        Self {
            items: Mutex::new(Vec::new()),
            capacity,
            is_closed: AtomicBool::new(false),
        }
    }

    pub fn set_closed(&self, closed: bool) {
        self.is_closed.store(closed, Ordering::SeqCst);
    }

    pub fn collected_items(&self) -> Vec<IngressItem> {
        self.items.lock().clone()
    }
}

impl RawIngressSink for FakeIngressSink {
    fn try_submit(&self, item: IngressItem) -> SubmitResult<IngressItem> {
        if self.is_closed.load(Ordering::SeqCst) {
            return SubmitResult::Closed(item);
        }
        let mut guard = self.items.lock();
        if guard.len() >= self.capacity {
            return SubmitResult::Full(item);
        }
        guard.push(item);
        SubmitResult::Accepted
    }
}

#[allow(dead_code)]
pub struct FakeLogSink {
    records: Mutex<Vec<Arc<LogRecord>>>,
    capacity: usize,
    fail_with: Mutex<Option<String>>,
}

impl FakeLogSink {
    pub fn new(capacity: usize) -> Self {
        Self {
            records: Mutex::new(Vec::new()),
            capacity,
            fail_with: Mutex::new(None),
        }
    }

    pub fn set_fault(&self, msg: Option<String>) {
        *self.fail_with.lock() = msg;
    }

    pub fn records(&self) -> Vec<Arc<LogRecord>> {
        self.records.lock().clone()
    }
}

impl LogSinkPort for FakeLogSink {
    fn try_append(&self, record: Arc<LogRecord>) -> AppendResult<Arc<LogRecord>> {
        if let Some(err) = self.fail_with.lock().as_ref() {
            return AppendResult::Fault(record, err.clone());
        }
        let mut guard = self.records.lock();
        if guard.len() >= self.capacity {
            return AppendResult::Full(record);
        }
        guard.push(record);
        AppendResult::Accepted
    }

    fn flush(&self) -> Result<(), String> {
        if let Some(err) = self.fail_with.lock().as_ref() {
            return Err(err.clone());
        }
        Ok(())
    }
}
