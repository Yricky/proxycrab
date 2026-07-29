use std::{
    collections::VecDeque,
    fmt,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

use tracing::{Event, Subscriber, field::Visit};
use tracing_subscriber::{Layer, layer::Context};

use crate::model::SystemLogEntry;

pub const DEFAULT_LOG_CAPACITY: usize = 10_000;

#[derive(Debug)]
pub struct LogBuffer {
    entries: Mutex<VecDeque<SystemLogEntry>>,
    next_seq: AtomicU64,
    capacity: usize,
}

impl Default for LogBuffer {
    fn default() -> Self {
        Self::new(DEFAULT_LOG_CAPACITY)
    }
}

impl LogBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            entries: Mutex::new(VecDeque::with_capacity(capacity)),
            next_seq: AtomicU64::new(1),
            capacity,
        }
    }

    pub fn push(&self, level: impl Into<String>, message: impl Into<String>) -> SystemLogEntry {
        let mut entries = self.entries.lock().expect("log buffer lock poisoned");
        let entry = SystemLogEntry {
            seq: self.next_seq.fetch_add(1, Ordering::Relaxed),
            timestamp: crate::workspace::now_millis(),
            level: level.into(),
            message: message.into(),
        };
        if self.capacity == 0 {
            return entry;
        }
        if entries.len() == self.capacity {
            entries.pop_front();
        }
        entries.push_back(entry.clone());
        entry
    }

    pub fn query(&self, after_seq: Option<u64>, limit: usize) -> Vec<SystemLogEntry> {
        let after_seq = after_seq.unwrap_or_default();
        self.entries
            .lock()
            .expect("log buffer lock poisoned")
            .iter()
            .filter(|entry| entry.seq > after_seq)
            .take(limit.min(self.capacity))
            .cloned()
            .collect()
    }

    pub fn clear(&self) {
        self.entries
            .lock()
            .expect("log buffer lock poisoned")
            .clear();
    }
}

#[derive(Clone)]
pub struct BufferLayer {
    buffer: Arc<LogBuffer>,
}

impl BufferLayer {
    pub fn new(buffer: Arc<LogBuffer>) -> Self {
        Self { buffer }
    }
}

impl<S> Layer<S> for BufferLayer
where
    S: Subscriber,
{
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let mut visitor = MessageVisitor::default();
        event.record(&mut visitor);
        let message = if visitor.message.is_empty() {
            visitor.fields.join(" ")
        } else if visitor.fields.is_empty() {
            visitor.message
        } else {
            format!("{} {}", visitor.message, visitor.fields.join(" "))
        };
        self.buffer
            .push(event.metadata().level().to_string(), message);
    }
}

#[derive(Default)]
struct MessageVisitor {
    message: String,
    fields: Vec<String>,
}

impl Visit for MessageVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn fmt::Debug) {
        if field.name() == "message" {
            self.message = format!("{value:?}").trim_matches('"').to_string();
        } else {
            self.fields.push(format!("{}={value:?}", field.name()));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, thread};

    use super::LogBuffer;

    #[test]
    fn evicts_old_entries_and_queries_incrementally() {
        let buffer = LogBuffer::new(2);
        let first = buffer.push("INFO", "one");
        let second = buffer.push("WARN", "two");
        let third = buffer.push("ERROR", "three");

        assert_eq!(buffer.query(None, 10), vec![second.clone(), third.clone()]);
        assert_eq!(buffer.query(Some(second.seq), 10), vec![third]);
        assert!(buffer.query(Some(first.seq), 1).len() == 1);
    }

    #[test]
    fn concurrent_entries_remain_in_sequence_order() {
        let buffer = Arc::new(LogBuffer::new(800));
        let threads = (0..8)
            .map(|_| {
                let buffer = buffer.clone();
                thread::spawn(move || {
                    for index in 0..100 {
                        buffer.push("INFO", index.to_string());
                    }
                })
            })
            .collect::<Vec<_>>();
        for thread in threads {
            thread.join().unwrap();
        }
        let entries = buffer.query(None, 800);
        assert_eq!(entries.len(), 800);
        assert!(entries.windows(2).all(|pair| pair[0].seq < pair[1].seq));
    }
}
