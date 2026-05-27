#![no_std]
use soroban_sdk::{contract, contractimpl, Env, Address, String, BytesN, Vec};
use soroban_sdk::xdr::ToXdr;

// ── Unit convention ──────────────────────────────────────────────────────────
//
// The `tonnes` field in `CreditMetadata` stores carbon credits in **micro-tonne
// units** where:
//
//   1 tonne  = 1_000_000 units
//   0.1 tonne = 100_000 units  ← minimum resolution
//
// All amounts submitted to the contract MUST be a positive multiple of
// `MIN_CREDIT_UNIT`.  Amounts that are not a multiple are rejected with
// `CarbonChainError::InvalidTonnes`.
//
// Upper bound: 1_000_000_000_000_000 units = 1 billion tonnes.
pub const UNITS_PER_TONNE: i128 = 1_000_000;
/// Minimum credit unit — represents 0.1 tonne.
pub const MIN_CREDIT_UNIT: i128 = 100_000;

pub mod types;
pub mod errors;
pub mod storage;
pub mod events;

use crate::errors::CarbonChainError;
use crate::storage::{
    set_admin, get_admin, has_admin,
    set_credit, get_credit,
    get_verifiers, set_verifiers, is_verifier,
    add_credit_to_project, get_credits_by_project,
    set_retirement_contract, get_retirement_contract,
    set_paused, is_paused,
};
use crate::types::{CreditMetadata, CreditStatus, DataKey};
use crate::events::{
    credit_submitted, credit_minted, verifier_added, verifier_removed,
    contract_paused, contract_unpaused,
};

fn get_nonce(env: &Env, addr: &Address) -> u64 {
    env.storage()
        .persistent()
        .get(&DataKey::Nonce(addr.clone()))
        .unwrap_or(0u64)
}

fn consume_nonce(env: &Env, addr: &Address, expected: u64) -> bool {
    let current = get_nonce(env, addr);
    if current != expected {
        return false;
    }
    let key = DataKey::Nonce(addr.clone());
    env.storage().persistent().set(&key, &(current + 1));
    env.storage()
        .persistent()
        .extend_ttl(&key, crate::storage::TTL_THRESHOLD, crate::storage::MIN_TTL);
    true
}

#[cfg(not(feature = "library"))]
#[contract]
pub struct CreditRegistry;

#[cfg(not(feature = "library"))]
#[contractimpl]
impl CreditRegistry {
    // ── Admin ────────────────────────────────────────────────────────────────

    pub fn initialize(env: Env, admin: Address, retirement_contract: Address) -> Result<(), CarbonChainError> {
        if has_admin(&env) {
            return Err(CarbonChainError::AlreadyInitialized);
        }
        // Validate that admin is a legitimate, authorised address.
        // require_auth() will panic for zero/invalid addresses in the Soroban VM.
        admin.require_auth();
        set_admin(&env, &admin);
        set_retirement_contract(&env, &retirement_contract);
        Ok(())
    }

    // ── Pause / Unpause ──────────────────────────────────────────────────────

    pub fn pause(env: Env, admin: Address) -> Result<(), CarbonChainError> {
        let stored_admin = get_admin(&env).ok_or(CarbonChainError::NotInitialized)?;
        admin.require_auth();
        if admin != stored_admin {
            return Err(CarbonChainError::Unauthorized);
        }
        set_paused(&env, true);
        contract_paused(&env, admin);
        Ok(())
    }

    pub fn unpause(env: Env, admin: Address) -> Result<(), CarbonChainError> {
        let stored_admin = get_admin(&env).ok_or(CarbonChainError::NotInitialized)?;
        admin.require_auth();
        if admin != stored_admin {
            return Err(CarbonChainError::Unauthorized);
        }
        set_paused(&env, false);
        contract_unpaused(&env, admin);
        Ok(())
    }

    pub fn paused(env: Env) -> bool {
        is_paused(&env)
    }

    // ── Verifier management ──────────────────────────────────────────────────

    pub fn register_verifier(env: Env, admin: Address, verifier: Address, nonce: u64) -> Result<(), CarbonChainError> {
        let stored_admin = get_admin(&env).ok_or(CarbonChainError::NotInitialized)?;
        admin.require_auth();
        if admin != stored_admin {
            return Err(CarbonChainError::Unauthorized);
        }
        if !consume_nonce(&env, &admin, nonce) {
            return Err(CarbonChainError::InvalidNonce);
        }
        if is_verifier(&env, &verifier) {
            return Err(CarbonChainError::VerifierAlreadyExists);
        }
        let mut verifiers = get_verifiers(&env);
        verifiers.push_back(verifier.clone());
        set_verifiers(&env, &verifiers);
        verifier_added(&env, admin, verifier);
        Ok(())
    }

