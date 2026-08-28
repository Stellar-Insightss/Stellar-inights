# Observability & Distributed Tracing

This document outlines the patterns and requirements for maintaining distributed tracing context across asynchronous boundaries in the Stellar Insights backend.

## Async Handoffs & `tokio::spawn`

By default, `tokio::spawn` does not inherit the OpenTelemetry/Tracing span from the parent task. This leads to broken traces where spawned tasks appear as new, disconnected traces.

To prevent this, **always** use the sanctioned helpers provided in `trace_context.rs`:

1. **Spawning async tasks**: Use `spawn_with_trace` instead of `tokio::spawn`.
   ```rust
   use crate::observability::trace_context::spawn_with_trace;

   spawn_with_trace(async move {
       // Your async task logic here
   });
   ```

2. **Spawning blocking tasks**: Use `spawn_blocking_with_trace` instead of `tokio::task::spawn_blocking`.

3. **Channel Message Handoffs**: Wrap your message payloads with `TracedMessage`.
   ```rust
   use crate::observability::trace_context::TracedMessage;

   // Sender
   let msg = TracedMessage::new(MyPayload { .. });
   tx.send(msg).await;

   // Receiver
   let msg = rx.recv().await.unwrap();
   let (payload, span) = msg.receive("my_payload");
   let _enter = span.enter(); // Propagates context into this task
   ```

## Code Review Checklist

Reviewers MUST check the following for any new Pull Requests:
- [ ] No direct calls to `tokio::spawn` or `tokio::task::spawn_blocking` (unless explicitly justified).
- [ ] Use of `spawn_with_trace` for all background tasks and async dispatch.
- [ ] Cross-channel messages carry `TracedMessage` to propagate OpenTelemetry trace contexts.

To automatically lint this locally, you can use `clippy::disallowed_methods` by configuring `clippy.toml` to disallow `tokio::spawn`.
