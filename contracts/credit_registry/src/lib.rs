#![no_std]
use soroban_sdk::{contract, contractimpl, Env, Address, String, BytesN, Vec, symbol_short};
use soroban_sdk::xdr::ToXdr;

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
    get_nonce, consume_nonce,
    get_verifier_reputation, set_verifier_reputation,
    increment_approval_count, increment_dispute_count,
    get_issuers, set_issuers, is_issuer as storage_is_issuer,
    get_methodologies, set_methodologies, is_methodology_valid,
};
use crate::types::{CreditMetadata, CreditStatus, DataKey, ServiceType, VerifierReputation, Methodology};
use crate::events::{
    credit_submitted, credit_minted, verifier_added, verifier_removed,
    contract_paused, contract_unpaused, credit_transferred, credit_split, batch_retired,
};

fn get_nonce(env: &Env, addr: &Address) -> u64 {
    env.storage().persistent().get(&DataKey::Nonce(addr.clone())).unwrap_or(0u64)
}

fn consume_nonce(env: &Env, addr: &Address, expected: u64) -> bool {
    let current = get_nonce(env, addr);
    if current != expected { return false; }
    let key = DataKey::Nonce(addr.clone());
    env.storage().persistent().set(&key, &(current + 1));
    env.storage().persistent().extend_ttl(&key, storage::TTL_THRESHOLD, storage::MIN_TTL);
    true
}

#[cfg(not(feature = "library"))]
#[contract]
pub struct CreditRegistry;

#[cfg(not(feature = "library"))]
#[contractimpl]
impl CreditRegistry {
    // ── Admin ────────────────────────────────────────────────────────────────

    /// Initialise the registry. Must be called exactly once.
    ///
    /// # Errors
    /// - [`CarbonChainError::AlreadyInitialized`] — contract has already been initialised.
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

    /// Pause all state-mutating operations. Only the admin may call this.
    ///
    /// # Errors
    /// - [`CarbonChainError::NotInitialized`] — contract has not been initialised.
    /// - [`CarbonChainError::Unauthorized`] — caller is not the admin.
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

    /// Resume all state-mutating operations. Only the admin may call this.
    ///
    /// # Errors
    /// - [`CarbonChainError::NotInitialized`] — contract has not been initialised.
    /// - [`CarbonChainError::Unauthorized`] — caller is not the admin.
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

    /// Returns `true` if the contract is currently paused.
    pub fn paused(env: Env) -> bool {
        is_paused(&env)
    }

    // ── Verifier management ──────────────────────────────────────────────────

    /// Add a verifier to the authorised set. Requires a valid admin nonce for replay protection.
    ///
    /// # Errors
    /// - [`CarbonChainError::NotInitialized`] — contract has not been initialised.
    /// - [`CarbonChainError::Unauthorized`] — caller is not the admin.
    /// - [`CarbonChainError::InvalidNonce`] — `nonce` does not match the current admin nonce.
    /// - [`CarbonChainError::VerifierAlreadyExists`] — `verifier` is already registered.
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

    /// Remove a verifier from the authorised set. Requires a valid admin nonce.
    ///
    /// # Errors
    /// - [`CarbonChainError::NotInitialized`] — contract has not been initialised.
    /// - [`CarbonChainError::Unauthorized`] — caller is not the admin.
    /// - [`CarbonChainError::InvalidNonce`] — `nonce` does not match the current admin nonce.
    /// - [`CarbonChainError::VerifierNotFound`] — `verifier` is not in the registered set.
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

    // ── Issuer management ────────────────────────────────────────────────────

    pub fn register_issuer(env: Env, admin: Address, issuer: Address, nonce: u64) -> Result<(), CarbonChainError> {
        let stored_admin = get_admin(&env).ok_or(CarbonChainError::NotInitialized)?;
        admin.require_auth();
        if admin != stored_admin {
            return Err(CarbonChainError::Unauthorized);
        }
        if !consume_nonce(&env, &admin, nonce) {
            return Err(CarbonChainError::InvalidNonce);
        }
        let mut issuers = get_issuers(&env);
        if issuers.contains(&issuer) {
            return Err(CarbonChainError::IssuerNotAllowed);
        }
        issuers.push_back(issuer);
        set_issuers(&env, &issuers);
        Ok(())
    }

    pub fn remove_issuer(env: Env, admin: Address, issuer: Address, nonce: u64) -> Result<(), CarbonChainError> {
        let stored_admin = get_admin(&env).ok_or(CarbonChainError::NotInitialized)?;
        admin.require_auth();
        if admin != stored_admin {
            return Err(CarbonChainError::Unauthorized);
        }
        if !consume_nonce(&env, &admin, nonce) {
            return Err(CarbonChainError::InvalidNonce);
        }
        let old = get_issuers(&env);
        let mut new_list: Vec<Address> = Vec::new(&env);
        for i in old.iter() {
            if i != issuer {
                new_list.push_back(i);
            }
        }
        set_issuers(&env, &new_list);
        Ok(())
    }

    pub fn list_issuers(env: Env) -> Vec<Address> {
        get_issuers(&env)
    }

    // ── Methodology management ───────────────────────────────────────────────

