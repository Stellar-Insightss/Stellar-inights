//! Schema-versioned parsing for decoded contract event payloads.

pub mod dispatch;
pub mod parsers;

pub use dispatch::{dispatch, DispatchError, NormalizedEvent, NormalizedSnapshotSubmitted};
