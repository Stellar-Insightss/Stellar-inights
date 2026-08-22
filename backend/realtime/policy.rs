use log::{info, warn};
use prometheus::{IntCounter, IntGauge, register_int_counter, register_int_gauge};

lazy_static::lazy_static! {
    static ref SLOW_CONSUMER_COUNT: IntCounter = register_int_counter!(
        "slow_consumer_disconnects_total",
        "Total number of slow consumer disconnections"
    ).unwrap();

    static ref QUEUE_DEPTH_GAUGE: IntGauge = register_int_gauge!(
        "connection_queue_depth",
        "Current queue depth per connection"
    ).unwrap();
}

pub async fn handle_overflow(connection_id: &str) -> Result<(), String> {
    warn!("Slow consumer detected: {} - disconnecting", connection_id);

    // Increment the slow consumer counter
    SLOW_CONSUMER_COUNT.inc();

    // Log the disconnection reason
    info!("Disconnected slow consumer: {}", connection_id);

    // Optionally emit a metric for queue depth
    QUEUE_DEPTH_GAUGE.set(0);

    Ok(())
}