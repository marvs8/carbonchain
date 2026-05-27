use soroban_sdk::{Env, Address, BytesN, Vec, String};
use crate::types::{DataKey, CreditMetadata, VerifierReputation, Methodology};

/// Minimum TTL in ledgers (~1 year at 5s/ledger).
pub const MIN_TTL: u32 = 6_307_200;
/// Threshold below which TTL is extended (half of MIN_TTL).
pub const TTL_THRESHOLD: u32 = MIN_TTL / 2;

pub fn set_admin(env: &Env, admin: &Address) {
    env.storage().instance().set(&DataKey::Admin, admin);
}

pub fn get_admin(env: &Env) -> Option<Address> {
    env.storage().instance().get(&DataKey::Admin)
}

pub fn has_admin(env: &Env) -> bool {
    env.storage().instance().has(&DataKey::Admin)
}

pub fn set_credit(env: &Env, id: &BytesN<32>, metadata: &CreditMetadata) {
    let key = DataKey::Credit(id.clone());
    env.storage().persistent().set(&key, metadata);
    env.storage().persistent().extend_ttl(&key, TTL_THRESHOLD, MIN_TTL);
}

pub fn get_credit(env: &Env, id: &BytesN<32>) -> Option<CreditMetadata> {
    env.storage().persistent().get(&DataKey::Credit(id.clone()))
}

pub fn set_project(env: &Env, project_id: &String, metadata: &ProjectMetadata) {
    let key = DataKey::Project(project_id.clone());
    env.storage().persistent().set(&key, metadata);
    env.storage().persistent().extend_ttl(&key, TTL_THRESHOLD, MIN_TTL);
}

pub fn get_project(env: &Env, project_id: &String) -> Option<ProjectMetadata> {
    env.storage().persistent().get(&DataKey::Project(project_id.clone()))
}

pub fn get_verifiers(env: &Env) -> Vec<Address> {
    env.storage()
        .instance()
        .get(&DataKey::VerifierSet)
        .unwrap_or_else(|| Vec::new(env))
}

pub fn set_verifiers(env: &Env, verifiers: &Vec<Address>) {
    env.storage().instance().set(&DataKey::VerifierSet, verifiers);
    env.storage().instance().extend_ttl(&DataKey::VerifierSet, TTL_THRESHOLD, MIN_TTL);
}

pub fn is_verifier(env: &Env, verifier: &Address) -> bool {
    get_verifiers(env).contains(verifier)
}

/// Append a credit id to the per-project index.
pub fn add_credit_to_project(env: &Env, project_id: &String, credit_id: &BytesN<32>) {
    let key = DataKey::ProjectCredits(project_id.clone());
    let mut list: Vec<BytesN<32>> = env.storage().persistent().get(&key).unwrap_or_else(|| Vec::new(env));
    list.push_back(credit_id.clone());
    env.storage().persistent().set(&key, &list);
    env.storage().persistent().extend_ttl(&key, TTL_THRESHOLD, MIN_TTL);
}

pub fn get_credits_by_project(env: &Env, project_id: &String) -> Vec<BytesN<32>> {
    env.storage()
        .persistent()
        .get(&DataKey::ProjectCredits(project_id.clone()))
        .unwrap_or_else(|| Vec::new(env))
}

pub fn set_retirement_contract(env: &Env, addr: &Address) {
    env.storage().instance().set(&DataKey::RetirementContract, addr);
}

pub fn get_retirement_contract(env: &Env) -> Option<Address> {
    env.storage().instance().get(&DataKey::RetirementContract)
}

pub fn set_paused(env: &Env, paused: bool) {
    env.storage().instance().set(&DataKey::Paused, &paused);
}

pub fn is_paused(env: &Env) -> bool {
    env.storage().instance().get(&DataKey::Paused).unwrap_or(false)
}

pub fn get_nonce(env: &Env, addr: &Address) -> u64 {
    env.storage().persistent().get(&DataKey::Nonce(addr.clone())).unwrap_or(0u64)
}

pub fn consume_nonce(env: &Env, addr: &Address, expected: u64) -> bool {
    let current = get_nonce(env, addr);
    if current != expected { return false; }
    let key = DataKey::Nonce(addr.clone());
    env.storage().persistent().set(&key, &(current + 1));
    env.storage().persistent().extend_ttl(&key, TTL_THRESHOLD, MIN_TTL);
    true
}

pub fn get_verifier_reputation(env: &Env, verifier: &Address) -> VerifierReputation {
    env.storage()
        .persistent()
        .get(&DataKey::VerifierReputation(verifier.clone()))
        .unwrap_or(VerifierReputation {
            approval_count: 0,
            dispute_count: 0,
        })
}

pub fn set_verifier_reputation(env: &Env, verifier: &Address, rep: &VerifierReputation) {
    let key = DataKey::VerifierReputation(verifier.clone());
    env.storage().persistent().set(&key, rep);
    env.storage().persistent().extend_ttl(&key, TTL_THRESHOLD, MIN_TTL);
}

pub fn increment_approval_count(env: &Env, verifier: &Address) {
    let mut rep = get_verifier_reputation(env, verifier);
    rep.approval_count += 1;
    set_verifier_reputation(env, verifier, &rep);
}

pub fn increment_dispute_count(env: &Env, verifier: &Address) {
    let mut rep = get_verifier_reputation(env, verifier);
    rep.dispute_count += 1;
    set_verifier_reputation(env, verifier, &rep);
}

pub fn get_issuers(env: &Env) -> Vec<Address> {
    env.storage()
        .instance()
        .get(&DataKey::IssuerSet)
        .unwrap_or_else(|| Vec::new(env))
}

pub fn set_issuers(env: &Env, issuers: &Vec<Address>) {
    env.storage().instance().set(&DataKey::IssuerSet, issuers);
    env.storage().instance().extend_ttl(&DataKey::IssuerSet, TTL_THRESHOLD, MIN_TTL);
}

pub fn is_issuer(env: &Env, issuer: &Address) -> bool {
    get_issuers(env).contains(issuer)
}

pub fn get_methodologies(env: &Env) -> Vec<Methodology> {
    env.storage()
        .instance()
        .get(&DataKey::MethodologySet)
        .unwrap_or_else(|| Vec::new(env))
}

pub fn set_methodologies(env: &Env, methodologies: &Vec<Methodology>) {
    env.storage().instance().set(&DataKey::MethodologySet, methodologies);
    env.storage().instance().extend_ttl(&DataKey::MethodologySet, TTL_THRESHOLD, MIN_TTL);
}

pub fn is_methodology_valid(env: &Env, code: &String) -> bool {
    let methodologies = get_methodologies(env);
    for m in methodologies.iter() {
        if m.code == *code {
            return true;
        }
    }
    false
}
