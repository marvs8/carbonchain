#![no_std]
use soroban_sdk::{contract, contractimpl, Env, Address, String, BytesN, Vec, Map};
use soroban_sdk::xdr::ToXdr;

pub mod types;
pub mod errors;
pub mod storage;
pub mod events;

use crate::errors::CarbonChainError;
use crate::storage::{
    set_admin, get_admin, has_admin,
    set_credit, get_credit, set_project, get_project,
    get_verifiers, set_verifiers, is_verifier,
    add_credit_to_project, get_credits_by_project,
    set_retirement_contract, get_retirement_contract,
    set_paused, is_paused, get_nonce, set_nonce, consume_nonce,
};
use crate::types::{CreditMetadata, CreditStatus, DataKey, ProjectMetadata, DisputeOutcome};
use crate::events::{
    credit_submitted, credit_minted, verifier_added, verifier_removed,
    contract_paused, contract_unpaused, credit_disputed, dispute_resolved,
    credit_expired, credits_merged, project_registered,
};

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
        // Validate project exists
        if get_project(&env, &project_id).is_none() {
            return Err(CarbonChainError::ProjectNotFound);
        }
        if tonnes <= 0 {
            return Err(CarbonChainError::InvalidTonnes);
        }
        // 1 billion tonnes upper bound (in kg units: 1e15)
        if tonnes > 1_000_000_000_000_000 {
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

    // ── Issue #91: Project Registry ──────────────────────────────────────────

    pub fn register_project(
        env: Env,
        owner: Address,
        project_id: String,
        name: String,
        description: String,
        location: String,
    ) -> Result<(), CarbonChainError> {
        if !has_admin(&env) {
            return Err(CarbonChainError::NotInitialized);
        }
        owner.require_auth();
        if get_project(&env, &project_id).is_some() {
            return Err(CarbonChainError::ProjectAlreadyExists);
        }
        let metadata = ProjectMetadata {
            owner: owner.clone(),
            name,
            description,
            location,
            created_at: env.ledger().timestamp(),
        };
        set_project(&env, &project_id, &metadata);
        project_registered(&env, project_id, owner);
        Ok(())
    }

    pub fn get_project(env: Env, project_id: String) -> Result<ProjectMetadata, CarbonChainError> {
        get_project(&env, &project_id).ok_or(CarbonChainError::ProjectNotFound)
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
        credit_expired(&env, credit_id);
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
        if !has_admin(&env) {
            return Err(CarbonChainError::NotInitialized);
        }
        disputer.require_auth();
        let mut credit = get_credit(&env, &credit_id).ok_or(CarbonChainError::CreditNotFound)?;
        if credit.status == CreditStatus::Retired || credit.status == CreditStatus::Disputed {
            return Err(CarbonChainError::InvalidStatusTransition);
        }
        credit.status = CreditStatus::Disputed;
        set_credit(&env, &credit_id, &credit);
        env.storage().persistent().set(&DataKey::Dispute(credit_id.clone()), &evidence_ipfs_hash);
        credit_disputed(&env, credit_id, disputer, evidence_ipfs_hash);
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
            // Approved - restore to Active
            credit.status = CreditStatus::Active;
        } else if outcome == 1 {
            // Rejected - mark as Flagged
            credit.status = CreditStatus::Flagged;
        } else {
            return Err(CarbonChainError::InvalidMetadata);
        }
        set_credit(&env, &credit_id, &credit);
        env.storage().persistent().remove(&DataKey::Dispute(credit_id.clone()));
        dispute_resolved(&env, credit_id, outcome);
        Ok(())
    }

    // ── Issue #88: Credit Merging ────────────────────────────────────────────

    pub fn merge_credits(
        env: Env,
        caller: Address,
        credit_ids: Vec<BytesN<32>>,
    ) -> Result<BytesN<32>, CarbonChainError> {
        if !has_admin(&env) {
            return Err(CarbonChainError::NotInitialized);
        }
        if is_paused(&env) {
            return Err(CarbonChainError::ContractPaused);
        }
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

        // Validate all credits and collect metadata
        for id in credit_ids.iter() {
            let credit = get_credit(&env, &id).ok_or(CarbonChainError::CreditNotFound)?;
            
            // Verify caller owns all credits
            if credit.issuer != caller {
                return Err(CarbonChainError::Unauthorized);
            }

            // Verify all credits are Active
            if credit.status != CreditStatus::Active {
                return Err(CarbonChainError::InvalidStatusTransition);
            }

            // Verify all credits share project_id and vintage_year
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

        // Create merged credit
        let nonce: u64 = env.storage().instance().get(&DataKey::CreditNonce).unwrap_or(0u64);
        env.storage().instance().set(&DataKey::CreditNonce, &(nonce + 1));
        let mut preimage = project_id.clone().unwrap().to_xdr(&env);
        preimage.append(&nonce.to_xdr(&env));
        let merged_id: BytesN<32> = env.crypto().sha256(&preimage).into();

        let merged_credit = CreditMetadata {
            project_id: project_id.unwrap(),
            issuer: issuer.unwrap(),
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

        // Retire source credits
        for id in credit_ids.iter() {
            let mut credit = get_credit(&env, &id).ok_or(CarbonChainError::CreditNotFound)?;
            credit.status = CreditStatus::Retired;
            set_credit(&env, &id, &credit);
        }

        credits_merged(&env, merged_id.clone(), credit_ids.len() as u32);
        Ok(merged_id)
    }
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
        
        // Register default project for tests
        let owner = Address::generate(&env);
        client.register_project(
            &owner,
            &String::from_str(&env, "PROJ-001"),
            &String::from_str(&env, "Test Project"),
            &String::from_str(&env, "A test project"),
            &String::from_str(&env, "Nigeria"),
        );
        
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
    fn test_list_credits_by_project() {
        let (env, client, _, _) = setup();
        let issuer = Address::generate(&env);
        submit_test_credit(&env, &client, &issuer);
        let ids = client.list_credits_by_project(&String::from_str(&env, "PROJ-001"));
        assert_eq!(ids.len(), 1);
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
        let nonce = client.get_nonce(&issuer);
        let result = client.try_submit_credit(
            &issuer,
            &String::from_str(&env, "PROJ-001"),
            &2024,
            &String::from_str(&env, "VCS"),
            &String::from_str(&env, "NG"),
            &1_000_000,
            &String::from_str(&env, "bafybei123"),
            &nonce,
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
        let nonce = client.get_nonce(&issuer);
        let result = client.try_submit_credit(
            &issuer,
            &String::from_str(&env, "PROJ-001"),
            &2024,
            &String::from_str(&env, "VCS"),
            &String::from_str(&env, "NG"),
            &1_000_000,
            &String::from_str(&env, "bafybei123"),
            &nonce,
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

    // ── Issue #91: Project Registry Tests ────────────────────────────────────

    #[test]
    fn test_register_project() {
        let (env, client, _, _) = setup();
        let owner = Address::generate(&env);
        let result = client.try_register_project(
            &owner,
            &String::from_str(&env, "PROJ-001"),
            &String::from_str(&env, "Test Project"),
            &String::from_str(&env, "A test project"),
            &String::from_str(&env, "Nigeria"),
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_get_project() {
        let (env, client, _, _) = setup();
        let owner = Address::generate(&env);
        client.register_project(
            &owner,
            &String::from_str(&env, "PROJ-001"),
            &String::from_str(&env, "Test Project"),
            &String::from_str(&env, "A test project"),
            &String::from_str(&env, "Nigeria"),
        );
        let project = client.get_project(&String::from_str(&env, "PROJ-001"));
        assert_eq!(project.owner, owner);
    }

    #[test]
    fn test_register_project_twice_fails() {
        let (env, client, _, _) = setup();
        let owner = Address::generate(&env);
        let proj_id = String::from_str(&env, "PROJ-001");
        client.register_project(
            &owner,
            &proj_id,
            &String::from_str(&env, "Test Project"),
            &String::from_str(&env, "A test project"),
            &String::from_str(&env, "Nigeria"),
        );
        let result = client.try_register_project(
            &owner,
            &proj_id,
            &String::from_str(&env, "Test Project 2"),
            &String::from_str(&env, "Another test project"),
            &String::from_str(&env, "Nigeria"),
        );
        assert!(result.is_err());
    }

    // ── Issue #90: Vintage Year Expiry Tests ─────────────────────────────────

    #[test]
    fn test_expire_credit() {
        let (env, client, admin, verifier) = setup();
        let nonce = client.get_nonce(&admin);
        client.register_verifier(&admin, &verifier, &nonce);
        let issuer = Address::generate(&env);
        let id = submit_test_credit(&env, &client, &issuer);
        let vnonce = client.get_nonce(&verifier);
        client.approve_and_mint(&verifier, &id, &vnonce);
        let result = client.try_expire_credit(&admin, &id);
        assert!(result.is_ok());
        assert_eq!(client.get_credit(&id).status, CreditStatus::Expired);
    }

    #[test]
    fn test_get_expired_credits() {
        let (env, client, admin, verifier) = setup();
        let nonce = client.get_nonce(&admin);
        client.register_verifier(&admin, &verifier, &nonce);
        let issuer = Address::generate(&env);
        let id = submit_test_credit(&env, &client, &issuer);
        let vnonce = client.get_nonce(&verifier);
        client.approve_and_mint(&verifier, &id, &vnonce);
        client.expire_credit(&admin, &id);
        let expired = client.get_expired_credits(&String::from_str(&env, "PROJ-001"));
        assert_eq!(expired.len(), 1);
    }

    // ── Issue #89: Verifier Dispute Resolution Tests ──────────────────────────

    #[test]
    fn test_dispute_credit() {
        let (env, client, admin, verifier) = setup();
        let nonce = client.get_nonce(&admin);
        client.register_verifier(&admin, &verifier, &nonce);
        let issuer = Address::generate(&env);
        let id = submit_test_credit(&env, &client, &issuer);
        let vnonce = client.get_nonce(&verifier);
        client.approve_and_mint(&verifier, &id, &vnonce);
        let disputer = Address::generate(&env);
        let result = client.try_dispute_credit(
            &disputer,
            &id,
            &String::from_str(&env, "bafybei_evidence"),
        );
        assert!(result.is_ok());
        assert_eq!(client.get_credit(&id).status, CreditStatus::Disputed);
    }

    #[test]
    fn test_resolve_dispute_approved() {
        let (env, client, admin, verifier) = setup();
        let nonce = client.get_nonce(&admin);
        client.register_verifier(&admin, &verifier, &nonce);
        let issuer = Address::generate(&env);
        let id = submit_test_credit(&env, &client, &issuer);
        let vnonce = client.get_nonce(&verifier);
        client.approve_and_mint(&verifier, &id, &vnonce);
        let disputer = Address::generate(&env);
        client.dispute_credit(&disputer, &id, &String::from_str(&env, "bafybei_evidence"));
        let result = client.try_resolve_dispute(&admin, &id, &0);
        assert!(result.is_ok());
        assert_eq!(client.get_credit(&id).status, CreditStatus::Active);
    }

    #[test]
    fn test_resolve_dispute_rejected() {
        let (env, client, admin, verifier) = setup();
        let nonce = client.get_nonce(&admin);
        client.register_verifier(&admin, &verifier, &nonce);
        let issuer = Address::generate(&env);
        let id = submit_test_credit(&env, &client, &issuer);
        let vnonce = client.get_nonce(&verifier);
        client.approve_and_mint(&verifier, &id, &vnonce);
        let disputer = Address::generate(&env);
        client.dispute_credit(&disputer, &id, &String::from_str(&env, "bafybei_evidence"));
        let result = client.try_resolve_dispute(&admin, &id, &1);
        assert!(result.is_ok());
        assert_eq!(client.get_credit(&id).status, CreditStatus::Flagged);
    }

    // ── Issue #88: Credit Merging Tests ──────────────────────────────────────

    #[test]
    fn test_merge_credits() {
        let (env, client, admin, verifier) = setup();
        let nonce = client.get_nonce(&admin);
        client.register_verifier(&admin, &verifier, &nonce);
        let issuer = Address::generate(&env);
        
        // Create two credits
        let id1 = submit_test_credit(&env, &client, &issuer);
        let id2 = submit_test_credit(&env, &client, &issuer);
        
        // Approve both
        let vnonce = client.get_nonce(&verifier);
        client.approve_and_mint(&verifier, &id1, &vnonce);
        let vnonce2 = client.get_nonce(&verifier);
        client.approve_and_mint(&verifier, &id2, &vnonce2);
        
        // Merge them
        let mut ids = soroban_sdk::Vec::new(&env);
        ids.push_back(id1);
        ids.push_back(id2);
        let result = client.try_merge_credits(&issuer, &ids);
        assert!(result.is_ok());
        
        let merged_id = result.unwrap();
        let merged = client.get_credit(&merged_id);
        assert_eq!(merged.tonnes, 2_000_000);
        assert_eq!(merged.status, CreditStatus::Active);
        
        // Source credits should be retired
        assert_eq!(client.get_credit(&id1).status, CreditStatus::Retired);
        assert_eq!(client.get_credit(&id2).status, CreditStatus::Retired);
    }

    #[test]
    fn test_merge_credits_different_projects_fails() {
        let (env, client, admin, verifier) = setup();
        let nonce = client.get_nonce(&admin);
        client.register_verifier(&admin, &verifier, &nonce);
        let issuer = Address::generate(&env);
        
        // Create first credit
        let nonce1 = client.get_nonce(&issuer);
        let id1 = client.submit_credit(
            &issuer,
            &String::from_str(&env, "PROJ-001"),
            &2024,
            &String::from_str(&env, "VCS"),
            &String::from_str(&env, "NG"),
            &1_000_000,
            &String::from_str(&env, "bafybei123"),
            &nonce1,
        );
        
        // Create second credit with different project
        let nonce2 = client.get_nonce(&issuer);
        let id2 = client.submit_credit(
            &issuer,
            &String::from_str(&env, "PROJ-002"),
            &2024,
            &String::from_str(&env, "VCS"),
            &String::from_str(&env, "NG"),
            &1_000_000,
            &String::from_str(&env, "bafybei456"),
            &nonce2,
        );
        
        // Approve both
        let vnonce = client.get_nonce(&verifier);
        client.approve_and_mint(&verifier, &id1, &vnonce);
        let vnonce2 = client.get_nonce(&verifier);
        client.approve_and_mint(&verifier, &id2, &vnonce2);
        
        // Try to merge - should fail
        let mut ids = soroban_sdk::Vec::new(&env);
        ids.push_back(id1);
        ids.push_back(id2);
        let result = client.try_merge_credits(&issuer, &ids);
        assert!(result.is_err());
    }
}
