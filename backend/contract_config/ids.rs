use serde::Deserialize;

#[derive(Deserialize, Debug, Clone)]
pub struct ContractIds {
    pub token_contract: String,
    pub voting_contract: String,
}

pub fn get_contract_ids() -> ContractIds {
    let file_content = include_str!("../../contracts/contract-ids.json");
    serde_json::from_str(file_content).expect("Failed to parse contract IDs")
}
