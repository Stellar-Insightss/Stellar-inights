# Cross-Contract Invocation & Authorization Privilege Graph

## Overview

This document specifies the formal cross-contract invocation, authorization graph, and transitive privilege boundaries for the **Stellar Insights** smart contract system. It provides a mathematical and operational model of privilege reachability, transitive closure, attack surface analysis, and explicit scope boundaries designed to prevent unintended privilege escalation.

---

## Contract Inventory & Privileged Entry Points

| Contract | Purpose | Privileged Entry Points | Authorization Model | Transitive Capability Boundary |
| :--- | :--- | :--- | :--- | :--- |
| **`UpgradeManager`** | Governed contract upgrade orchestration | `create_proposal`, `record_realistic_storage_test`, `approve_upgrade`, `execute_upgrade`, `migrate_upgrade` | Requires `Governance` auth for proposal creation, multi-approver threshold signatures for approvals, and enforces explicit target scope restrictions. | Prohibited from self-modification and cannot target `Governance`. Invokes target via `GovernedTargetClient::governance_upgrade`. |
| **`MultisigContract`** (Governance) | Multi-owner governance wallet & execution engine | `initialize`, `reconfigure`, `propose`, `approve`, `execute` | Requires threshold approval from snapshotted owners to invoke arbitrary downstream functions as the Governance entity; `reconfigure` requires admin auth. | Snapshotted policy per proposal prevents retroactive manipulation. Protected from governed upgrade targeting by `TargetScope`. |
| **`StellarInsights`** / **`V2`** | Core protocol snapshot storage & aggregation | `initialize`, `set_upgrade_manager`, `submit_snapshot`, `governance_upgrade`, `migrate_schema` | One-time initialization; `set_upgrade_manager` requires admin auth and enforces write-once immutability; `governance_upgrade` & `migrate_schema` require `UpgradeManager` auth. | Upgraded WASM cannot rebind `UpgradeManager` due to `UpgradeManagerAlreadySet` invariant; cannot forge governance auth. |
| **`TimeLockedTransactions`** | Time-delayed execution module | `schedule_transfer`, `execute_transfer`, `get_transfer` | Sender authorization required at scheduling; funds locked immediately; deterministic permissionless execution once absolute unlock timestamp is reached. | Non-upgradeable; immutable transfer parameters (recipient, token, amount) committed at schedule time. |
| **`EscrowContract`** | Conditional fund custody & release | `__constructor`, `deposit`, `accept`, `release`, `open_dispute`, `resolve_dispute`, `timeout` | Terms committed immutably in constructor; depositor/beneficiary/arbiter must be distinct; typed resolution outcomes (`ReleaseToBeneficiary` / `RefundToDepositor`). | Non-upgradeable; no capability rotation hooks; funds can only flow to predefined terms participants. |
| **`TokenSwap`** | Automated liquidity & swap execution | `create_offer`, `cancel_offer`, `settle_offer`, `get_offer` | Maker authorization required for offer creation and cancellation; settler authorization for execution; slippage floor strictly enforced. | Non-upgradeable; self-contained escrow; no administrative backdoors or authority delegation. |
| **`Analytics`** | Off-chain indexer query aggregator | `initialize`, `pause`, `unpause`, `submit_snapshot`, `previous_snapshot`, `latest_proof` | Admin authorization required for pause, unpause, and snapshot submission; monotonic epoch ordering enforced; bounded working set ($O(1)$ storage). | Non-upgradeable; bounded state eliminates storage exhaustion risks; isolated reporting module. |

---

## Cross-Contract Invocation Graph

```mermaid
graph TD
    User["External Multi-sig Owner / Proposer"] -->|propose / approve / execute| Governance["MultisigContract (Governance)"]
    Governance -->|create_proposal| UpgradeManager["UpgradeManager Contract"]
    Approvers["Upgrade Approver Set"] -->|approve_upgrade| UpgradeManager
    UpgradeManager -->|execute_upgrade / migrate_upgrade| GovernedTarget["Governed Target (StellarInsights)"]
    
    subgraph Restricted Scope Boundaries (Blocked by scope.rs)
        UpgradeManager -.x|BLOCKED: TargetOutOfScope| Self["UpgradeManager (Self)"]
        UpgradeManager -.x|BLOCKED: TargetOutOfScope| Governance
    end

    subgraph Anti-Redirection Boundary (Governed Target Invariant)
        GovernedTarget -.x|BLOCKED: UpgradeManagerAlreadySet| RogueManager["Attacker UpgradeManager"]
        GovernedTarget -.x|BLOCKED: Unauthorized / Abort| DirectUpgrade["Direct governance_upgrade Call"]
    end
```

---

## Formal Invariant Specification (`scope.rs`)

The scope validation module `contracts/upgrade/src/scope.rs` enforces a strict mathematical authorization boundary:

### 1. Governance Root Isolation Invariant
$$\forall t \in \text{Address}, \quad t = \text{config.governance} \implies \text{validate\_target\_scope}(t) = \text{Err}(\text{TargetOutOfScope})$$
- **Guarantee**: An upgrade proposal cannot designate the Governance contract (`MultisigContract`) as an upgrade target.
- **Security Purpose**: Prevents an upgrade proposal from replacing governance multi-signature logic, changing owner rosters, reducing voting thresholds, or circumventing consensus rules.

### 2. Upgrade Orchestrator Non-Self-Modification Invariant
$$\forall t \in \text{Address}, \quad t = \text{env.current\_contract\_address}() \implies \text{validate\_target\_scope}(t) = \text{Err}(\text{TargetOutOfScope})$$
- **Guarantee**: `UpgradeManager` cannot target its own contract address for code replacement.
- **Security Purpose**: Guarantees that proposal lifecycles, storage test evidence validation, multi-approver thresholds, and target scope checks cannot be uninstalled or modified.

