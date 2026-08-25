//! Centralized, monitored TTL / rent-bumping for every deployed Soroban contract.
//!
//! # Problem
//!
//! Each contract bumps its own TTL inline (on ingest, save_proposal, etc.).
//! If that inline path stalls — operator error, paused ingest, network
//! partition — the contract's instance and persistent entries silently
//! archive. Restoring archived entries costs extra fee + latency and may
//! require manual operator intervention.
//!
//! # Solution
//!
//! A backend-level supervisor periodically polls every registered contract's
//! on-chain TTL via Soroban RPC `getTtl` / `getContractData` and issues
//! `extendFootprintTtl` transactions when the remaining ledger count drops
//! below a per-contract threshold. This is a **safety net**, not the
//! primary bump path — the contracts still bump inline for the fast path.
//!
//! ## Architecture
//!
//! ```text
//! ┌──────────────┐    ┌──────────────┐    ┌──────────────┐
//! │   Registry   │───▶│  TtlManager  │───▶│  RpcClient   │
//! │ (contract    │    │  (poll loop, │    │  (stellar RPC│
//! │  addresses + │    │   threshold  │    │   extend)    │
//! │  policies)   │    │   checks)    │    │              │
//! └──────────────┘    └──────┬───────┘    └──────────────┘
//!                            │
//!                     ┌──────▼───────┐
//!                     │   Metrics    │
//!                     │  (Prometheus)│
//!                     └──────────────┘
//! ```
//!
//! ## Constants
//!
//! All contracts in this repo use the same on-chain TTL constants:
//! - `HOT_TTL_EXTEND_TO = 535_680` ledgers (~31 days at 5 s pace)
//! - `HOT_TTL_THRESHOLD = 100_000` ledgers (bump when below this)
//!
//! The off-chain manager mirrors these as defaults but allows per-contract
//! overrides for contracts with different persistence profiles.

pub mod metrics;
pub mod registry;
pub mod ttl_manager;

pub use registry::{ContractEntry, ContractRegistry, TtlPolicy};
pub use ttl_manager::{RpcClient, TtlManager, TtlManagerConfig};