    pub fn register_methodology(env: Env, admin: Address, code: String, name: String, nonce: u64) -> Result<(), CarbonChainError> {
        let stored_admin = get_admin(&env).ok_or(CarbonChainError::NotInitialized)?;
        admin.require_auth();
        if admin != stored_admin {
            return Err(CarbonChainError::Unauthorized);
        }
        if !consume_nonce(&env, &admin, nonce) {
            return Err(CarbonChainError::InvalidNonce);
        }
        let mut methodologies = get_methodologies(&env);
        for m in methodologies.iter() {
            if m.code == code {
                return Err(CarbonChainError::InvalidMetadata);
            }
        }
        methodologies.push_back(Methodology { code, name });
        set_methodologies(&env, &methodologies);
        Ok(())
    }

    pub fn list_methodologies(env: Env) -> Vec<Methodology> {
        get_methodologies(&env)
    }

    // ── Credit lifecycle ─────────────────────────────────────────────────────

    /// Submit a new carbon credit for verifier approval.
    ///
    /// Stores the credit with [`CreditStatus::Pending`] and returns its deterministic ID
    /// (SHA-256 of `project_id || internal_nonce`). The credit cannot be traded or retired
    /// until a registered verifier calls [`approve_and_mint`].
    ///
    /// `tonnes` is expressed in kg units (1 tonne = 1 000 000). Valid range: `1..=1_000_000_000_000_000`.
    ///
    /// # Errors
    /// - [`CarbonChainError::NotInitialized`] — contract has not been initialised.
    /// - [`CarbonChainError::ContractPaused`] — contract is paused.
    /// - [`CarbonChainError::InvalidNonce`] — `nonce` does not match the current issuer nonce.
    /// - [`CarbonChainError::InvalidTonnes`] — `tonnes` is zero, negative, or exceeds the upper bound.
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
        // Validate project exists
        if env.storage().persistent().get::<_, ProjectMetadata>(&DataKey::Project(project_id.clone())).is_none() {
            return Err(CarbonChainError::ProjectNotFound);
        }
        if !storage_is_issuer(&env, &issuer) {
            return Err(CarbonChainError::IssuerNotAllowed);
        }
        if !is_methodology_valid(&env, &methodology) {
            return Err(CarbonChainError::InvalidMethodology);
        }
        if tonnes <= 0 {
            return Err(CarbonChainError::InvalidTonnes);
        }
        // 1 billion tonnes upper bound (1_000_000_000 * TONNES_SCALE = 1e15)
        if tonnes > 1_000_000_000_000_000 {
            return Err(CarbonChainError::InvalidTonnes);
        }
        // Validate vintage_year: 1990 to current_year + 1
        let current_year = (env.ledger().timestamp() / 31_536_000) as u32 + 1970;
        if vintage_year < 1990 || vintage_year > current_year + 1 {
            return Err(CarbonChainError::InvalidMetadata);
        }
        // Validate geography: minimum 2 characters (ISO 3166-1 alpha-2)
        if geography.len() < 2 {
            return Err(CarbonChainError::InvalidMetadata);
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
            owner: issuer.clone(),
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
        credit_submitted(&env, issuer, project_id, id.clone(), tonnes);

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
        increment_approval_count(&env, &verifier);
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
        increment_dispute_count(&env, &verifier);
        crate::events::credit_flagged(&env, credit_id, reason);
        Ok(())
    }

    /// Mark a credit as retired. Only callable by the registered retirement contract.
    ///
    /// This is an internal cross-contract call made by the retirement contract after
    /// recording the retirement receipt. The credit must be [`CreditStatus::Active`].
    ///
    /// # Errors
    /// - [`CarbonChainError::ContractPaused`] — contract is paused.
    /// - [`CarbonChainError::NotInitialized`] — no retirement contract has been registered.
    /// - [`CarbonChainError::CreditNotFound`] — no credit exists for `credit_id`.
    /// - [`CarbonChainError::InvalidStatusTransition`] — credit is not in `Active` status.
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

    // ── Issue #85: Credit Transfer ───────────────────────────────────────────

    pub fn transfer_credit(env: Env, from: Address, to: Address, credit_id: BytesN<32>, nonce: u64) -> Result<(), CarbonChainError> {
        if is_paused(&env) {
            return Err(CarbonChainError::ContractPaused);
        }
        from.require_auth();
        if !consume_nonce(&env, &from, nonce) {
            return Err(CarbonChainError::InvalidNonce);
        }
        let mut credit = get_credit(&env, &credit_id).ok_or(CarbonChainError::CreditNotFound)?;
        if credit.owner != from {
            return Err(CarbonChainError::Unauthorized);
        }
        credit.owner = to.clone();
        set_credit(&env, &credit_id, &credit);
        credit_transferred(&env, from, to, credit_id);
        Ok(())
    }

    // ── Issue #87: Credit Splitting ──────────────────────────────────────────

