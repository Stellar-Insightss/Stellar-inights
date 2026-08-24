use soroban_sdk::{contracttype, Address, BytesN, Vec};

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GovernanceConfig {
    pub governance: Address,
    pub approvers: Vec<Address>,
    pub threshold: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProposalStatus {
    Pending,
    Approved,
    UpgradeApplied,
    Executed,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UpgradeProposal {
    pub id: u32,
    pub target: Address,
    pub new_wasm_hash: BytesN<32>,
    pub expected_source_schema: u32,
    pub target_schema: u32,
    pub proposer: Address,
    pub status: ProposalStatus,
    pub approval_count: u32,
    pub realistic_test_evidence: Option<BytesN<32>>,
}