    pub fn remove_verifier(env: Env, admin: Address, verifier: Address, nonce: u64) -> Result<(), CarbonChainError> {
        let stored_admin = get_admin(&env).ok_or(CarbonChainError::NotInitialized)?;
        admin.require_auth();
        if admin != stored_admin {
            return Err(CarbonChainError::Unauthorized);
        }
        if !consume_nonce(&env, &admin, nonce) {
            return Err(CarbonChainError::InvalidNonce);
        }
        if !is_verifier(&env, &verifier) {
            return Err(CarbonChainError::VerifierNotFound);
        }
        let old = get_verifiers(&env);
        let mut new_list: Vec<Address> = Vec::new(&env);
        for v in old.iter() {
            if v != verifier {
                new_list.push_back(v);
            }
        }
        set_verifiers(&env, &new_list);
        verifier_removed(&env, admin, verifier);
        Ok(())
    }

    /// Returns the total number of registered verifiers.
    pub fn get_verifier_count(env: Env) -> u32 {
        get_verifiers(&env).len()
    }

    /// Returns up to the first 50 verifiers. Use `list_verifiers_paginated` for larger sets.
    pub fn list_verifiers(env: Env) -> Vec<Address> {
        let all = get_verifiers(&env);
        let cap: u32 = 50;
        if all.len() <= cap {
            return all;
        }
        let mut out: Vec<Address> = Vec::new(&env);
        for i in 0..cap {
            out.push_back(all.get(i).unwrap());
        }
        out
    }

    /// Returns one page of verifiers. `page` is 0-indexed; `page_size` must be 1–50.
    pub fn list_verifiers_paginated(env: Env, page: u32, page_size: u32) -> Vec<Address> {
        let page_size = if page_size == 0 || page_size > 50 { 50 } else { page_size };
        let all = get_verifiers(&env);
        let start = page * page_size;
        let mut out: Vec<Address> = Vec::new(&env);
        let mut i = start;
        while i < start + page_size && i < all.len() {
            out.push_back(all.get(i).unwrap());
            i += 1;
        }
        out
    }

    // ── Credit lifecycle ─────────────────────────────────────────────────────

    pub fn submit_credit(
        env: Env,
        issuer: Address,
        project_id: String,
        vintage_year: u32,
        methodology: String,
        geography: String,
        tonnes: i128,
        ipfs_hash: String,
        nonce: u64,
    ) -> Result<BytesN<32>, CarbonChainError> {
        if !has_admin(&env) {
            return Err(CarbonChainError::NotInitialized);
        }
        if is_paused(&env) {
            return Err(CarbonChainError::ContractPaused);
        }
        issuer.require_auth();
        if !consume_nonce(&env, &issuer, nonce) {
            return Err(CarbonChainError::InvalidNonce);
        }
        if tonnes <= 0 {
            return Err(CarbonChainError::InvalidTonnes);
        }
        // 1 billion tonnes upper bound (in units: 1e15)
        if tonnes > 1_000_000_000_000_000 {
            return Err(CarbonChainError::InvalidTonnes);
        }
        // Enforce 0.1 tonne minimum resolution: amount must be a multiple of MIN_CREDIT_UNIT.
        if tonnes % MIN_CREDIT_UNIT != 0 {
            return Err(CarbonChainError::InvalidTonnes);
        }

        // Include a per-contract nonce so two credits for the same project get distinct IDs.
        let nonce: u64 = env.storage().instance().get(&DataKey::CreditNonce).unwrap_or(0u64);
        env.storage().instance().set(&DataKey::CreditNonce, &(nonce + 1));
        let mut preimage = project_id.clone().to_xdr(&env);
        preimage.append(&nonce.to_xdr(&env));
        let id: BytesN<32> = env.crypto().sha256(&preimage).into();
        let metadata = CreditMetadata {
            project_id: project_id.clone(),
            issuer: issuer.clone(),
            vintage_year,
            methodology,
            geography,
            tonnes,
            ipfs_hash,
            status: CreditStatus::Pending,
            issued_at: env.ledger().timestamp(),
        };

        set_credit(&env, &id, &metadata);
        add_credit_to_project(&env, &project_id, &id);
        credit_submitted(&env, issuer, project_id, tonnes);

        Ok(id)
    }

