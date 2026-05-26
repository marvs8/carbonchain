use soroban_sdk::{contracttype, Address, String, BytesN};

#[derive(Clone, Debug, PartialEq)]
#[contracttype]
pub struct RetirementRecord {
    pub credit_id: BytesN<32>,
    pub buyer: Address,
    pub tonnes_retired: i128,
    pub reason: String,
    pub retired_at: u64,
}

#[derive(Clone)]
#[contracttype]
pub enum DataKey {
    Retirement(BytesN<32>),
    AccountRetirements(Address),
    Admin,
    Paused,
}