    pub fn split_credit(env: Env, caller: Address, credit_id: BytesN<32>, split_tonnes: i128, nonce: u64) -> Result<(BytesN<32>, BytesN<32>), CarbonChainError> {
        if is_paused(&env) {
            return Err(CarbonChainError::ContractPaused);
        }
        caller.require_auth();
        if !consume_nonce(&env, &caller, nonce) {
            return Err(CarbonChainError::InvalidNonce);
        }
        let mut original = get_credit(&env, &credit_id).ok_or(CarbonChainError::CreditNotFound)?;
        if original.owner != caller {
            return Err(CarbonChainError::Unauthorized);
        }
        if split_tonnes <= 0 || split_tonnes >= original.tonnes {
            return Err(CarbonChainError::InvalidSplit);
        }

        let remaining_tonnes = original.tonnes - split_tonnes;
        
        // Generate IDs for child credits
        let nonce_val: u64 = env.storage().instance().get(&DataKey::CreditNonce).unwrap_or(0u64);
        env.storage().instance().set(&DataKey::CreditNonce, &(nonce_val + 1));
        let mut preimage1 = credit_id.clone().to_xdr(&env);
        preimage1.append(&nonce_val.to_xdr(&env));
        let child1_id: BytesN<32> = env.crypto().sha256(&preimage1).into();

        let nonce_val2: u64 = env.storage().instance().get(&DataKey::CreditNonce).unwrap_or(0u64);
        env.storage().instance().set(&DataKey::CreditNonce, &(nonce_val2 + 1));
        let mut preimage2 = credit_id.clone().to_xdr(&env);
        preimage2.append(&nonce_val2.to_xdr(&env));
        let child2_id: BytesN<32> = env.crypto().sha256(&preimage2).into();

        // Create child credits with same metadata
        let mut child1 = original.clone();
        child1.tonnes = split_tonnes;
        child1.owner = caller.clone();
        set_credit(&env, &child1_id, &child1);
        add_credit_to_project(&env, &original.project_id, &child1_id);

        let mut child2 = original.clone();
        child2.tonnes = remaining_tonnes;
        child2.owner = caller.clone();
        set_credit(&env, &child2_id, &child2);
        add_credit_to_project(&env, &original.project_id, &child2_id);

        // Retire original credit
        original.status = CreditStatus::Retired;
        set_credit(&env, &credit_id, &original);

        credit_split(&env, credit_id, child1_id.clone(), child2_id.clone());
        Ok((child1_id, child2_id))
    }

    // ── Queries ──────────────────────────────────────────────────────────────

    /// Fetch full metadata for a credit by its ID.
    ///
    /// # Errors
    /// - [`CarbonChainError::CreditNotFound`] — no credit exists for `credit_id`.
    pub fn get_credit(env: Env, credit_id: BytesN<32>) -> Result<CreditMetadata, CarbonChainError> {
        get_credit(&env, &credit_id).ok_or(CarbonChainError::CreditNotFound)
    }

    /// Returns all credit IDs registered under `project_id`.
    pub fn list_credits_by_project(env: Env, project_id: String) -> Vec<BytesN<32>> {
        get_credits_by_project(&env, &project_id)
    }

    /// Returns the current replay-protection nonce for `address`.
    /// Pass this value as the `nonce` argument to the next state-mutating call.
    pub fn get_nonce(env: Env, address: Address) -> u64 {
        get_nonce(&env, &address)
    }

    // ── Issue #84: Verifier Reputation ───────────────────────────────────────