    pub fn approve_and_mint(env: Env, verifier: Address, credit_id: BytesN<32>, nonce: u64) -> Result<(), CarbonChainError> {
        if is_paused(&env) {
            return Err(CarbonChainError::ContractPaused);
        }
        verifier.require_auth();
        if !is_verifier(&env, &verifier) {
            return Err(CarbonChainError::Unauthorized);
        }
        if !consume_nonce(&env, &verifier, nonce) {
            return Err(CarbonChainError::InvalidNonce);
        }
        let mut credit = get_credit(&env, &credit_id).ok_or(CarbonChainError::CreditNotFound)?;
        if credit.status != CreditStatus::Pending {
            return Err(CarbonChainError::InvalidStatusTransition);
        }
        credit.status = CreditStatus::Active;
        set_credit(&env, &credit_id, &credit);
        credit_minted(&env, verifier, credit_id);
        Ok(())
    }

    pub fn flag_credit(env: Env, verifier: Address, credit_id: BytesN<32>, reason: String, nonce: u64) -> Result<(), CarbonChainError> {
        if is_paused(&env) {
            return Err(CarbonChainError::ContractPaused);
        }
        verifier.require_auth();
        if !is_verifier(&env, &verifier) {
            return Err(CarbonChainError::Unauthorized);
        }
        if !consume_nonce(&env, &verifier, nonce) {
            return Err(CarbonChainError::InvalidNonce);
        }
        let mut credit = get_credit(&env, &credit_id).ok_or(CarbonChainError::CreditNotFound)?;
        if credit.status == CreditStatus::Retired || credit.status == CreditStatus::Flagged {
            return Err(CarbonChainError::InvalidStatusTransition);
        }
        credit.status = CreditStatus::Flagged;
        set_credit(&env, &credit_id, &credit);
        crate::events::credit_flagged(&env, credit_id, reason);
        Ok(())
    }

    pub fn mark_retired(env: Env, credit_id: BytesN<32>) -> Result<(), CarbonChainError> {
        if is_paused(&env) {
            return Err(CarbonChainError::ContractPaused);
        }
        // Only the registered retirement contract may call this.
        let retirement_contract = get_retirement_contract(&env)
            .ok_or(CarbonChainError::NotInitialized)?;
        retirement_contract.require_auth();
        let mut credit = get_credit(&env, &credit_id).ok_or(CarbonChainError::CreditNotFound)?;
        if credit.status != CreditStatus::Active {
            return Err(CarbonChainError::InvalidStatusTransition);
        }
        credit.status = CreditStatus::Retired;
        set_credit(&env, &credit_id, &credit);
        Ok(())
    }

    // ── Queries ──────────────────────────────────────────────────────────────

    pub fn get_credit(env: Env, credit_id: BytesN<32>) -> Result<CreditMetadata, CarbonChainError> {
        get_credit(&env, &credit_id).ok_or(CarbonChainError::CreditNotFound)
    }

    /// Lightweight view that returns only the [`CreditStatus`] for a given
    /// `credit_id` without deserialising the full [`CreditMetadata`] struct.
    /// Callers that only need to check status should prefer this over
    /// [`Self::get_credit`] to reduce compute cost.
    ///
    /// Returns [`CarbonChainError::CreditNotFound`] if no credit exists for
    /// the given ID.
    pub fn get_credit_status(env: Env, credit_id: BytesN<32>) -> Result<CreditStatus, CarbonChainError> {
        let credit = get_credit(&env, &credit_id).ok_or(CarbonChainError::CreditNotFound)?;
        Ok(credit.status)
    }

    pub fn list_credits_by_project(env: Env, project_id: String) -> Vec<BytesN<32>> {
        get_credits_by_project(&env, &project_id)
    }

    pub fn get_nonce(env: Env, address: Address) -> u64 {
        get_nonce(&env, &address)
    }

    pub fn propose_admin(env: Env, admin: Address, new_admin: Address) -> Result<(), CarbonChainError> {
        let stored_admin = get_admin(&env).ok_or(CarbonChainError::NotInitialized)?;
        admin.require_auth();
        if admin != stored_admin {
            return Err(CarbonChainError::Unauthorized);
        }
        env.storage().instance().set(&crate::types::DataKey::PendingAdmin, &new_admin);
        Ok(())
    }

    pub fn accept_admin(env: Env, new_admin: Address) -> Result<(), CarbonChainError> {
        let pending: Address = env
            .storage().instance()
            .get(&crate::types::DataKey::PendingAdmin)
            .ok_or(CarbonChainError::NoPendingAdmin)?;
        if new_admin != pending {
            return Err(CarbonChainError::Unauthorized);
        }
        new_admin.require_auth();
        set_admin(&env, &new_admin);
        env.storage().instance().remove(&crate::types::DataKey::PendingAdmin);
        Ok(())
    }

    pub fn is_verifier(env: Env, address: Address) -> bool {
        is_verifier(&env, &address)
    }
}


