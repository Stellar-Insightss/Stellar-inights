use soroban_sdk::{contracttype, Address, Env};

pub const HOT_TTL_EXTEND_TO: u32 = 535_680;
pub const HOT_TTL_THRESHOLD: u32 = 100_000;

#[contracttype]
pub enum DataKey {
    Offer(Address, u64),
    OfferCounter(Address),
}

/// An open offer posted by a maker.
///
/// Slippage protection: `min_output` is the absolute minimum amount of
/// `token_out` the maker will accept. The settler specifies the actual
/// `output_amount`; execution only proceeds if `output_amount >= min_output`.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct Offer {
    pub id: u64,
    pub maker: Address,
    pub token_in: Address,
    pub amount_in: i128,
    pub token_out: Address,
    pub min_output: i128,
    pub filled: bool,
    pub cancelled: bool,
}

pub fn bump_instance(env: &Env) {
    env.storage()
        .instance()
        .extend_ttl(HOT_TTL_THRESHOLD, HOT_TTL_EXTEND_TO);
}

pub fn next_offer_id(env: &Env, maker: &Address) -> u64 {
    let key = DataKey::OfferCounter(maker.clone());
    let id: u64 = env.storage().instance().get(&key).unwrap_or(0u64);
    env.storage().instance().set(&key, &(id + 1));
    id
}

pub fn save_offer(env: &Env, offer: &Offer) {
    let key = DataKey::Offer(offer.maker.clone(), offer.id);
    env.storage().persistent().set(&key, offer);
    env.storage().persistent().extend_ttl(
        &DataKey::Offer(offer.maker.clone(), offer.id),
        HOT_TTL_THRESHOLD,
        HOT_TTL_EXTEND_TO,
    );
}

pub fn load_offer(env: &Env, maker: Address, offer_id: u64) -> Offer {
    env.storage()
        .persistent()
        .get(&DataKey::Offer(maker, offer_id))
        .unwrap()
}
