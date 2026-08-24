# Analytics contract: storage lifecycle

This is the durable design record for issue #339 (bounded on-chain storage).

## What must be readable on-chain at diff time

`diff.rs` compares the incoming snapshot to **the immediately previous snapshot
only**. It never walks older epochs. Therefore the live working set is:

1. `PreviousSnapshot` (persistent) — metrics map + hashes for epoch *N−1*
2. Control-plane instance data — admin, pause flag, latest availability proof

Full historical snapshots are **not** required to ingest epoch *N*.

## TTL / bump policy

| State | Kind | Bump | If we did not bump |
|---|---|---|---|
| Admin / paused | instance | Every ingest and pause/unpause (`HOT_TTL_EXTEND_TO` ≈ 31d) | Contract becomes unusable until instance is restored. Tiny, so we keep it hot. |
| Latest availability proof | instance | Overwritten + bumped every ingest | Proof of the last committed epoch; one slot, not a log. |
| Previous snapshot | persistent | Overwritten + bumped every ingest | Diff cannot run; next ingest would look like genesis unless the operator restores from the off-chain store and resubmits. |
| Epoch *N−2* and older | not stored | — | Would grow keys and rent linearly. Allowed to “archive” by never existing. |

Constants live in `src/storage.rs` (`HOT_TTL_THRESHOLD`, `HOT_TTL_EXTEND_TO`).

We do **not** aggressively bump “everything ever written”: there is no historical
key set. Rent liability is O(1) keys, not O(ingests).

## On-chain vs off-chain

| Role | Where |
|---|---|
| Live working set + availability proof | This contract |
| Full snapshot history, query, analytics API | Backend off-chain store in this repo |
| Ingest receipt (diff counts) | Return value of `submit_snapshot`; backend persists it |

On-chain storage is a **minimal live working set plus an availability proof**,
not a duplicate of the backend ledger.

## Growth invariant

After the first successful ingest, `persistent_entry_count() == 1` for any
number of subsequent submissions. `storage_growth_test.rs` submits a long
sequence and asserts that count does not grow with *N*.
