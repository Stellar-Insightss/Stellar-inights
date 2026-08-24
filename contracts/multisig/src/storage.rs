use soroban_sdk::{contracttype, Address, Env, Vec};

// TTL constants – keep hot state alive for ~30 days at 5s ledger pace.
pub const HOT_TTL_EXTEND_TO: u32 = 535_680;
pub const HOT_TTL_THRESHOLD: u32 = 100_000;

// ---------------------------------------------------------------------------
// Storage keys
// ---------------------------------------------------------------------------

#[contracttype]
pub enum DataKey {
    /// Global config: current owner list + threshold (mutable by reconfigure).
    Config,
    /// Proposal stored by ID.
    Proposal(u64),
    /// Monotonically increasing proposal counter.
    NextId,
}

// ---------------------------------------------------------------------------
// On-chain types
// ---------------------------------------------------------------------------

/// Snapshot of the signing policy captured **at proposal creation time**.
/// Even if the live config is later reconfigured, execution always checks
/// against these frozen values.
#[contracttype]
#[derive(Clone)]
pub struct PolicySnapshot {
    pub owners: Vec<Address>,
    pub threshold: u32,
}

/// A multi-sig proposal.
#[contracttype]
#[derive(Clone)]
pub struct Proposal {
    pub id: u64,
    /// Snapshot of policy at proposal time – immutable after creation.
    pub policy: PolicySnapshot,
    /// Target contract to call.
    pub target: Address,
    /// ABI function name.
    pub function: soroban_sdk::Symbol,
    /// Encoded call args stored as raw Bytes.
    pub args: soroban_sdk::Bytes,
    /// Addresses that have already approved.
    pub approvals: Vec<Address>,
    /// Whether the proposal has been executed.
    pub executed: bool,
}

/// Mutable global configuration (can be changed by reconfigure).
#[contracttype]
#[derive(Clone)]
pub struct Config {
    pub admin: Address,
    pub owners: Vec<Address>,
    pub threshold: u32,
}

// ---------------------------------------------------------------------------
// Storage helpers
// ---------------------------------------------------------------------------

pub fn bump_instance(env: &Env) {
    env.storage()
        .instance()
        .extend_ttl(HOT_TTL_THRESHOLD, HOT_TTL_EXTEND_TO);
}

pub fn load_config(env: &Env) -> Config {
    env.storage().instance().get(&DataKey::Config).unwrap()
}

pub fn save_config(env: &Env, cfg: &Config) {
    env.storage().instance().set(&DataKey::Config, cfg);
}

pub fn next_id(env: &Env) -> u64 {
    let id: u64 = env
        .storage()
        .instance()
        .get(&DataKey::NextId)
        .unwrap_or(0u64);
    id
}

pub fn increment_id(env: &Env) -> u64 {
    let id = next_id(env);
    env.storage().instance().set(&DataKey::NextId, &(id + 1));
    id
}

pub fn load_proposal(env: &Env, id: u64) -> Proposal {
    env.storage()
        .persistent()
        .get(&DataKey::Proposal(id))
        .unwrap()
}

pub fn save_proposal(env: &Env, proposal: &Proposal) {
    env.storage()
        .persistent()
        .set(&DataKey::Proposal(proposal.id), proposal);
    env.storage().persistent().extend_ttl(
        &DataKey::Proposal(proposal.id),
        HOT_TTL_THRESHOLD,
        HOT_TTL_EXTEND_TO,
    );
}
