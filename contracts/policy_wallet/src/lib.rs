//! Policy wallet: a Soroban custom account owned by a sub-grantee.
//!
//! The wallet holds the grant tranche that `grant_escrow` releases and only
//! lets the grantee key authorise `pay(category, payee, amount, memo_hash)`
//! within the policy the funder configured: allowlisted payees, a per-payment
//! ceiling and a spend cap per budget category. Every accepted payment emits
//! an event that carries its category, so the funder has a live ledger instead
//! of a pile of receipts to reconcile after the fact.
//!
//! Authorization flow for a payment:
//!
//! 1. the grantee signs the Soroban auth payload for `pay(...)` with their
//!    Ed25519 key and submits the transaction;
//! 2. `pay` calls `env.current_contract_address().require_auth()`, which makes
//!    the host invoke `__check_auth` below;
//! 3. `__check_auth` accepts only calls to this contract's own `pay` or
//!    `submit_evidence` and verifies the signature over the payload;
//! 4. `pay` then applies the policy checks and moves the token through the
//!    Stellar Asset Contract (`token.transfer(wallet, payee, amount)`).
//!
//! The grantee key can therefore never authorise `token.transfer` directly,
//! nor any funder-only function such as `set_policy`.
#![no_std]

use soroban_sdk::{
    auth::{Context, CustomAccountInterface},
    contract, contractclient, contracterror, contractevent, contractimpl, contracttype,
    crypto::Hash,
    symbol_short, token, Address, BytesN, Env, Symbol, Vec,
};

// Ledgers close roughly every 5 seconds: 17_280 ledgers is about one day.
const LEDGERS_PER_DAY: u32 = 17_280;
/// Extend instance storage when fewer than this many ledgers remain.
const INSTANCE_TTL_THRESHOLD: u32 = LEDGERS_PER_DAY * 7;
/// Extend instance storage to this many ledgers from now.
const INSTANCE_TTL_EXTEND_TO: u32 = LEDGERS_PER_DAY * 30;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    NotInitialized = 1,
    PayeeNotAllowed = 2,
    AmountExceedsPerTxMax = 3,
    CategoryUnknown = 4,
    CategoryCapExceeded = 5,
    InvalidAmount = 6,
    InvalidPolicy = 7,
    UnauthorizedFunction = 8,
    UnsupportedContext = 9,
    NoContexts = 10,
    PayeeAlreadyListed = 11,
    PayeeNotFound = 12,
    ForeignContract = 13,
    CategoryCannotBeRemoved = 14,
}

/// One budget line: how much may be spent under `name` and how much has been.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Category {
    pub name: Symbol,
    pub cap: i128,
    pub spent: i128,
}

/// The spending policy the funder sets for the wallet.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Policy {
    pub categories: Vec<Category>,
    pub payees: Vec<Address>,
    pub per_tx_max: i128,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Pubkey,
    Funder,
    Token,
    Policy,
    PaySeq,
}