### 3. Transitive Authority Non-Redirection Invariant
$$\text{has\_upgrade\_manager}(\text{target}) = \text{true} \implies \text{set\_upgrade\_manager}(\text{target}, m') = \text{Err}(\text{UpgradeManagerAlreadySet})$$
- **Guarantee**: Governed target contracts bind their `UpgradeManager` address as a write-once instance storage variable.
- **Security Purpose**: Even when a governed target (e.g. `StellarInsights`) is upgraded to a new WASM binary, the newly installed code cannot reassign or redirect upgrade authority to an attacker-controlled contract. Future upgrades remain strictly gated by the genuine `UpgradeManager`.

### 4. Non-Delegation of Governance Authority Invariant
$$\text{Caller} \neq \text{config.governance} \implies \text{UpgradeManager.create\_proposal}(\text{caller}) = \text{Err}(\text{Unauthorized / Abort})$$
- **Guarantee**: Upgraded target contracts cannot forge or inherit governance authorization.
- **Security Purpose**: Upgraded code running at target addresses remains confined to its domain execution context and cannot invoke governance-restricted entrypoints on other protocol contracts.

---

## Exhaustive Capability-Granting Hook Audit

Every contract across the codebase was audited for capability-granting hooks, administrative reconfiguration vectors, and transitive escalation paths:

### 1. `UpgradeManager`
- **Hook Analysis**: Entry points `create_proposal`, `approve_upgrade`, `execute_upgrade`, `migrate_upgrade`.
- **Finding**: Proposal creation is gated by `config.governance.require_auth()`. Approvals are restricted to the registered `approvers` set with threshold verification. Self-upgrade and governance upgrade are blocked via `TargetScope`.
- **Status**: **Hardened**.

### 2. `MultisigContract` (Governance)
- **Hook Analysis**: `initialize`, `reconfigure`, `propose`, `approve`, `execute`.
- **Finding**: `initialize` is single-write (`DataKey::Config`). `reconfigure` requires snapshotted `admin.require_auth()`. All open proposals snapshot owner rosters and thresholds (`PolicySnapshot`) at creation time, preventing retroactive manipulation.
- **Status**: **Hardened**.

### 3. `StellarInsights` & `StellarInsightsV2`
- **Hook Analysis**: `set_upgrade_manager`, `governance_upgrade`, `migrate_schema`.
- **Finding**: `set_upgrade_manager` enforces `Error::UpgradeManagerAlreadySet` if `DataKey::UpgradeManager` already exists. `governance_upgrade` and `migrate_schema` strictly require `manager.require_auth()`.
- **Status**: **Hardened**. Upgraded code cannot redirect `UpgradeManager` or execute unauthenticated migrations.

### 4. `TimeLockedTransactions`
- **Hook Analysis**: `schedule_transfer`, `execute_transfer`.
- **Finding**: Transfer parameters (sender, recipient, token, amount, unlock_time) are immutable once scheduled. Ledger sequence and monotonic timestamp progressions are strictly checked.
- **Status**: **Accepted Risk / Safe Design**. No administrative rotation hooks exist.

### 5. `EscrowContract`
- **Hook Analysis**: `__constructor`, `deposit`, `accept`, `release`, `open_dispute`, `resolve_dispute`, `timeout`.
- **Finding**: Escrow terms (depositor, beneficiary, arbiter, token, amount, timeouts) are committed immutably during contract initialization. Dispute resolution is constrained to typed outcomes releasing to either the beneficiary or depositor.
- **Status**: **Accepted Risk / Safe Design**. Zero capability delegation or upgrade vectors.

### 6. `TokenSwap`
- **Hook Analysis**: `create_offer`, `cancel_offer`, `settle_offer`.
- **Finding**: Offers are isolated and escrowed in instance storage. Maker authorization is verified for cancellation; slippage protections enforce floor outputs during settlement.
- **Status**: **Accepted Risk / Safe Design**. Self-contained atomic swap primitive.

### 7. `Analytics`
- **Hook Analysis**: `initialize`, `pause`, `unpause`, `submit_snapshot`.
- **Finding**: State transitions require `admin.require_auth()`. Storage is bounded to a persistent entry count of 1 to prevent resource exhaustion.
- **Status**: **Accepted Risk / Safe Design**. Reporting aggregator with no cross-contract privilege authority.

---

## Executable Exploit-Attempt Verification

The verification suite in `contracts/upgrade/tests/privilege_escalation_test.rs` executes genuine Soroban contract deployments and interactions:

1. **Self-Upgrade Escalation Check**: `UpgradeManager` rejecting proposals targeting itself with `TargetOutOfScope`.
2. **Governance Upgrade Escalation Check**: `UpgradeManager` rejecting proposals targeting `Governance` with `TargetOutOfScope`.
3. **Threshold Gate Check**: Unapproved proposals cannot invoke `execute_upgrade`.
4. **End-to-End Upgrade & Anti-Redirection Verification**:
   - Deploys `UpgradeManager` and `StellarInsights` v1.
   - Bootstraps snapshot data.
   - Executes governed upgrade to `StellarInsightsV2`.
   - **Exploit Attempt**: Attempts calling `set_upgrade_manager` on the upgraded V2 contract — verified rejected with `TargetError::UpgradeManagerAlreadySet`.
   - **Exploit Attempt**: Attempts calling `governance_upgrade` directly from unauthorized callers — verified rejected.
   - **Exploit Attempt**: Attempts calling `migrate_schema` directly without manager auth — verified rejected.
   - Executes legitimate migration via `UpgradeManager.migrate_upgrade` and confirms state integrity.
