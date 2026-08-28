# Cross-Contract Invocation & Authorization Privilege Graph

## Overview

This document specifies the complete cross-contract invocation and authorization graph for the **Stellar Insights** smart contract system. It provides a formal model of privilege reachability, transitive closure, attack surface analysis, and explicit scope boundaries designed to prevent unintended privilege escalation.

---

## Contract Inventory & Entry Points

| Contract | Purpose | Privileged Entry Points | Authorization Model |
| :--- | :--- | :--- | :--- |
| **`UpgradeManager`** | Governed contract upgrade orchestration | `create_proposal`, `record_realistic_storage_test`, `approve_upgrade`, `execute_upgrade`, `migrate_upgrade` | Requires `Governance` auth to create proposals, multi-approver signatures to approve, and enforces target scope checks. |
| **`MultisigContract`** (Governance) | Multi-owner governance wallet & execution engine | `initialize`, `reconfigure`, `propose`, `approve`, `execute` | Requires threshold approval from snapshotted owners to invoke arbitrary downstream functions as the Governance entity. |
| **`StellarInsights`** | Core protocol data & snapshot storage | `initialize`, `set_upgrade_manager`, `submit_snapshot`, `governance_upgrade`, `migrate_schema` | Admin auth for snapshot submission and upgrade manager setup; `UpgradeManager` auth for WASM upgrade & schema migration. |
| **`TimeLockedTransactions`** | Time-delayed execution module | `initialize`, `queue_transaction`, `execute_transaction`, `cancel_transaction` | Requires admin/proposer authorization and enforces delay locks before execution. |
| **`EscrowContract`** | Conditional fund custody & release | `initialize`, `deposit`, `release`, `refund` | Requires authorized depositor/arbiter sign-off to execute funds transfer. |
| **`TokenSwap`** | Automated liquidity & swap execution | `initialize`, `swap`, `add_liquidity`, `remove_liquidity` | User signatures for swaps; admin for pool parameters. |
| **`Analytics`** | Off-chain indexer query aggregator | `record_metric`, `query_analytics` | Authorized metric providers; public queries. |

---

## Cross-Contract Invocation Edges

```mermaid
graph TD
    User["External User / Proposer"] -->|propose / approve| Governance["MultisigContract (Governance)"]
    Governance -->|create_proposal| UpgradeManager["UpgradeManager Contract"]
    Approvers["Upgrade Approver Set"] -->|approve_upgrade| UpgradeManager
    UpgradeManager -->|execute_upgrade| GovernedTarget["Governed Target (e.g. StellarInsights)"]
    UpgradeManager -->|migrate_upgrade| GovernedTarget
    
    subgraph Restricted Targets (Blocked Scope)
        UpgradeManager -.x|BLOCKED BY scope.rs| Self["UpgradeManager (Self)"]
        UpgradeManager -.x|BLOCKED BY scope.rs| Governance
        UpgradeManager -.x|BLOCKED BY scope.rs| AccessControl["AccessControl / System Core"]
    end
```

---

## Transitive Closure Analysis

### Reachable Privilege Chains

1. **Chain 1: Governance Proposal -> Contract Upgrade**
   - **Path**: `Multisig Owner` $\rightarrow$ `Multisig.propose` + `approve` $\rightarrow$ `Multisig.execute` $\rightarrow$ `UpgradeManager.create_proposal(target, wasm_hash)` $\rightarrow$ `Approvers.approve_upgrade` $\rightarrow$ `UpgradeManager.execute_upgrade` $\rightarrow$ `GovernedTarget.governance_upgrade`.
   - **Reachability**: External multisig owners and upgrade approvers can transitively update the executable WASM of governed domain target contracts (e.g., `StellarInsights`).
   - **Intended Scope**: **Yes**. Governed targets are explicitly designed to receive updated code binaries approved by governance and multi-signature approvers.

2. **Chain 2: Transitive Self-Modification / Access Control Hijacking (Potential Risk)**
   - **Path**: `Multisig Owner` $\rightarrow$ `UpgradeManager.create_proposal(target = UpgradeManager or MultisigContract, rogue_wasm)` $\rightarrow$ `execute_upgrade` $\rightarrow$ Target code replaced with malicious binary $\rightarrow$ Total privilege escalation / role modification in `MultisigContract` or `UpgradeManager`.
   - **Reachability**: If `UpgradeManager.create_proposal` accepts `UpgradeManager` or `MultisigContract` (Governance) as target contracts, a successful proposal can alter the governance rules, bypass thresholds, or assign arbitrary roles.
   - **Intended Scope**: **UNINTENDED / HAZARDOUS**. Allowing standard target upgrades to re-write governance or upgrade infrastructure contracts breaks privilege separation and enables single-path total system takeover.

---

## Scope Restrictions (`scope.rs`)

To mitigate unintended wide-reaching transitive privilege escalation, explicit target scoping rules are implemented in `contracts/upgrade/src/scope.rs`:

1. **Self-Upgrade Restriction**: `UpgradeManager` cannot target its own contract address (`env.current_contract_address()`).
2. **Governance Contract Restriction**: `UpgradeManager` cannot target the `Governance` contract address (`config.governance`).
3. **Explicit Scope Validation**: `create_proposal` validates every target address before recording a proposal. If `target` matches a restricted system address, proposal creation is aborted with `Error::TargetOutOfScope`.

---

## Escalation Testing Strategy

`contracts/upgrade/tests/privilege_escalation_test.rs` validates the following invariant checks:

- [x] **Legitimate Target Upgrade**: Upgrades targeting valid governed contracts succeed when all approvals are present.
- [x] **Blocked Self-Upgrade**: Proposals targeting `UpgradeManager` fail immediately with `TargetOutOfScope`.
- [x] **Blocked Governance Upgrade**: Proposals targeting `Governance` (`MultisigContract`) fail immediately with `TargetOutOfScope`.
- [x] **Threshold Enforcement**: Unapproved or partially approved proposals cannot trigger `execute_upgrade`.
- [x] **Unauthorized Target Call**: Direct invocations of `governance_upgrade` on targets from non-manager callers fail with `Unauthorized`.
