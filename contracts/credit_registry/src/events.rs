use soroban_sdk::{Env, Address, BytesN, Symbol, String};

/// Event topics use Symbol::new for consistent formatting across all contracts.
/// This ensures off-chain indexers can reliably parse event schemas.

pub fn credit_submitted(env: &Env, issuer: Address, project_id: String, credit_id: BytesN<32>, tonnes: i128) {
    let topics = (symbol_short!("submit"), issuer);
    env.events().publish(topics, (project_id, credit_id, tonnes));
}

pub fn credit_minted(env: &Env, verifier: Address, id: BytesN<32>) {
    let topics = (Symbol::new(env, "credit_minted"), verifier);
    env.events().publish(topics, id);
}

pub fn credit_flagged(env: &Env, id: BytesN<32>, reason: String) {
    let topics = (Symbol::new(env, "credit_flagged"),);
    env.events().publish(topics, (id, reason));
}

pub fn credit_disputed(env: &Env, id: BytesN<32>, disputer: Address, evidence: String) {
    let topics = (symbol_short!("dispute"), disputer);
    env.events().publish(topics, (id, evidence));
}

pub fn dispute_resolved(env: &Env, id: BytesN<32>, outcome: u32) {
    let topics = (symbol_short!("resolved"),);
    env.events().publish(topics, (id, outcome));
}

pub fn credit_expired(env: &Env, id: BytesN<32>) {
    let topics = (symbol_short!("expired"),);
    env.events().publish(topics, id);
}

pub fn credits_merged(env: &Env, new_id: BytesN<32>, source_count: u32) {
    let topics = (symbol_short!("merged"),);
    env.events().publish(topics, (new_id, source_count));
}

pub fn project_registered(env: &Env, project_id: String, owner: Address) {
    let topics = (symbol_short!("proj_reg"), owner);
    env.events().publish(topics, project_id);
}

pub fn verifier_added(env: &Env, admin: Address, verifier: Address) {
    let topics = (Symbol::new(env, "verifier_added"), admin);
    env.events().publish(topics, verifier);
}

pub fn verifier_removed(env: &Env, admin: Address, verifier: Address) {
    let topics = (Symbol::new(env, "verifier_removed"), admin);
    env.events().publish(topics, verifier);
}

pub fn contract_paused(env: &Env, admin: Address) {
    env.events().publish((Symbol::new(env, "contract_paused"),), admin);
}

pub fn contract_unpaused(env: &Env, admin: Address) {
    env.events().publish((Symbol::new(env, "contract_unpaused"),), admin);
}

pub fn credit_transferred(env: &Env, from: Address, to: Address, credit_id: BytesN<32>) {
    let topics = (symbol_short!("xfer"),);
    env.events().publish(topics, (from, to, credit_id));
}

pub fn credit_split(env: &Env, original_id: BytesN<32>, child1_id: BytesN<32>, child2_id: BytesN<32>) {
    let topics = (symbol_short!("split"),);
    env.events().publish(topics, (original_id, child1_id, child2_id));
}

pub fn batch_retired(env: &Env, buyer: Address, count: u32) {
    let topics = (symbol_short!("batch_ret"), buyer);
    env.events().publish(topics, count);
}
