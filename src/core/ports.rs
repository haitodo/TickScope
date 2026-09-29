//! Abstract ports and sinks for inter-module communication.

use super::models::*;
use super::types::*;
use std::sync::Arc;

pub trait ClockPort: Send + Sync {
    fn sample(&self) -> ClockReading;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubmitResult<T> {
    Accepted,
    Full(T),
    Closed(T),
}

pub trait RawIngressSink: Send + Sync {
    fn try_submit(&self, item: IngressItem) -> SubmitResult<IngressItem>;

    /// Submit an item with a timeout when the queue is full, avoiding busy polling.
    fn submit_timeout(
        &self,
        item: IngressItem,
        timeout: std::time::Duration,
    ) -> SubmitResult<IngressItem> {
        let _ = timeout;
        self.try_submit(item)
    }
}

pub trait HealthSink: Send + Sync {
    fn update_health(&self, broker_id: BrokerId, health: HealthState);
    fn report_diagnostic(&self, diagnostic: Diagnostic);
}

pub trait TransportControl: Send + Sync {
    fn close_connection(&self, broker_id: BrokerId, generation: u64, reason: &str);
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppendResult<T> {
    Accepted,
    Full(T),
    Fault(T, String),
}

pub trait LogSinkPort: Send + Sync {
    fn try_append(&self, record: Arc<LogRecord>) -> AppendResult<Arc<LogRecord>>;

    /// Append a record at the durable boundary used by the reliable transport.
    ///
    /// Implementations that only provide an in-memory sink may retain the
    /// default behaviour. The production logger overrides this to wait until
    /// the record is flushed and synced before reporting success.
    fn append_durable(&self, record: Arc<LogRecord>) -> Result<(), String> {
        match self.try_append(record) {
            AppendResult::Accepted => Ok(()),
            AppendResult::Full(_) => Err("Log queue is full".to_string()),
            AppendResult::Fault(_, reason) => Err(reason),
        }
    }

    fn flush(&self) -> Result<(), String>;
}

pub trait SnapshotExchangePort: Send + Sync {
    fn publish(&self, snapshot: Arc<UiSnapshot>);
    fn load_latest(&self) -> Arc<UiSnapshot>;
    fn register_repaint_signal(&self, _signal: Arc<dyn Fn() + Send + Sync>) {}
}
