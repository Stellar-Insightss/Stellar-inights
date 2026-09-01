//! Real-time payment reliability and latency percentile computation engine for Stellar.
//!
//! # Overview
//!
//! This subsystem provides high-throughput, mergeable streaming analytics for cross-border payments:
//!
//! 1. **Bounded-Error Percentile Estimation ([`sketch`])**:
//!    Uses DDSketch to provide guaranteed, bounded relative error ($\le \alpha$, default 1%) on
//!    arbitrary quantiles (P50, P75, P90, P95, P99, P99.9) across unbounded event streams.
//!    Sketches are fully mergeable ($S_1 \oplus S_2$) across shards and windows without reprocessing raw data.
//!
//! 2. **Out-of-Order Watermarking & Windowing ([`watermark`])**:
//!    Applies event-time windowing with monotonically advancing watermarks ($W(t) = \max(T) - \Delta$)
//!    to handle out-of-order ledger events. Strict window lifecycles (`Active` $\rightarrow$ `Finalized` $\rightarrow$ `Amended`)
//!    and explicit late-event policies (`DropAndRecord`, `SideOutput`, `RetroactiveUpdate`) ensure deterministic finality.
//!
//! 3. **Two Clocks, One Truth ([`clock`])**:
//!    Distinguishes the event-time domain ($T_{\text{ledger}}$) for deterministic calculation and reproducible replay
//!    from the processing-time domain ($T_{\text{ingest}}$) for pipeline health and lag monitoring.
//!
//! 4. **Reconciliation Consistency ([`reconciliation_bridge`])**:
//!    Integrates directly with the `reconciliation` subsystem via [`WatermarkedAggregateStore`], ensuring
//!    that `reconcilable_periods()` strictly aligns with watermarked finalized windows.
//!
//! 5. **Ingestion Burst Resilience ([`engine`])**:
//!    $O(1)$ sample insertion, lock-free batch ingestion, and bounded memory retention guarantee
//!    stability under sudden backlog sync bursts (e.g. 50,000+ events).

pub mod clock;
pub mod engine;
pub mod reconciliation_bridge;
pub mod reliability;
pub mod sketch;
pub mod watermark;

pub use clock::{ClockDomain, PaymentEvent, DEFAULT_MAX_CLOCK_SKEW_SECS, DEFAULT_SLA_THRESHOLD_MS};
pub use engine::{BatchIngestResult, EngineSummary, PaymentAnalyticsEngine};
pub use reconciliation_bridge::WatermarkedAggregateStore;
pub use reliability::{
    PaymentCorridor, PaymentStatus, ReliabilityCounters, ReliabilitySummary,
};
pub use sketch::{DDSketch, ExactSummary, PercentileSummary, SketchError, DEFAULT_ALPHA};
pub use watermark::{
    IngestOutcome, LateEventPolicy, LateEventRecord, WatermarkConfig, WatermarkTracker,
    WindowMetrics, WindowState, DEFAULT_MAX_RETAINED_WINDOWS, DEFAULT_WATERMARK_DELAY_SECS,
    DEFAULT_WINDOW_SIZE_SECS,
};