/// Emitted for every accepted payment. `category` and `payee` are topics so a
/// funder dashboard can filter the ledger by budget line or vendor.
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Paid {
    #[topic]
    pub category: Symbol,
    #[topic]
    pub payee: Address,
    pub amount: i128,
    pub memo_hash: BytesN<32>,
    pub seq: u64,
    pub category_spent: i128,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PolicyUpdated {
    pub categories: u32,
    pub payees: u32,
    pub per_tx_max: i128,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PayeeAdded {
    #[topic]
    pub payee: Address,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PayeeRemoved {
    #[topic]
    pub payee: Address,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KeyRotated {
    pub old_pubkey: BytesN<32>,
    pub new_pubkey: BytesN<32>,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvidenceForwarded {
    #[topic]
    pub escrow: Address,
    pub grant_id: u64,
    pub milestone: u32,
    pub evidence_hash: BytesN<32>,
}

/// The slice of the escrow interface the wallet calls on the grantee's behalf.
#[contractclient(name = "EscrowClient")]
pub trait EscrowInterface {
    fn submit_evidence(env: Env, id: u64, idx: u32, evidence_hash: BytesN<32>);
}

#[contract]
pub struct PolicyWallet;

fn bump_instance(env: &Env) {
    env.storage()
        .instance()
        .extend_ttl(INSTANCE_TTL_THRESHOLD, INSTANCE_TTL_EXTEND_TO);
}

fn read_policy(env: &Env) -> Result<Policy, Error> {
    env.storage()
        .instance()
        .get(&DataKey::Policy)
        .ok_or(Error::NotInitialized)
}

fn read_funder(env: &Env) -> Result<Address, Error> {
    env.storage()
        .instance()
        .get(&DataKey::Funder)
        .ok_or(Error::NotInitialized)
}

fn contains(payees: &Vec<Address>, payee: &Address) -> bool {
    payees.iter().any(|p| p == *payee)
}

fn validate_policy(policy: &Policy) -> Result<(), Error> {
    if policy.per_tx_max <= 0 || policy.categories.is_empty() {
        return Err(Error::InvalidPolicy);
    }
    for (i, c) in policy.categories.iter().enumerate() {
        if c.cap < 0 || c.spent < 0 || c.spent > c.cap {
            return Err(Error::InvalidPolicy);
        }
        // Category names must be unique.
        for other in policy.categories.iter().skip(i + 1) {
            if other.name == c.name {
                return Err(Error::InvalidPolicy);
            }
        }
    }
    for (i, p) in policy.payees.iter().enumerate() {
        for other in policy.payees.iter().skip(i + 1) {
            if other == p {
                return Err(Error::InvalidPolicy);
            }
        }
    }
    Ok(())
}

#[contractimpl]
impl PolicyWallet {
    /// Initialise the wallet. `policy.spent` values are ignored and start at 0.
    pub fn __constructor(
        env: Env,
        grantee_pubkey: BytesN<32>,
        funder: Address,
        token: Address,
        policy: Policy,
    ) -> Result<(), Error> {
        let mut fresh = Vec::new(&env);
        for c in policy.categories.iter() {
            fresh.push_back(Category {
                name: c.name,
                cap: c.cap,
                spent: 0,
            });
        }
        let policy = Policy {
            categories: fresh,
            payees: policy.payees,
            per_tx_max: policy.per_tx_max,
        };
        validate_policy(&policy)?;
        let s = env.storage().instance();
        s.set(&DataKey::Pubkey, &grantee_pubkey);
        s.set(&DataKey::Funder, &funder);
        s.set(&DataKey::Token, &token);
        s.set(&DataKey::Policy, &policy);
        s.set(&DataKey::PaySeq, &0u64);
        bump_instance(&env);
        Ok(())
    }

    /// Pay an allowlisted payee from a budget category. Requires the wallet's
    /// own authorization, i.e. the grantee's Ed25519 signature checked by
    /// `__check_auth`.
    pub fn pay(
        env: Env,
        category: Symbol,
        payee: Address,
        amount: i128,
        memo_hash: BytesN<32>,
    ) -> Result<u64, Error> {
        env.current_contract_address().require_auth();

        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }
        let mut policy = read_policy(&env)?;
        if !contains(&policy.payees, &payee) {
            return Err(Error::PayeeNotAllowed);
        }
        if amount > policy.per_tx_max {
            return Err(Error::AmountExceedsPerTxMax);
        }
        let mut idx: Option<u32> = None;
        for (i, c) in policy.categories.iter().enumerate() {
            if c.name == category {
                idx = Some(i as u32);
                break;
            }
        }
        let idx = idx.ok_or(Error::CategoryUnknown)?;
        let mut line = policy.categories.get(idx).ok_or(Error::CategoryUnknown)?;
        let new_spent = line
            .spent
            .checked_add(amount)
            .ok_or(Error::CategoryCapExceeded)?;
        if new_spent > line.cap {
            return Err(Error::CategoryCapExceeded);
        }

        let token_addr: Address = env
            .storage()
            .instance()
            .get(&DataKey::Token)
            .ok_or(Error::NotInitialized)?;
        token::TokenClient::new(&env, &token_addr).transfer(
            &env.current_contract_address(),
            &payee,
            &amount,
        );

        line.spent = new_spent;
        policy.categories.set(idx, line);
        let seq: u64 = env
            .storage()
            .instance()
            .get(&DataKey::PaySeq)
            .unwrap_or(0)
            + 1;
        let s = env.storage().instance();
        s.set(&DataKey::Policy, &policy);
        s.set(&DataKey::PaySeq, &seq);
        bump_instance(&env);

        Paid {
            category,
            payee,
            amount,
            memo_hash,
            seq,
            category_spent: new_spent,
        }
        .publish(&env);
        Ok(seq)
    }

    /// Forward milestone evidence to the escrow on the grantee's behalf. The
    /// escrow requires the wallet's authorization, which is satisfied because
    /// the wallet is the direct invoker; the grantee's signature is checked
    /// here through `__check_auth` exactly like a payment.
    pub fn submit_evidence(
        env: Env,
        escrow: Address,
        grant_id: u64,
        idx: u32,
        evidence_hash: BytesN<32>,
    ) {
        env.current_contract_address().require_auth();
        EscrowClient::new(&env, &escrow).submit_evidence(&grant_id, &idx, &evidence_hash);
        bump_instance(&env);
        EvidenceForwarded {
            escrow,
            grant_id,
            milestone: idx,
            evidence_hash,
        }
        .publish(&env);
    }

    /// Replace the policy. Funder only. Spend already recorded under a
    /// category name is carried over so the funder can raise or lower caps
    /// without resetting the ledger.
    pub fn set_policy(env: Env, policy: Policy) -> Result<(), Error> {
        read_funder(&env)?.require_auth();
        let current = read_policy(&env)?;
        let mut merged = Vec::new(&env);
        for c in policy.categories.iter() {
            let spent = current
                .categories
                .iter()
                .find(|existing| existing.name == c.name)
                .map(|existing| existing.spent)
                .unwrap_or(0);
            merged.push_back(Category {
                name: c.name,
                cap: c.cap,
                spent,
            });
        }
        
        // Ensure no categories with spent funds were dropped
        for existing in current.categories.iter() {
            if existing.spent > 0 {
                if !merged.iter().any(|c| c.name == existing.name) {
                    return Err(Error::CategoryCannotBeRemoved);
                }
            }
        }

        let policy = Policy {
            categories: merged,
            payees: policy.payees,
            per_tx_max: policy.per_tx_max,
        };
        validate_policy(&policy)?;
        env.storage().instance().set(&DataKey::Policy, &policy);
        bump_instance(&env);
        PolicyUpdated {
            categories: policy.categories.len(),
            payees: policy.payees.len(),
            per_tx_max: policy.per_tx_max,
        }
        .publish(&env);
        Ok(())
    }

    /// Allowlist a payee. Funder only.
    pub fn add_payee(env: Env, payee: Address) -> Result<(), Error> {
        read_funder(&env)?.require_auth();
        let mut policy = read_policy(&env)?;
        if contains(&policy.payees, &payee) {
            return Err(Error::PayeeAlreadyListed);
        }
        policy.payees.push_back(payee.clone());
        env.storage().instance().set(&DataKey::Policy, &policy);
        bump_instance(&env);
        PayeeAdded { payee }.publish(&env);
        Ok(())
    }

    /// Remove a payee from the allowlist. Funder only.
    pub fn remove_payee(env: Env, payee: Address) -> Result<(), Error> {
        read_funder(&env)?.require_auth();
        let mut policy = read_policy(&env)?;
        let pos = policy
            .payees
            .iter()
            .position(|p| p == payee)
            .ok_or(Error::PayeeNotFound)?;
        policy.payees.remove(pos as u32);
        env.storage().instance().set(&DataKey::Policy, &policy);
        bump_instance(&env);
        PayeeRemoved { payee }.publish(&env);
        Ok(())
    }

    /// Replace the grantee signing key (key loss or staff change). Funder only.
    pub fn rotate_key(env: Env, new_pubkey: BytesN<32>) -> Result<(), Error> {
        read_funder(&env)?.require_auth();
        let old_pubkey: BytesN<32> = env
            .storage()
            .instance()
            .get(&DataKey::Pubkey)
            .ok_or(Error::NotInitialized)?;
        env.storage().instance().set(&DataKey::Pubkey, &new_pubkey);
        bump_instance(&env);
        KeyRotated {
            old_pubkey,
            new_pubkey,
        }
        .publish(&env);
        Ok(())
    }

    /// The current policy, including spend per category.
    pub fn policy(env: Env) -> Result<Policy, Error> {
        read_policy(&env)
    }

    /// Spend per category: the funder's live ledger.
    pub fn ledger(env: Env) -> Result<Vec<Category>, Error> {
        Ok(read_policy(&env)?.categories)
    }

    /// Number of payments accepted so far.
    pub fn payment_count(env: Env) -> u64 {
        env.storage().instance().get(&DataKey::PaySeq).unwrap_or(0)
    }

    pub fn funder(env: Env) -> Result<Address, Error> {
        read_funder(&env)
    }

    pub fn token(env: Env) -> Result<Address, Error> {
        env.storage()
            .instance()
            .get(&DataKey::Token)
            .ok_or(Error::NotInitialized)
    }

    pub fn grantee_pubkey(env: Env) -> Result<BytesN<32>, Error> {
        env.storage()
            .instance()
            .get(&DataKey::Pubkey)
            .ok_or(Error::NotInitialized)
    }

    pub fn is_payee(env: Env, payee: Address) -> Result<bool, Error> {
        let policy = read_policy(&env)?;
        Ok(contains(&policy.payees, &payee))
    }

    pub fn category_ledger(env: Env, category: Symbol) -> Result<Category, Error> {
        let policy = read_policy(&env)?;
        policy
            .categories
            .iter()
            .find(|c| c.name == category)
            .ok_or(Error::CategoryUnknown)
    }

    pub fn total_spent(env: Env) -> Result<i128, Error> {
        let policy = read_policy(&env)?;
        let total: i128 = policy.categories.iter().map(|c| c.spent).sum();
        Ok(total)
    }
}

#[contractimpl]
impl CustomAccountInterface for PolicyWallet {
    type Signature = BytesN<64>;
    type Error = Error;

    /// Called by the host whenever this wallet's address must authorize
    /// something. Only `pay` and `submit_evidence` on this very contract are
    /// acceptable, and the payload must carry the grantee's signature.
    fn __check_auth(
        env: Env,
        signature_payload: Hash<32>,
        signature: BytesN<64>,
        auth_contexts: Vec<Context>,
    ) -> Result<(), Error> {
        if auth_contexts.is_empty() {
            return Err(Error::NoContexts);
        }
        let this = env.current_contract_address();
        let allowed_pay = symbol_short!("pay");
        let allowed_submit = Symbol::new(&env, "submit_evidence");
        for context in auth_contexts.iter() {
            match context {
                Context::Contract(c) => {
                    if c.contract != this {
                        return Err(Error::ForeignContract);
                    }
                    if c.fn_name != allowed_pay && c.fn_name != allowed_submit {
                        return Err(Error::UnauthorizedFunction);
                    }
                }
                Context::CreateContractHostFn(_) | Context::CreateContractWithCtorHostFn(_) => {
                    return Err(Error::UnsupportedContext)
                }
            }
        }

        let pubkey: BytesN<32> = env
            .storage()
            .instance()
            .get(&DataKey::Pubkey)
            .ok_or(Error::NotInitialized)?;
        // The host traps if the signature does not verify.
        env.crypto()
            .ed25519_verify(&pubkey, &signature_payload.into(), &signature);
        Ok(())
    }
}

#[cfg(test)]
mod test;
