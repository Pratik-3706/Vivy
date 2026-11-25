//! How the engine reports progress to whichever front end is running it.
//!
//! Event names and payloads are the same ones the desktop UI listens to:
//! `agent-action`, `agent-stream`, `agent-permission-request`, `phone-status`,
//! `phone-frame`, and so on.

use std::sync::Arc;

use serde::Serialize;
use serde_json::Value;

pub trait EventSink: Send + Sync + 'static {
    fn emit(&self, event: &str, payload: Value);
}

/// Shared handle to the front end's event sink.
#[derive(Clone)]
pub struct Events(Arc<dyn EventSink>);

impl Events {
    pub fn new(sink: impl EventSink) -> Self {
        Self(Arc::new(sink))
    }

    /// Discards everything; for tests.
    pub fn none() -> Self {
        Self::new(|_: &str, _: Value| {})
    }

    pub fn emit<T: Serialize + ?Sized>(&self, event: &str, payload: &T) {
        let value = serde_json::to_value(payload).unwrap_or(Value::Null);
        self.0.emit(event, value);
    }
}

impl<F> EventSink for F
where
    F: Fn(&str, Value) + Send + Sync + 'static,
{
    fn emit(&self, event: &str, payload: Value) {
        self(event, payload)
    }
}
