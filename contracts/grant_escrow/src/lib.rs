//! Grant escrow: milestone-based release of grant funds into a policy wallet.
//!
//! A funder creates a grant with a list of milestones, a reviewer set and a
//! quorum, then funds it in one or more tranches. The grantee wallet submits
//! evidence for a milestone; reviewers approve or reject it. When the quorum
//! is reached the milestone amount is transferred to the grantee wallet (a
//! `policy_wallet` custom account). After the deadline the funder can claw
//! back whatever was never released. Every transition emits an event.
#![no_std]

use soroban_sdk::{
    contract, contracterror, contractevent, contractimpl, contracttype, token, Address, BytesN,
    Env, Vec,
};

const LEDGERS_PER_DAY: u32 = 17_280;
const TTL_THRESHOLD: u32 = LEDGERS_PER_DAY * 7;
const TTL_EXTEND_TO: u32 = LEDGERS_PER_DAY * 30;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    GrantNotFound = 1,
    InvalidMilestones = 2,
    InvalidReviewers = 3,
    InvalidQuorum = 4,
    InvalidDeadline = 5,
    InvalidAmount = 6,
    Overfunded = 7,
    MilestoneNotFound = 8,
    InvalidState = 9,
    NotReviewer = 10,
    AlreadyApproved = 11,
    DeadlineNotPassed = 12,
    GrantClosed = 13,
    InsufficientFunds = 14,
    NothingToClawback = 15,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MilestoneState {
    /// Waiting for the grantee to submit evidence.
    Pending,
    /// Evidence submitted, waiting for reviewer quorum.
    Submitted,
    /// Quorum reached but the escrow did not hold enough funds; `release`
    /// completes it once funded.
    Approved,
    /// The last submission was rejected; the grantee may resubmit.
    Rejected,
    /// Paid out to the grantee wallet.
    Released,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Milestone {
    pub amount: i128,
    /// Hash of the milestone specification text (deliverables list).
    pub spec_hash: BytesN<32>,
    /// Due date (unix seconds), informational for reviewers.
    pub due: u64,
    pub state: MilestoneState,
    /// Hash of the latest evidence report, if any was submitted.
    pub evidence_hash: Option<BytesN<32>>,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Grant {
    pub funder: Address,
    /// The policy wallet that receives released milestones.
    pub grantee_wallet: Address,
    pub token: Address,
    pub milestones: Vec<Milestone>,
    pub reviewers: Vec<Address>,
    pub quorum: u32,
    /// Unix seconds after which the funder may claw back unreleased funds.
    pub deadline: u64,
    /// Total deposited so far.
    pub funded: i128,
    /// Total released to the grantee wallet so far.
    pub released: i128,
    /// Set once the funder has clawed back; no further transitions.
    pub closed: bool,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    NextId,
    Grant(u64),
    /// Reviewers who approved the current submission of (grant, milestone).
    Approvals(u64, u32),
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GrantCreated {
    #[topic]
    pub id: u64,
    pub funder: Address,
    pub grantee_wallet: Address,
    pub token: Address,
    pub total: i128,
    pub milestones: u32,
    pub quorum: u32,
    pub deadline: u64,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Funded {
    #[topic]
    pub id: u64,
    pub from: Address,
    pub amount: i128,
    pub funded: i128,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvidenceSubmitted {
    #[topic]
    pub id: u64,
    #[topic]
    pub idx: u32,
    pub evidence_hash: BytesN<32>,
    pub late: bool,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MilestoneApproved {
    #[topic]
    pub id: u64,
    #[topic]
    pub idx: u32,
    pub reviewer: Address,
    pub approvals: u32,
    pub quorum: u32,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MilestoneReleased {
    #[topic]
    pub id: u64,
    #[topic]
    pub idx: u32,
    pub amount: i128,
    pub to: Address,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MilestoneRejected {
    #[topic]
    pub id: u64,
    #[topic]
    pub idx: u32,
    pub reviewer: Address,
    pub reason_hash: BytesN<32>,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClawedBack {
    #[topic]
    pub id: u64,
    pub amount: i128,
    pub to: Address,
}

#[contract]
pub struct GrantEscrow;

fn grant_key(id: u64) -> DataKey {
    DataKey::Grant(id)
}

fn read_grant(env: &Env, id: u64) -> Result<Grant, Error> {
    env.storage()
        .persistent()
        .get(&grant_key(id))
        .ok_or(Error::GrantNotFound)
}

fn write_grant(env: &Env, id: u64, grant: &Grant) {
    let key = grant_key(id);
    env.storage().persistent().set(&key, grant);
    env.storage()
        .persistent()
        .extend_ttl(&key, TTL_THRESHOLD, TTL_EXTEND_TO);
    env.storage()
        .instance()
        .extend_ttl(TTL_THRESHOLD, TTL_EXTEND_TO);
}

fn read_approvals(env: &Env, id: u64, idx: u32) -> Vec<Address> {
    env.storage()
        .persistent()
        .get(&DataKey::Approvals(id, idx))
        .unwrap_or(Vec::new(env))
}

fn write_approvals(env: &Env, id: u64, idx: u32, approvals: &Vec<Address>) {
    let key = DataKey::Approvals(id, idx);
    env.storage().persistent().set(&key, approvals);
    env.storage()
        .persistent()
        .extend_ttl(&key, TTL_THRESHOLD, TTL_EXTEND_TO);
}

fn milestone(grant: &Grant, idx: u32) -> Result<Milestone, Error> {
    grant.milestones.get(idx).ok_or(Error::MilestoneNotFound)
}

fn is_reviewer(grant: &Grant, who: &Address) -> bool {
    grant.reviewers.iter().any(|r| r == *who)
}

/// Move the milestone amount to the grantee wallet and mark it released.
fn release_milestone(env: &Env, id: u64, grant: &mut Grant, idx: u32, mut m: Milestone) {
    token::TokenClient::new(env, &grant.token).transfer(
        &env.current_contract_address(),
        &grant.grantee_wallet,
        &m.amount,
    );
    m.state = MilestoneState::Released;
    grant.milestones.set(idx, m.clone());
    grant.released += m.amount;
    MilestoneReleased {
        id,
        idx,
        amount: m.amount,
        to: grant.grantee_wallet.clone(),
    }
    .publish(env);
}

#[contractimpl]
impl GrantEscrow {
    /// Create a grant. Funder only. Returns the grant id.
    pub fn create_grant(
        env: Env,
        funder: Address,
        grantee_wallet: Address,
        token: Address,
        milestones: Vec<Milestone>,
        reviewers: Vec<Address>,
        quorum: u32,
        deadline: u64,
    ) -> Result<u64, Error> {
        funder.require_auth();

        if milestones.is_empty() {
            return Err(Error::InvalidMilestones);
        }
        let mut total: i128 = 0;
        let mut clean = Vec::new(&env);
        for m in milestones.iter() {
            if m.amount <= 0 {
                return Err(Error::InvalidMilestones);
            }
            total = total.checked_add(m.amount).ok_or(Error::InvalidMilestones)?;
            clean.push_back(Milestone {
                amount: m.amount,
                spec_hash: m.spec_hash,
                due: m.due,
                state: MilestoneState::Pending,
                evidence_hash: None,
            });
        }
        if reviewers.is_empty() {
            return Err(Error::InvalidReviewers);
        }
        for (i, r) in reviewers.iter().enumerate() {
            for other in reviewers.iter().skip(i + 1) {
                if other == r {
                    return Err(Error::InvalidReviewers);
                }
            }
        }
        if quorum == 0 || quorum > reviewers.len() {
            return Err(Error::InvalidQuorum);
        }
        if deadline <= env.ledger().timestamp() {
            return Err(Error::InvalidDeadline);
        }

        let id: u64 = env.storage().instance().get(&DataKey::NextId).unwrap_or(0);
        env.storage().instance().set(&DataKey::NextId, &(id + 1));

        let grant = Grant {
            funder: funder.clone(),
            grantee_wallet: grantee_wallet.clone(),
            token: token.clone(),
            milestones: clean,
            reviewers,
            quorum,
            deadline,
            funded: 0,
            released: 0,
            closed: false,
        };
        write_grant(&env, id, &grant);
        GrantCreated {
            id,
            funder,
            grantee_wallet,
            token,
            total,
            milestones: grant.milestones.len(),
            quorum,
            deadline,
        }
        .publish(&env);
        Ok(id)
    }

    /// Deposit funds into the grant. Anyone may fund, up to the grant total.
    pub fn fund(env: Env, id: u64, from: Address, amount: i128) -> Result<(), Error> {
        from.require_auth();
        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }
        let mut grant = read_grant(&env, id)?;
        if grant.closed {
            return Err(Error::GrantClosed);
        }
        let total: i128 = grant.milestones.iter().map(|m| m.amount).sum();
        let new_funded = grant.funded.checked_add(amount).ok_or(Error::Overfunded)?;
        if new_funded > total {
            return Err(Error::Overfunded);
        }
        token::TokenClient::new(&env, &grant.token).transfer(
            &from,
            &env.current_contract_address(),
            &amount,
        );
        grant.funded = new_funded;
        write_grant(&env, id, &grant);
        Funded {
            id,
            from,
            amount,
            funded: new_funded,
        }
        .publish(&env);
        Ok(())
    }

    /// Submit (or resubmit) evidence for a milestone. Grantee wallet only.
    pub fn submit_evidence(
        env: Env,
        id: u64,
        idx: u32,
        evidence_hash: BytesN<32>,
    ) -> Result<(), Error> {
        let mut grant = read_grant(&env, id)?;
        grant.grantee_wallet.require_auth();
        if grant.closed {
            return Err(Error::GrantClosed);
        }
        let mut m = milestone(&grant, idx)?;
        match m.state {
            MilestoneState::Pending | MilestoneState::Rejected => {}
            _ => return Err(Error::InvalidState),
        }
        m.state = MilestoneState::Submitted;
        m.evidence_hash = Some(evidence_hash.clone());
        let late = env.ledger().timestamp() > m.due;
        grant.milestones.set(idx, m);
        write_grant(&env, id, &grant);
        write_approvals(&env, id, idx, &Vec::new(&env));
        EvidenceSubmitted {
            id,
            idx,
            evidence_hash,
            late,
        }
        .publish(&env);
        Ok(())
    }

    /// Approve a submitted milestone. Reviewer only, once per submission. At
    /// quorum the milestone amount is released to the grantee wallet if the
    /// escrow holds enough; otherwise it becomes `Approved` until `release`.
    pub fn approve(env: Env, id: u64, idx: u32, reviewer: Address) -> Result<(), Error> {
        reviewer.require_auth();
        let mut grant = read_grant(&env, id)?;
        if grant.closed {
            return Err(Error::GrantClosed);
        }
        if !is_reviewer(&grant, &reviewer) {
            return Err(Error::NotReviewer);
        }
        let mut m = milestone(&grant, idx)?;
        if m.state != MilestoneState::Submitted {
            return Err(Error::InvalidState);
        }
        let mut approvals = read_approvals(&env, id, idx);
        if approvals.iter().any(|a| a == reviewer) {
            return Err(Error::AlreadyApproved);
        }
        approvals.push_back(reviewer.clone());
        write_approvals(&env, id, idx, &approvals);
        MilestoneApproved {
            id,
            idx,
            reviewer,
            approvals: approvals.len(),
            quorum: grant.quorum,
        }
        .publish(&env);

        if approvals.len() >= grant.quorum {
            if grant.funded - grant.released >= m.amount {
                release_milestone(&env, id, &mut grant, idx, m);
            } else {
                m.state = MilestoneState::Approved;
                grant.milestones.set(idx, m);
            }
        }
        write_grant(&env, id, &grant);
        Ok(())
    }

    /// Complete the payout of an `Approved` milestone once funds arrived.
    /// Permissionless: the state already carries the reviewers' decision.
    pub fn release(env: Env, id: u64, idx: u32) -> Result<(), Error> {
        let mut grant = read_grant(&env, id)?;
        if grant.closed {
            return Err(Error::GrantClosed);
        }
        let m = milestone(&grant, idx)?;
        if m.state != MilestoneState::Approved {
            return Err(Error::InvalidState);
        }
        if grant.funded - grant.released < m.amount {
            return Err(Error::InsufficientFunds);
        }
        release_milestone(&env, id, &mut grant, idx, m);
        write_grant(&env, id, &grant);
        Ok(())
    }

    /// Reject a submitted milestone. Reviewer only. The grantee may resubmit.
    pub fn reject(
        env: Env,
        id: u64,
        idx: u32,
        reviewer: Address,
        reason_hash: BytesN<32>,
    ) -> Result<(), Error> {
        reviewer.require_auth();
        let mut grant = read_grant(&env, id)?;
        if grant.closed {
            return Err(Error::GrantClosed);
        }
        if !is_reviewer(&grant, &reviewer) {
            return Err(Error::NotReviewer);
        }
        let mut m = milestone(&grant, idx)?;
        if m.state != MilestoneState::Submitted {
            return Err(Error::InvalidState);
        }
        m.state = MilestoneState::Rejected;
        grant.milestones.set(idx, m);
        write_grant(&env, id, &grant);
        write_approvals(&env, id, idx, &Vec::new(&env));
        MilestoneRejected {
            id,
            idx,
            reviewer,
            reason_hash,
        }
        .publish(&env);
        Ok(())
    }

    /// After the deadline, return every unreleased token to the funder and
    /// close the grant. Funder only.
    pub fn clawback(env: Env, id: u64) -> Result<i128, Error> {
        let mut grant = read_grant(&env, id)?;
        grant.funder.require_auth();
        if grant.closed {
            return Err(Error::GrantClosed);
        }
        if env.ledger().timestamp() <= grant.deadline {
            return Err(Error::DeadlineNotPassed);
        }
        let amount = grant.funded - grant.released;
        if amount <= 0 {
            return Err(Error::NothingToClawback);
        }
        token::TokenClient::new(&env, &grant.token).transfer(
            &env.current_contract_address(),
            &grant.funder,
            &amount,
        );
        grant.funded -= amount;
        grant.closed = true;
        write_grant(&env, id, &grant);
        ClawedBack {
            id,
            amount,
            to: grant.funder.clone(),
        }
        .publish(&env);
        Ok(amount)
    }

    pub fn grant(env: Env, id: u64) -> Result<Grant, Error> {
        read_grant(&env, id)
    }

    /// Reviewers who approved the current submission of a milestone.
    pub fn approvals(env: Env, id: u64, idx: u32) -> Vec<Address> {
        read_approvals(&env, id, idx)
    }

    pub fn grant_count(env: Env) -> u64 {
        env.storage().instance().get(&DataKey::NextId).unwrap_or(0)
    }
}

#[cfg(test)]
mod test;
