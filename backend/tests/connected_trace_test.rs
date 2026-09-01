use opentelemetry::trace::{TraceContextExt, TracerProvider as _};
use opentelemetry_sdk::trace::SdkTracerProvider;
use tracing_opentelemetry::OpenTelemetrySpanExt;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::Registry;
use stellar_insights_backend::observability::trace_context::{spawn_with_trace, TracedMessage};

fn setup_tracer() {
    let provider = SdkTracerProvider::builder().build();
    let tracer = provider.tracer("test");
    let telemetry = tracing_opentelemetry::layer().with_tracer(tracer);
    let subscriber = Registry::default().with(telemetry);
    let _ = tracing::subscriber::set_global_default(subscriber);
}

#[tokio::test]
async fn test_spawn_with_trace() {
    setup_tracer();

    let span = tracing::info_span!("parent_span");
    
    // We need to extract the span context BEFORE entering it and spanning,
    // to compare it later.
    let expected_trace_id = span.context().span().span_context().trace_id();

    let handle = {
        let _enter = span.enter();
        spawn_with_trace(async move {
            let child_span = tracing::info_span!("child_span");
            let _enter = child_span.enter();
            tracing::Span::current().context().span().span_context().trace_id()
        })
    };

    let actual_trace_id = handle.await.unwrap();

    // Verify trace ID is non-empty and matches
    assert!(expected_trace_id != opentelemetry::trace::TraceId::INVALID);
    assert_eq!(expected_trace_id, actual_trace_id);
}

#[tokio::test]
async fn test_traced_message_handoff() {
    setup_tracer();

    let span = tracing::info_span!("producer_span");
    let expected_trace_id = span.context().span().span_context().trace_id();
    
    let msg = {
        let _enter = span.enter();
        TracedMessage::new("hello")
    };

    let handle = tokio::spawn(async move {
        // We simulate receiving on the other side of a channel, where there's no parent span active.
        let (payload, recv_span) = msg.receive("test_msg");
        let _enter = recv_span.enter();
        
        let actual_trace_id = tracing::Span::current().context().span().span_context().trace_id();
        (payload, actual_trace_id)
    });

    let (payload, actual_trace_id) = handle.await.unwrap();
    
    assert_eq!(payload, "hello");
    assert!(expected_trace_id != opentelemetry::trace::TraceId::INVALID);
    assert_eq!(expected_trace_id, actual_trace_id);
}
