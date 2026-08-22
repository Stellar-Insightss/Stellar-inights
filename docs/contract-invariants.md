# Smart-contract invariant testing

The Soroban contracts are maintained in the dedicated
[`Stellar-Insightss/contracts`](https://github.com/Stellar-Insightss/contracts) repository.
They were intentionally split out of this dashboard repository in commit `05fd5054` so that
contract deployment, audit, and release controls are isolated from application changes.

## Contract ownership and required safety properties

The contracts repository is the source of truth for both the executable contract code and its
property/fuzz suites. Every contract change must document and test the following invariants before
it is merged:

| Contract | Core invariants |
| --- | --- |
| `access-control` | Only an authorised administrator can change roles or pause state; role membership is idempotent. |
| `analytics` | Snapshot epochs are strictly monotonic; an accepted snapshot cannot be replaced by an older epoch. |
| `stellar_insights` | Snapshot submissions are authorised, monotonically ordered, and cannot mutate state while paused. |
| `governance` | A proposal executes only after its voting period and only when quorum and the passing rule are met; a voter votes at most once. |
| `governance-voting` | Vote weights are counted exactly once, and finalisation is immutable after the deadline. |
| `escrow` | An escrow reaches exactly one terminal state; deposited funds cannot be released to both parties. |
| `multi-sig-wallet` | A transaction executes at most once and never below its configured owner threshold. |
| `time-locked-transactions` | A transfer cannot be released before its unlock time and has one terminal state. |
| `token-swap` | An offer is filled or cancelled at most once; token movement is atomic and respects the quoted amounts. |
| `upgrade` | Only approved upgrades can change the active code/version, and each proposal has one final outcome. |

## Required verification in the contracts repository

Each deployable crate must have a `tests/properties.rs` suite using generated values to exercise
its documented invariant, including numeric boundaries and call-order permutations. Parsing or
deserialising attacker-controlled input must additionally have a `cargo-fuzz` target. The contract
repository's CI runs property tests and time-boxed fuzz targets, then publishes an LCOV report so
uncovered contract paths are visible in review.

This repository deliberately does **not** vendor a second copy of the contracts: doing so would
make the dashboard CI test a potentially stale artifact rather than the code that is deployed.
The workflow in `.github/workflows/contract-fuzzing.yml` therefore verifies that contract testing
is owned by the contract repository and fails fast if a contracts directory is accidentally
reintroduced here.
