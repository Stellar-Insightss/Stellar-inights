use opentelemetry::Context;
use std::future::Future;
use tokio::task::JoinHandle;
use tracing::Instrument;
use tracing_opentelemetry::OpenTelemetrySpanExt;

/// Spawns a new asynchronous task, propagating the current tracing span.
/// 
/// This is the sanctioned way to spawn tasks to ensure distributed traces
/// don't break across task boundaries.
pub fn spawn_with_trace<F>(future: F) -> JoinHandle<F::Output>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    let span = tracing::Span::current();
    tokio::spawn(future.instrument(span))
}

/// Spawns a blocking task, propagating the current tracing span.
pub fn spawn_blocking_with_trace<F, R>(f: F) -> JoinHandle<R>
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    let span = tracing::Span::current();
    tokio::task::spawn_blocking(move || {
        let _enter = span.enter();
        f()
    })
}

/// A wrapper for sending messages across channels with trace context.
#[derive(Debug)]
pub struct TracedMessage<T> {
    pub payload: T,
    context: Context,
}

impl<T> TracedMessage<T> {
    /// Wraps a payload with the current tracing context.
    pub fn new(payload: T) -> Self {
        Self {
            payload,
            context: tracing::Span::current().context(),
        }
    }

    /// Extracts the payload and creates a new child span linked to the sender's context.
    pub fn receive(self, name: &'static str) -> (T, tracing::Span) {
        let span = tracing::info_span!("receive", message_type = name);
        span.set_parent(self.context);
        (self.payload, span)
    }
}