    pub fn get_verifier_reputation(env: Env, verifier: Address) -> VerifierReputation {
        get_verifier_reputation(&env, &verifier)
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

    /// Complete an admin transfer initiated by [`propose_admin`].
    /// `new_admin` must match the pending candidate.
    ///
    /// # Errors
    /// - [`CarbonChainError::NoPendingAdmin`] — no transfer has been proposed.
    /// - [`CarbonChainError::Unauthorized`] — `new_admin` does not match the pending candidate.
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

    /// Returns `true` if `address` is a registered verifier.
    pub fn is_verifier(env: Env, address: Address) -> bool {
        is_verifier(&env, &address)
    }

    // ── Verifier Services ────────────────────────────────────────────────────

    /// Replace all capabilities for a verifier. This overwrites any existing services.
    pub fn configure_verifier_services(env: Env, admin: Address, verifier: Address, services: Vec<ServiceType>, nonce: u64) -> Result<(), CarbonChainError> {
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
        env.storage().persistent().set(&DataKey::VerifierServices(verifier), &services);
        Ok(())
    }

    /// Add a single service to a verifier's capabilities.
    pub fn add_verifier_service(env: Env, admin: Address, verifier: Address, service: ServiceType, nonce: u64) -> Result<(), CarbonChainError> {
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
        
        let mut services: Vec<ServiceType> = env.storage().persistent()
            .get(&DataKey::VerifierServices(verifier.clone()))
            .unwrap_or_else(|| Vec::new(&env));
        
        if !services.contains(&service) {
            services.push_back(service);
            env.storage().persistent().set(&DataKey::VerifierServices(verifier), &services);
        }
        Ok(())
    }

    /// Remove a single service from a verifier's capabilities.
    pub fn remove_verifier_service(env: Env, admin: Address, verifier: Address, service: ServiceType, nonce: u64) -> Result<(), CarbonChainError> {
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
        
        let old_services: Vec<ServiceType> = env.storage().persistent()
            .get(&DataKey::VerifierServices(verifier.clone()))
            .unwrap_or_else(|| Vec::new(&env));
        
        let mut new_services: Vec<ServiceType> = Vec::new(&env);
        for s in old_services.iter() {
            if s != service {
                new_services.push_back(s);
            }
        }
        env.storage().persistent().set(&DataKey::VerifierServices(verifier), &new_services);
        Ok(())
    }

    pub fn get_verifier_services(env: Env, verifier: Address) -> Vec<ServiceType> {
        env.storage().persistent()
            .get(&DataKey::VerifierServices(verifier))
            .unwrap_or_else(|| Vec::new(&env))
    }

    // ── Issue #91: Project Registry ──────────────────────────────────────────

    pub fn register_project(
        env: Env,
        owner: Address,
        project_id: String,
        name: String,
        description: String,
        location: String,
    ) -> Result<(), CarbonChainError> {
        owner.require_auth();
        if env.storage().persistent().get::<_, ProjectMetadata>(&DataKey::Project(project_id.clone())).is_some() {
            return Err(CarbonChainError::ProjectAlreadyExists);
        }
        let metadata = ProjectMetadata {
            owner: owner.clone(),
            name,
            description,
            location,
            created_at: env.ledger().timestamp(),
        };
        env.storage().persistent().set(&DataKey::Project(project_id.clone()), &metadata);
        env.events().publish((symbol_short!("proj_reg"), owner), project_id);
        Ok(())
    }

    pub fn get_project(env: Env, project_id: String) -> Result<ProjectMetadata, CarbonChainError> {
        env.storage().persistent()
            .get(&DataKey::Project(project_id))
            .ok_or(CarbonChainError::ProjectNotFound)
    }

    // ── Issue #90: Vintage Year Expiry ───────────────────────────────────────

    pub fn expire_credit(env: Env, admin: Address, credit_id: BytesN<32>) -> Result<(), CarbonChainError> {
        let stored_admin = get_admin(&env).ok_or(CarbonChainError::NotInitialized)?;
        admin.require_auth();
        if admin != stored_admin {
            return Err(CarbonChainError::Unauthorized);
        }
        let mut credit = get_credit(&env, &credit_id).ok_or(CarbonChainError::CreditNotFound)?;
        if credit.status == CreditStatus::Retired || credit.status == CreditStatus::Expired {
            return Err(CarbonChainError::InvalidStatusTransition);
        }
        credit.status = CreditStatus::Expired;
        set_credit(&env, &credit_id, &credit);
        env.events().publish((symbol_short!("expired"),), credit_id);
        Ok(())
    }

    pub fn get_expired_credits(env: Env, project_id: String) -> Vec<BytesN<32>> {
        let credit_ids = get_credits_by_project(&env, &project_id);
        let mut expired: Vec<BytesN<32>> = Vec::new(&env);
        for id in credit_ids.iter() {
            if let Some(credit) = get_credit(&env, &id) {
                if credit.status == CreditStatus::Expired {
                    expired.push_back(id);
                }
            }
        }
        expired
    }

    // ── Issue #89: Verifier Dispute Resolution ───────────────────────────────

    pub fn dispute_credit(
        env: Env,
        disputer: Address,
        credit_id: BytesN<32>,
        evidence_ipfs_hash: String,
    ) -> Result<(), CarbonChainError> {
        disputer.require_auth();
        let mut credit = get_credit(&env, &credit_id).ok_or(CarbonChainError::CreditNotFound)?;
        if credit.status == CreditStatus::Retired || credit.status == CreditStatus::Disputed {
            return Err(CarbonChainError::InvalidStatusTransition);
        }
        credit.status = CreditStatus::Disputed;
        set_credit(&env, &credit_id, &credit);
        env.storage().persistent().set(&DataKey::Dispute(credit_id.clone()), &evidence_ipfs_hash);
        env.events().publish((symbol_short!("dispute"), disputer), (credit_id, evidence_ipfs_hash));
        Ok(())
    }

    pub fn resolve_dispute(
        env: Env,
        admin: Address,
        credit_id: BytesN<32>,
        outcome: u32,
    ) -> Result<(), CarbonChainError> {
        let stored_admin = get_admin(&env).ok_or(CarbonChainError::NotInitialized)?;
        admin.require_auth();
        if admin != stored_admin {
            return Err(CarbonChainError::Unauthorized);
        }
        let mut credit = get_credit(&env, &credit_id).ok_or(CarbonChainError::CreditNotFound)?;
        if credit.status != CreditStatus::Disputed {
            return Err(CarbonChainError::InvalidDisputeStatus);
        }
        if outcome == 0 {
            credit.status = CreditStatus::Active;
        } else if outcome == 1 {
            credit.status = CreditStatus::Flagged;
        } else {
            return Err(CarbonChainError::InvalidMetadata);
        }
        set_credit(&env, &credit_id, &credit);
        env.storage().persistent().remove(&DataKey::Dispute(credit_id.clone()));
        env.events().publish((symbol_short!("resolved"),), (credit_id, outcome));
        Ok(())
    }

    // ── Issue #88: Credit Merging ────────────────────────────────────────────

    pub fn merge_credits(
        env: Env,
        caller: Address,
        credit_ids: Vec<BytesN<32>>,
    ) -> Result<BytesN<32>, CarbonChainError> {
        caller.require_auth();
        if credit_ids.len() < 2 {
            return Err(CarbonChainError::InvalidMetadata);
        }

        let mut total_tonnes: i128 = 0;
        let mut project_id: Option<String> = None;
        let mut vintage_year: Option<u32> = None;
        let mut issuer: Option<Address> = None;
        let mut methodology: Option<String> = None;
        let mut geography: Option<String> = None;
        let mut ipfs_hash: Option<String> = None;

        for id in credit_ids.iter() {
            let credit = get_credit(&env, &id).ok_or(CarbonChainError::CreditNotFound)?;
            
            if credit.owner != caller {
                return Err(CarbonChainError::Unauthorized);
            }

            if credit.status != CreditStatus::Active {
                return Err(CarbonChainError::InvalidStatusTransition);
            }

            if let Some(ref pid) = project_id {
                if credit.project_id != *pid {
                    return Err(CarbonChainError::InvalidMetadata);
                }
            } else {
                project_id = Some(credit.project_id.clone());
            }

            if let Some(vy) = vintage_year {
                if credit.vintage_year != vy {
                    return Err(CarbonChainError::InvalidMetadata);
                }
            } else {
                vintage_year = Some(credit.vintage_year);
            }

            if let Some(ref iss) = issuer {
                if credit.issuer != *iss {
                    return Err(CarbonChainError::InvalidMetadata);
                }
            } else {
                issuer = Some(credit.issuer.clone());
            }

            if let Some(ref meth) = methodology {
                if credit.methodology != *meth {
                    return Err(CarbonChainError::InvalidMetadata);
                }
            } else {
                methodology = Some(credit.methodology.clone());
            }

            if let Some(ref geo) = geography {
                if credit.geography != *geo {
                    return Err(CarbonChainError::InvalidMetadata);
                }
            } else {
                geography = Some(credit.geography.clone());
            }

            ipfs_hash = Some(credit.ipfs_hash.clone());
            total_tonnes = total_tonnes.checked_add(credit.tonnes).ok_or(CarbonChainError::Overflow)?;
        }

        let nonce: u64 = env.storage().instance().get(&DataKey::CreditNonce).unwrap_or(0u64);
        env.storage().instance().set(&DataKey::CreditNonce, &(nonce + 1));
        let mut preimage = project_id.clone().unwrap().to_xdr(&env);
        preimage.append(&nonce.to_xdr(&env));
        let merged_id: BytesN<32> = env.crypto().sha256(&preimage).into();

        let merged_credit = CreditMetadata {
            project_id: project_id.unwrap(),
            issuer: issuer.unwrap(),
            owner: caller.clone(),
            vintage_year: vintage_year.unwrap(),
            methodology: methodology.unwrap(),
            geography: geography.unwrap(),
            tonnes: total_tonnes,
            ipfs_hash: ipfs_hash.unwrap(),
            status: CreditStatus::Active,
            issued_at: env.ledger().timestamp(),
        };

        set_credit(&env, &merged_id, &merged_credit);
        add_credit_to_project(&env, &merged_credit.project_id, &merged_id);

        for id in credit_ids.iter() {
            let mut credit = get_credit(&env, &id).ok_or(CarbonChainError::CreditNotFound)?;
            credit.status = CreditStatus::Retired;
            set_credit(&env, &id, &credit);
        }

        env.events().publish((symbol_short!("merged"),), (merged_id.clone(), credit_ids.len() as u32));
        Ok(merged_id)
    }
}

// ── Helper functions ─────────────────────────────────────────────────────────

fn get_nonce(env: &Env, addr: &Address) -> u64 {
    env.storage().persistent().get(&DataKey::Nonce(addr.clone())).unwrap_or(0u64)
}

fn consume_nonce(env: &Env, addr: &Address, expected: u64) -> bool {
    let current = get_nonce(env, addr);
    if current != expected { return false; }
    let key = DataKey::Nonce(addr.clone());
    env.storage().persistent().set(&key, &(current + 1));
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::testutils::Address as _;
    use soroban_sdk::{Env, String};

    fn setup() -> (Env, CreditRegistryClient<'static>, Address, Address) {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(CreditRegistry, ());
        let client = CreditRegistryClient::new(&env, &contract_id);
        let admin = Address::generate(&env);
        let verifier = Address::generate(&env);
        let retirement = Address::generate(&env);
        client.initialize(&admin, &retirement);
        (env, client, admin, verifier)
    }

    fn submit_test_credit(env: &Env, client: &CreditRegistryClient, issuer: &Address) -> BytesN<32> {
        let nonce = client.get_nonce(issuer);
        client.submit_credit(
            issuer,
            &String::from_str(env, "PROJ-001"),
            &2024,
            &String::from_str(env, "VCS"),
            &String::from_str(env, "NG"),
            &1_000_000,
            &String::from_str(env, "bafybei123"),
            &nonce,
        )
    }

    #[test]
    fn test_initialize_twice_fails() {
        let (env, client, admin, _) = setup();
        let retirement = Address::generate(&env);
        let result = client.try_initialize(&admin, &retirement);
        assert!(result.is_err());
    }

    #[test]
    fn test_register_and_list_verifier() {
        let (_env, client, admin, verifier) = setup();
        let nonce = client.get_nonce(&admin);
        client.register_verifier(&admin, &verifier, &nonce);
        let list = client.list_verifiers();
        assert_eq!(list.len(), 1);
        assert_eq!(list.get(0).unwrap(), verifier);
    }

    #[test]
    fn test_register_verifier_twice_fails() {
        let (_env, client, admin, verifier) = setup();
        let nonce = client.get_nonce(&admin);
        client.register_verifier(&admin, &verifier, &nonce);
        let nonce2 = client.get_nonce(&admin);
        let result = client.try_register_verifier(&admin, &verifier, &nonce2);
        assert!(result.is_err());
    }

    #[test]
    fn test_remove_verifier() {
        let (_env, client, admin, verifier) = setup();
        let nonce = client.get_nonce(&admin);
        client.register_verifier(&admin, &verifier, &nonce);
        let nonce2 = client.get_nonce(&admin);
        client.remove_verifier(&admin, &verifier, &nonce2);
        assert_eq!(client.list_verifiers().len(), 0);
    }

    #[test]
    fn test_submit_credit() {
        let (env, client, _, _) = setup();
        let issuer = Address::generate(&env);
        let id = submit_test_credit(&env, &client, &issuer);
        let credit = client.get_credit(&id);
        assert_eq!(credit.status, CreditStatus::Pending);
        assert_eq!(credit.tonnes, 1_000_000);
    }

    #[test]
    fn test_approve_and_mint() {
        let (env, client, admin, verifier) = setup();
        let nonce = client.get_nonce(&admin);
        client.register_verifier(&admin, &verifier, &nonce);
        let issuer = Address::generate(&env);
        let id = submit_test_credit(&env, &client, &issuer);
        let vnonce = client.get_nonce(&verifier);
        client.approve_and_mint(&verifier, &id, &vnonce);
        assert_eq!(client.get_credit(&id).status, CreditStatus::Active);
    }

    #[test]
    fn test_approve_non_pending_fails() {
        let (env, client, admin, verifier) = setup();
        let nonce = client.get_nonce(&admin);
        client.register_verifier(&admin, &verifier, &nonce);
        let issuer = Address::generate(&env);
        let id = submit_test_credit(&env, &client, &issuer);
        let vnonce = client.get_nonce(&verifier);
        client.approve_and_mint(&verifier, &id, &vnonce);
        // second approval should fail
        let vnonce2 = client.get_nonce(&verifier);
        let result = client.try_approve_and_mint(&verifier, &id, &vnonce2);
        assert!(result.is_err());
    }

    #[test]
    fn test_flag_credit() {
        let (env, client, admin, verifier) = setup();
        let nonce = client.get_nonce(&admin);
        client.register_verifier(&admin, &verifier, &nonce);
        let issuer = Address::generate(&env);
        let id = submit_test_credit(&env, &client, &issuer);
        let vnonce = client.get_nonce(&verifier);
        client.flag_credit(&verifier, &id, &String::from_str(&env, "suspicious data"), &vnonce);
        assert_eq!(client.get_credit(&id).status, CreditStatus::Flagged);
    }

    #[test]
    fn test_double_flag_fails() {
        let (env, client, admin, verifier) = setup();
        let nonce = client.get_nonce(&admin);
        client.register_verifier(&admin, &verifier, &nonce);
        let issuer = Address::generate(&env);
        let id = submit_test_credit(&env, &client, &issuer);
        let vnonce = client.get_nonce(&verifier);
        client.flag_credit(&verifier, &id, &String::from_str(&env, "first flag"), &vnonce);
        let vnonce2 = client.get_nonce(&verifier);
        let result = client.try_flag_credit(&verifier, &id, &String::from_str(&env, "second flag"), &vnonce2);
        assert!(result.is_err());
    }

    #[test]
    fn test_list_verifiers_paginated() {
        let (env, client, admin, _) = setup();
        let mut addrs = soroban_sdk::Vec::new(&env);
        for _ in 0..5u32 {
            let v = Address::generate(&env);
            let nonce = client.get_nonce(&admin);
            client.register_verifier(&admin, &v, &nonce);
            addrs.push_back(v);
        }
        // page 0, size 2 → first 2
        let p0 = client.list_verifiers_paginated(&0, &2);
        assert_eq!(p0.len(), 2);
        assert_eq!(p0.get(0).unwrap(), addrs.get(0).unwrap());
        // page 1, size 2 → next 2
        let p1 = client.list_verifiers_paginated(&1, &2);
        assert_eq!(p1.len(), 2);
        assert_eq!(p1.get(0).unwrap(), addrs.get(2).unwrap());
        // page 2, size 2 → last 1
        let p2 = client.list_verifiers_paginated(&2, &2);
        assert_eq!(p2.len(), 1);
    }

    #[test]
    fn test_mark_retired() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(CreditRegistry, ());
        let client = CreditRegistryClient::new(&env, &contract_id);
        let admin = Address::generate(&env);
        let verifier = Address::generate(&env);
        let retirement = Address::generate(&env);
        client.initialize(&admin, &retirement);
        let nonce = client.get_nonce(&admin);
        client.register_verifier(&admin, &verifier, &nonce);
        let issuer = Address::generate(&env);
        let id = submit_test_credit(&env, &client, &issuer);
        let vnonce = client.get_nonce(&verifier);
        client.approve_and_mint(&verifier, &id, &vnonce);
        client.mark_retired(&id);
        assert_eq!(client.get_credit(&id).status, CreditStatus::Retired);
    }

