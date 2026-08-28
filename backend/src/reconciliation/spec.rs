/// Off-chain aggregate record to reconcile against the on-chain snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OffChainAggregate {
    pub period: u64,
    pub snapshot_hash: [u8; 32],
    pub source_data_hash: [u8; 32],
}

/// On-chain snapshot view normalized for reconciliation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OnChainSnapshotView {
    pub period: u64,
    pub snapshot_hash: [u8; 32],
    pub source_data_hash: [u8; 32],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiscrepancyKind {
    MissingOnChain,
    MissingOffChain,
    EpochMismatch,
    SnapshotHashMismatch,
    SourceDataHashMismatch,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Discrepancy {
    pub period: u64,
    pub kind: DiscrepancyKind,
    pub detail: String,
}

/// Explicit agreement contract between off-chain and on-chain representations.
///
/// Agreement is exact-match only:
/// - The compared records must be for the same period.
/// - `snapshot_hash` must match byte-for-byte.
/// - `source_data_hash` must match byte-for-byte.
///
/// Tolerance is intentionally zero for all fields in this initial implementation.
#[derive(Debug, Clone, Default)]
pub struct AgreementSpec;

impl AgreementSpec {
    pub fn compare(
        &self,
        period: u64,
        offchain: Option<&OffChainAggregate>,
        onchain: Option<&OnChainSnapshotView>,
    ) -> Vec<Discrepancy> {
        match (offchain, onchain) {
            (None, None) => Vec::new(),
            (Some(_), None) => vec![Discrepancy {
                period,
                kind: DiscrepancyKind::MissingOnChain,
                detail: "off-chain aggregate exists but on-chain snapshot is missing".to_string(),
            }],
            (None, Some(_)) => vec![Discrepancy {
                period,
                kind: DiscrepancyKind::MissingOffChain,
                detail: "on-chain snapshot exists but off-chain aggregate is missing".to_string(),
            }],
            (Some(off), Some(on)) => {
                let mut diffs = Vec::new();

                if off.period != on.period {
                    diffs.push(Discrepancy {
                        period,
                        kind: DiscrepancyKind::EpochMismatch,
                        detail: format!(
                            "period mismatch: off-chain={}, on-chain={}",
                            off.period, on.period
                        ),
                    });
                }

                if off.snapshot_hash != on.snapshot_hash {
                    diffs.push(Discrepancy {
                        period,
                        kind: DiscrepancyKind::SnapshotHashMismatch,
                        detail: "snapshot hash mismatch".to_string(),
                    });
                }

                if off.source_data_hash != on.source_data_hash {
                    diffs.push(Discrepancy {
                        period,
                        kind: DiscrepancyKind::SourceDataHashMismatch,
                        detail: "source data hash mismatch".to_string(),
                    });
                }

                diffs
            }
        }
    }

    pub fn is_above_tolerance(&self, _discrepancy: &Discrepancy) -> bool {
        // Zero tolerance policy: every discrepancy is alert-worthy.
        true
    }
}