    #[test]
    fn test_unauthorized_mark_retired_fails() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(CreditRegistry, ());
        let client = CreditRegistryClient::new(&env, &contract_id);
        let admin = Address::generate(&env);
        let verifier = Address::generate(&env);
        let retirement = Address::generate(&env);

        client.initialize(&admin, &retirement);
        let nonce = client.get_nonce(&admin);
        client.register_verifier(&admin, &verifier, &nonce);
        let issuer = Address::generate(&env);
        let id = submit_test_credit(&env, &client, &issuer);
        let vnonce = client.get_nonce(&verifier);
        client.approve_and_mint(&verifier, &id, &vnonce);

        env.set_auths(&[]);
        let result = client.try_mark_retired(&id);
        assert!(result.is_err());
    }

    #[test]
    fn test_submit_credit_zero_tonnes_fails() {
        let (env, client, _, _) = setup();
        let issuer = Address::generate(&env);
        let nonce = client.get_nonce(&issuer);
        let result = client.try_submit_credit(
            &issuer,
            &String::from_str(&env, "PROJ-001"),
            &2024,
            &String::from_str(&env, "VCS"),
            &String::from_str(&env, "NG"),
            &0,
            &String::from_str(&env, "bafybei123"),
            &nonce,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_submit_credit_negative_tonnes_fails() {
        let (env, client, _, _) = setup();
        let issuer = Address::generate(&env);
        let nonce = client.get_nonce(&issuer);
        let result = client.try_submit_credit(
            &issuer,
            &String::from_str(&env, "PROJ-001"),
            &2024,
            &String::from_str(&env, "VCS"),
            &String::from_str(&env, "NG"),
            &-1,
            &String::from_str(&env, "bafybei123"),
            &nonce,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_submit_credit_over_upper_bound_fails() {
        let (env, client, _, _) = setup();
        let issuer = Address::generate(&env);
        let nonce = client.get_nonce(&issuer);
        let result = client.try_submit_credit(
            &issuer,
            &String::from_str(&env, "PROJ-001"),
            &2024,
            &String::from_str(&env, "VCS"),
            &String::from_str(&env, "NG"),
            &1_000_000_000_000_001,
            &String::from_str(&env, "bafybei123"),
            &nonce,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_submit_credit_at_upper_bound_succeeds() {
        let (env, client, _, _) = setup();
        let issuer = Address::generate(&env);
        let nonce = client.get_nonce(&issuer);
        let result = client.try_submit_credit(
            &issuer,
            &String::from_str(&env, "PROJ-001"),
            &2024,
            &String::from_str(&env, "VCS"),
            &String::from_str(&env, "NG"),
            &1_000_000_000_000_000,
            &String::from_str(&env, "bafybei123"),
            &nonce,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_get_credit_returns_error_for_missing_credit() {
        let (env, client, _, _) = setup();
        let fake_id = BytesN::from_array(&env, &[0u8; 32]);
        let result = client.try_get_credit(&fake_id);
        assert!(result.is_err());
    }

    #[test]
    fn test_get_credit_returns_credit_metadata() {
        let (env, client, _, _) = setup();
        let issuer = Address::generate(&env);
        let id = submit_test_credit(&env, &client, &issuer);
        let credit = client.get_credit(&id);
        assert_eq!(credit.tonnes, 1_000_000);
        assert_eq!(credit.status, CreditStatus::Pending);
    }

    #[test]
    fn test_list_credits_by_project() {
        let (env, client, _, _) = setup();
        let issuer = Address::generate(&env);
        submit_test_credit(&env, &client, &issuer);
        let ids = client.list_credits_by_project(&String::from_str(&env, "PROJ-001"));
        assert_eq!(ids.len(), 1);
    }

    #[test]
    fn test_list_credits_by_status() {
        let (env, client, admin, verifier) = setup();
        let nonce = client.get_nonce(&admin);
        client.register_verifier(&admin, &verifier, &nonce);
        
        // Submit two credits
        let issuer = Address::generate(&env);
        let id1 = submit_test_credit(&env, &client, &issuer);
        let id2 = submit_test_credit(&env, &client, &issuer);
        
        // Both should be Pending
        let pending = client.list_credits_by_status(&CreditStatus::Pending);
        assert_eq!(pending.len(), 2);
        
        // Approve one
        let vnonce = client.get_nonce(&verifier);
        client.approve_and_mint(&verifier, &id1, &vnonce);
        
        // Now one should be Active, one Pending
        let active = client.list_credits_by_status(&CreditStatus::Active);
        assert_eq!(active.len(), 1);
        assert_eq!(active.get(0).unwrap(), id1);
        
        let pending = client.list_credits_by_status(&CreditStatus::Pending);
        assert_eq!(pending.len(), 1);
        assert_eq!(pending.get(0).unwrap(), id2);
    }

    #[test]
    fn test_non_verifier_cannot_approve() {
        let (env, client, _, _) = setup();
        let issuer = Address::generate(&env);
        let id = submit_test_credit(&env, &client, &issuer);
        let fake = Address::generate(&env);
        let nonce = client.get_nonce(&fake);
        let result = client.try_approve_and_mint(&fake, &id, &nonce);
        assert!(result.is_err());
    }

    // ── Pause tests ──────────────────────────────────────────────────────────

    #[test]
    fn test_pause_blocks_submit_credit() {
        let (env, client, admin, _) = setup();
        client.pause(&admin);
        assert!(client.paused());
        let issuer = Address::generate(&env);
        let result = client.try_submit_credit(
            &issuer,
            &String::from_str(&env, "PROJ-001"),
            &2024,
            &String::from_str(&env, "VCS"),
            &String::from_str(&env, "NG"),
            &1_000_000,
            &String::from_str(&env, "bafybei123"),
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_unpause_restores_submit_credit() {
        let (env, client, admin, _) = setup();
        client.pause(&admin);
        client.unpause(&admin);
        assert!(!client.paused());
        let issuer = Address::generate(&env);
        let result = client.try_submit_credit(
            &issuer,
            &String::from_str(&env, "PROJ-001"),
            &2024,
            &String::from_str(&env, "VCS"),
            &String::from_str(&env, "NG"),
            &1_000_000,
            &String::from_str(&env, "bafybei123"),
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_pause_blocks_approve_and_mint() {
        let (env, client, admin, verifier) = setup();
        let nonce = client.get_nonce(&admin);
        client.register_verifier(&admin, &verifier, &nonce);
        let issuer = Address::generate(&env);
        let id = submit_test_credit(&env, &client, &issuer);
        client.pause(&admin);
        let vnonce = client.get_nonce(&verifier);
        assert!(client.try_approve_and_mint(&verifier, &id, &vnonce).is_err());
    }

    #[test]
    fn test_non_admin_cannot_pause() {
        let (env, client, _, _) = setup();
        let rando = Address::generate(&env);
        assert!(client.try_pause(&rando).is_err());
    }

    // ── Tests for Issue #84: Verifier Reputation ─────────────────────────────

    #[test]
    fn test_verifier_reputation_increments_on_approval() {
        let (env, client, admin, verifier) = setup();
        let nonce = client.get_nonce(&admin);
        client.register_verifier(&admin, &verifier, &nonce);
        let issuer = Address::generate(&env);
        let id = submit_test_credit(&env, &client, &issuer);
        let vnonce = client.get_nonce(&verifier);
        client.approve_and_mint(&verifier, &id, &vnonce);
        let rep = client.get_verifier_reputation(&verifier);
        assert_eq!(rep.approval_count, 1);
        assert_eq!(rep.dispute_count, 0);
    }

    #[test]
    fn test_verifier_reputation_increments_on_dispute() {
        let (env, client, admin, verifier) = setup();
        let nonce = client.get_nonce(&admin);
        client.register_verifier(&admin, &verifier, &nonce);
        let issuer = Address::generate(&env);
        let id = submit_test_credit(&env, &client, &issuer);
        let vnonce = client.get_nonce(&verifier);
        client.flag_credit(&verifier, &id, &String::from_str(&env, "fraud"), &vnonce);
        let rep = client.get_verifier_reputation(&verifier);
        assert_eq!(rep.approval_count, 0);
        assert_eq!(rep.dispute_count, 1);
    }

    // ── Tests for Issue #85: Credit Transfer ─────────────────────────────────

    #[test]
    fn test_transfer_credit_changes_owner() {
        let (env, client, _, _) = setup();
        let issuer = Address::generate(&env);
        let id = submit_test_credit(&env, &client, &issuer);
        let recipient = Address::generate(&env);
        let nonce = client.get_nonce(&issuer);
        client.transfer_credit(&issuer, &recipient, &id, &nonce);
        let credit = client.get_credit(&id).unwrap();
        assert_eq!(credit.owner, recipient);
    }

    #[test]
    fn test_transfer_credit_requires_ownership() {
        let (env, client, _, _) = setup();
        let issuer = Address::generate(&env);
        let id = submit_test_credit(&env, &client, &issuer);
        let rando = Address::generate(&env);
        let recipient = Address::generate(&env);
        let nonce = client.get_nonce(&rando);
        let result = client.try_transfer_credit(&rando, &recipient, &id, &nonce);
        assert!(result.is_err());
    }

    // ── Tests for Issue #87: Credit Splitting ───────────────────────────────

    #[test]
    fn test_split_credit_creates_two_children() {
        let (env, client, _, _) = setup();
        let issuer = Address::generate(&env);
        let id = submit_test_credit(&env, &client, &issuer);
        let nonce = client.get_nonce(&issuer);
        let (child1, child2) = client.split_credit(&issuer, &id, &500_000, &nonce).unwrap();
        
        let c1 = client.get_credit(&child1).unwrap();
        let c2 = client.get_credit(&child2).unwrap();
        assert_eq!(c1.tonnes, 500_000);
        assert_eq!(c2.tonnes, 500_000);
        assert_eq!(c1.owner, issuer);
        assert_eq!(c2.owner, issuer);
    }

    #[test]
    fn test_split_credit_retires_original() {
        let (env, client, _, _) = setup();
        let issuer = Address::generate(&env);
        let id = submit_test_credit(&env, &client, &issuer);
        let nonce = client.get_nonce(&issuer);
        client.split_credit(&issuer, &id, &500_000, &nonce).unwrap();
        
        let original = client.get_credit(&id).unwrap();
        assert_eq!(original.status, CreditStatus::Retired);
    }

    #[test]
    fn test_split_credit_invalid_split_fails() {
        let (env, client, _, _) = setup();
        let issuer = Address::generate(&env);
        let id = submit_test_credit(&env, &client, &issuer);
        let nonce = client.get_nonce(&issuer);
        let result = client.try_split_credit(&issuer, &id, &1_000_000, &nonce);
        assert!(result.is_err());
    }
}
