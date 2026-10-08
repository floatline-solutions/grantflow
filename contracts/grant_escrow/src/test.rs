#![cfg(test)]
extern crate std;

use super::*;
use policy_wallet::{
    Category, Error as WalletError, Policy, PolicyWallet, PolicyWalletClient,
};
use soroban_sdk::{
    testutils::{Address as _, Events as _, Ledger as _, MockAuth, MockAuthInvoke},
    token::{StellarAssetClient, TokenClient},
    vec, Address, BytesN, Env, IntoVal, Symbol, Vec,
};

/// USDC has 7 decimals on Stellar: 1 cent = 100_000 stroops.
fn usdc(cents: i128) -> i128 {
    cents * 100_000
}

const DAY: u64 = 86_400;
/// 2025-10-09T06:13:20Z, an arbitrary realistic start of the grant.
const START: u64 = 1_760_000_000;

// Milestone amounts from data/seed/grant.json.
const M1: i128 = 1_250_000; // 12,500.00 USDC in cents
const M2: i128 = 1_500_000; // 15,000.00
const M3: i128 = 1_000_000; // 10,000.00

fn hash(env: &Env, n: u8) -> BytesN<32> {
    BytesN::from_array(env, &[n; 32])
}

fn seed_milestones(env: &Env) -> Vec<Milestone> {
    let mk = |cents: i128, tag: u8, due: u64| Milestone {
        amount: usdc(cents),
        spec_hash: hash(env, tag),
        due,
        state: MilestoneState::Pending,
        evidence_hash: None,
    };
    vec![
        env,
        mk(M1, 0xA1, START + 45 * DAY),
        mk(M2, 0xA2, START + 100 * DAY),
        mk(M3, 0xA3, START + 160 * DAY),
    ]
}

struct Fixture {
    funder: Address,
    token: Address,
    escrow: Address,
    wallet: Address,
    reviewers: std::vec::Vec<Address>,
    deadline: u64,
}

fn setup(env: &Env) -> Fixture {
    env.ledger().set_timestamp(START);
    // Minting needs the token admin's authorization; tests that are about
    // auth switch to `mock_auths` afterwards, which replaces this.
    env.mock_all_auths();
    let funder = Address::generate(env);
    let token_admin = Address::generate(env);
    let token = env.register_stellar_asset_contract_v2(token_admin).address();
    let escrow = env.register(GrantEscrow, ());
    let wallet = Address::generate(env);
    let reviewers: std::vec::Vec<Address> = (0..3).map(|_| Address::generate(env)).collect();
    StellarAssetClient::new(env, &token).mint(&funder, &usdc(M1 + M2 + M3));
    Fixture {
        funder,
        token,
        escrow,
        wallet,
        reviewers,
        deadline: START + 180 * DAY,
    }
}

fn reviewers_vec(env: &Env, f: &Fixture) -> Vec<Address> {
    let mut v = Vec::new(env);
    for r in &f.reviewers {
        v.push_back(r.clone());
    }
    v
}

/// Create the seed grant with a 2-of-3 reviewer quorum.
fn create(env: &Env, f: &Fixture) -> u64 {
    GrantEscrowClient::new(env, &f.escrow).create_grant(
        &f.funder,
        &f.wallet,
        &f.token,
        &seed_milestones(env),
        &reviewers_vec(env, f),
        &2,
        &f.deadline,
    )
}

fn create_and_fund(env: &Env, f: &Fixture) -> u64 {
    let id = create(env, f);
    GrantEscrowClient::new(env, &f.escrow).fund(&id, &f.funder, &usdc(M1 + M2 + M3));
    id
}

fn escrow_events(env: &Env, escrow: &Address) -> usize {
    env.events()
        .all()
        .filter_by_contract(escrow)
        .events()
        .len()
}

// ---------------------------------------------------------------------------
// create_grant
// ---------------------------------------------------------------------------

#[test]
fn create_grant_stores_the_grant_and_numbers_sequentially() {
    let env = Env::default();
    env.mock_all_auths();
    let f = setup(&env);
    let client = GrantEscrowClient::new(&env, &f.escrow);

    let id = create(&env, &f);
    assert_eq!(id, 0);
    assert_eq!(escrow_events(&env, &f.escrow), 1);
    assert_eq!(client.grant_count(), 1);

    let g = client.grant(&id);
    assert_eq!(g.funder, f.funder);
    assert_eq!(g.grantee_wallet, f.wallet);
    assert_eq!(g.token, f.token);
    assert_eq!(g.milestones.len(), 3);
    assert_eq!(g.milestones.get(0).unwrap().state, MilestoneState::Pending);
    assert_eq!(g.milestones.get(0).unwrap().evidence_hash, None);
    assert_eq!(g.reviewers.len(), 3);
    assert_eq!(g.quorum, 2);
    assert_eq!(g.deadline, f.deadline);
    assert_eq!(g.funded, 0);
    assert_eq!(g.released, 0);
    assert!(!g.closed);

    assert_eq!(create(&env, &f), 1);
    assert_eq!(client.grant_count(), 2);
    assert_eq!(client.try_grant(&7), Err(Ok(Error::GrantNotFound)));
}

#[test]
fn create_grant_ignores_caller_supplied_milestone_state() {
    let env = Env::default();
    env.mock_all_auths();
    let f = setup(&env);
    let client = GrantEscrowClient::new(&env, &f.escrow);
    let mut ms = seed_milestones(&env);
    let mut first = ms.get(0).unwrap();
    first.state = MilestoneState::Released;
    first.evidence_hash = Some(hash(&env, 1));
    ms.set(0, first);
    let id = client.create_grant(
        &f.funder,
        &f.wallet,
        &f.token,
        &ms,
        &reviewers_vec(&env, &f),
        &2,
        &f.deadline,
    );
    let m = client.grant(&id).milestones.get(0).unwrap();
    assert_eq!(m.state, MilestoneState::Pending);
    assert_eq!(m.evidence_hash, None);
}

#[test]
fn create_grant_requires_funder_authorization() {
    let env = Env::default();
    let f = setup(&env);
    let client = GrantEscrowClient::new(&env, &f.escrow);
    let ms = seed_milestones(&env);
    let rs = reviewers_vec(&env, &f);
    let stranger = Address::generate(&env);
    env.mock_auths(&[MockAuth {
        address: &stranger,
        invoke: &MockAuthInvoke {
            contract: &f.escrow,
            fn_name: "create_grant",
            args: (
                f.funder.clone(),
                f.wallet.clone(),
                f.token.clone(),
                ms.clone(),
                rs.clone(),
                2u32,
                f.deadline,
            )
                .into_val(&env),
            sub_invokes: &[],
        },
    }]);
    assert!(client
        .try_create_grant(&f.funder, &f.wallet, &f.token, &ms, &rs, &2, &f.deadline)
        .is_err());

    env.mock_auths(&[MockAuth {
        address: &f.funder,
        invoke: &MockAuthInvoke {
            contract: &f.escrow,
            fn_name: "create_grant",
            args: (
                f.funder.clone(),
                f.wallet.clone(),
                f.token.clone(),
                ms.clone(),
                rs.clone(),
                2u32,
                f.deadline,
            )
                .into_val(&env),
            sub_invokes: &[],
        },
    }]);
    client.create_grant(&f.funder, &f.wallet, &f.token, &ms, &rs, &2, &f.deadline);
    assert_eq!(env.auths()[0].0, f.funder);
}

#[test]
fn create_grant_rejects_invalid_inputs() {
    let env = Env::default();
    env.mock_all_auths();
    let f = setup(&env);
    let client = GrantEscrowClient::new(&env, &f.escrow);
    let ms = seed_milestones(&env);
    let rs = reviewers_vec(&env, &f);

    assert_eq!(
        client.try_create_grant(&f.funder, &f.wallet, &f.token, &vec![&env], &rs, &2, &f.deadline),
        Err(Ok(Error::InvalidMilestones))
    );
    let mut zero = ms.clone();
    let mut m = zero.get(1).unwrap();
    m.amount = 0;
    zero.set(1, m);
    assert_eq!(
        client.try_create_grant(&f.funder, &f.wallet, &f.token, &zero, &rs, &2, &f.deadline),
        Err(Ok(Error::InvalidMilestones))
    );
    assert_eq!(
        client.try_create_grant(&f.funder, &f.wallet, &f.token, &ms, &vec![&env], &1, &f.deadline),
        Err(Ok(Error::InvalidReviewers))
    );
    let dup = vec![&env, f.reviewers[0].clone(), f.reviewers[0].clone()];
    assert_eq!(
        client.try_create_grant(&f.funder, &f.wallet, &f.token, &ms, &dup, &1, &f.deadline),
        Err(Ok(Error::InvalidReviewers))
    );
    assert_eq!(
        client.try_create_grant(&f.funder, &f.wallet, &f.token, &ms, &rs, &0, &f.deadline),
        Err(Ok(Error::InvalidQuorum))
    );
    assert_eq!(
        client.try_create_grant(&f.funder, &f.wallet, &f.token, &ms, &rs, &4, &f.deadline),
        Err(Ok(Error::InvalidQuorum))
    );
    assert_eq!(
        client.try_create_grant(&f.funder, &f.wallet, &f.token, &ms, &rs, &2, &START),
        Err(Ok(Error::InvalidDeadline))
    );
    assert_eq!(client.grant_count(), 0);
}

// ---------------------------------------------------------------------------
// fund
// ---------------------------------------------------------------------------

#[test]
fn fund_moves_tokens_into_escrow_in_tranches() {
    let env = Env::default();
    env.mock_all_auths();
    let f = setup(&env);
    let client = GrantEscrowClient::new(&env, &f.escrow);
    let token = TokenClient::new(&env, &f.token);
    let id = create(&env, &f);

    client.fund(&id, &f.funder, &usdc(2_500_000));
    assert_eq!(escrow_events(&env, &f.escrow), 1);
    assert_eq!(client.grant(&id).funded, usdc(2_500_000));
    assert_eq!(token.balance(&f.escrow), usdc(2_500_000));

    // Anyone may top up (a co-funder), as long as the total is not exceeded.
    let cofunder = Address::generate(&env);
    StellarAssetClient::new(&env, &f.token).mint(&cofunder, &usdc(1_250_000));
    client.fund(&id, &cofunder, &usdc(1_250_000));
    assert_eq!(client.grant(&id).funded, usdc(M1 + M2 + M3));
    assert_eq!(token.balance(&f.escrow), usdc(M1 + M2 + M3));

    assert_eq!(
        client.try_fund(&id, &f.funder, &1),
        Err(Ok(Error::Overfunded))
    );
    assert_eq!(client.try_fund(&id, &f.funder, &0), Err(Ok(Error::InvalidAmount)));
    assert_eq!(
        client.try_fund(&9, &f.funder, &usdc(1)),
        Err(Ok(Error::GrantNotFound))
    );
}

#[test]
fn fund_requires_the_payers_authorization() {
    let env = Env::default();
    env.mock_all_auths();
    let f = setup(&env);
    let client = GrantEscrowClient::new(&env, &f.escrow);
    let id = create(&env, &f);
    let stranger = Address::generate(&env);
    env.mock_auths(&[MockAuth {
        address: &stranger,
        invoke: &MockAuthInvoke {
            contract: &f.escrow,
            fn_name: "fund",
            args: (id, f.funder.clone(), usdc(100)).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    assert!(client.try_fund(&id, &f.funder, &usdc(100)).is_err());
    assert_eq!(client.grant(&id).funded, 0);
}

// ---------------------------------------------------------------------------
// submit_evidence
// ---------------------------------------------------------------------------

#[test]
fn submit_evidence_marks_milestone_submitted() {
    let env = Env::default();
    env.mock_all_auths();
    let f = setup(&env);
    let client = GrantEscrowClient::new(&env, &f.escrow);
    let id = create_and_fund(&env, &f);

    client.submit_evidence(&id, &0, &hash(&env, 0xE1));
    assert_eq!(escrow_events(&env, &f.escrow), 1);
    let m = client.grant(&id).milestones.get(0).unwrap();
    assert_eq!(m.state, MilestoneState::Submitted);
    assert_eq!(m.evidence_hash, Some(hash(&env, 0xE1)));
    assert_eq!(client.approvals(&id, &0).len(), 0);

    assert_eq!(
        client.try_submit_evidence(&id, &0, &hash(&env, 0xE2)),
        Err(Ok(Error::InvalidState))
    );
    assert_eq!(
        client.try_submit_evidence(&id, &3, &hash(&env, 0xE2)),
        Err(Ok(Error::MilestoneNotFound))
    );
    assert_eq!(
        client.try_submit_evidence(&5, &0, &hash(&env, 0xE2)),
        Err(Ok(Error::GrantNotFound))
    );
}

#[test]
fn submit_evidence_after_due_date_is_accepted_but_flagged() {
    let env = Env::default();
    env.mock_all_auths();
    let f = setup(&env);
    let client = GrantEscrowClient::new(&env, &f.escrow);
    let id = create_and_fund(&env, &f);
    env.ledger().set_timestamp(START + 46 * DAY);
    client.submit_evidence(&id, &0, &hash(&env, 0xE1));
    let ev = env.events().all().filter_by_contract(&f.escrow);
    assert_eq!(ev.events().len(), 1);
    assert_eq!(
        client.grant(&id).milestones.get(0).unwrap().state,
        MilestoneState::Submitted
    );
}

#[test]
fn submit_evidence_requires_the_grantee_wallets_authorization() {
    let env = Env::default();
    env.mock_all_auths();
    let f = setup(&env);
    let client = GrantEscrowClient::new(&env, &f.escrow);
    let id = create_and_fund(&env, &f);
    let evidence = hash(&env, 0xE1);
    for who in [&f.funder, &f.reviewers[0]] {
        env.mock_auths(&[MockAuth {
            address: who,
            invoke: &MockAuthInvoke {
                contract: &f.escrow,
                fn_name: "submit_evidence",
                args: (id, 0u32, evidence.clone()).into_val(&env),
                sub_invokes: &[],
            },
        }]);
        assert!(client.try_submit_evidence(&id, &0, &evidence).is_err());
    }
    env.mock_auths(&[MockAuth {
        address: &f.wallet,
        invoke: &MockAuthInvoke {
            contract: &f.escrow,
            fn_name: "submit_evidence",
            args: (id, 0u32, evidence.clone()).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    client.submit_evidence(&id, &0, &evidence);
    assert_eq!(env.auths()[0].0, f.wallet);
}

// ---------------------------------------------------------------------------
// approve / release
// ---------------------------------------------------------------------------

#[test]
fn approve_releases_at_quorum() {
    let env = Env::default();
    env.mock_all_auths();
    let f = setup(&env);
    let client = GrantEscrowClient::new(&env, &f.escrow);
    let token = TokenClient::new(&env, &f.token);
    let id = create_and_fund(&env, &f);
    client.submit_evidence(&id, &0, &hash(&env, 0xE1));

    client.approve(&id, &0, &f.reviewers[0]);
    assert_eq!(escrow_events(&env, &f.escrow), 1);
    assert_eq!(
        client.grant(&id).milestones.get(0).unwrap().state,
        MilestoneState::Submitted
    );
    assert_eq!(client.approvals(&id, &0), vec![&env, f.reviewers[0].clone()]);
    assert_eq!(token.balance(&f.wallet), 0);

    client.approve(&id, &0, &f.reviewers[2]);
    // approved + released
    assert_eq!(escrow_events(&env, &f.escrow), 2);
    let g = client.grant(&id);
    assert_eq!(g.milestones.get(0).unwrap().state, MilestoneState::Released);
    assert_eq!(g.released, usdc(M1));
    assert_eq!(token.balance(&f.wallet), usdc(M1));
    assert_eq!(token.balance(&f.escrow), usdc(M2 + M3));
    assert_eq!(client.approvals(&id, &0).len(), 2);

    // A third approval of a released milestone is meaningless.
    assert_eq!(
        client.try_approve(&id, &0, &f.reviewers[1]),
        Err(Ok(Error::InvalidState))
    );
}

#[test]
fn approve_rejects_non_reviewers_double_votes_and_wrong_state() {
    let env = Env::default();
    env.mock_all_auths();
    let f = setup(&env);
    let client = GrantEscrowClient::new(&env, &f.escrow);
    let id = create_and_fund(&env, &f);

    assert_eq!(
        client.try_approve(&id, &0, &f.reviewers[0]),
        Err(Ok(Error::InvalidState))
    );
    client.submit_evidence(&id, &0, &hash(&env, 0xE1));
    assert_eq!(
        client.try_approve(&id, &0, &f.funder),
        Err(Ok(Error::NotReviewer))
    );
    client.approve(&id, &0, &f.reviewers[0]);
    assert_eq!(
        client.try_approve(&id, &0, &f.reviewers[0]),
        Err(Ok(Error::AlreadyApproved))
    );
    assert_eq!(
        client.try_approve(&id, &4, &f.reviewers[1]),
        Err(Ok(Error::MilestoneNotFound))
    );
}

#[test]
fn approve_requires_the_reviewers_authorization() {
    let env = Env::default();
    env.mock_all_auths();
    let f = setup(&env);
    let client = GrantEscrowClient::new(&env, &f.escrow);
    let id = create_and_fund(&env, &f);
    client.submit_evidence(&id, &0, &hash(&env, 0xE1));

    // The funder cannot vote in a reviewer's name.
    env.mock_auths(&[MockAuth {
        address: &f.funder,
        invoke: &MockAuthInvoke {
            contract: &f.escrow,
            fn_name: "approve",
            args: (id, 0u32, f.reviewers[0].clone()).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    assert!(client.try_approve(&id, &0, &f.reviewers[0]).is_err());
    assert_eq!(client.approvals(&id, &0).len(), 0);

    env.mock_auths(&[MockAuth {
        address: &f.reviewers[0],
        invoke: &MockAuthInvoke {
            contract: &f.escrow,
            fn_name: "approve",
            args: (id, 0u32, f.reviewers[0].clone()).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    client.approve(&id, &0, &f.reviewers[0]);
    assert_eq!(env.auths()[0].0, f.reviewers[0]);
}

#[test]
fn approve_without_funds_waits_for_release() {
    let env = Env::default();
    env.mock_all_auths();
    let f = setup(&env);
    let client = GrantEscrowClient::new(&env, &f.escrow);
    let token = TokenClient::new(&env, &f.token);
    let id = create(&env, &f); // not funded yet
    client.submit_evidence(&id, &0, &hash(&env, 0xE1));
    client.approve(&id, &0, &f.reviewers[0]);
    client.approve(&id, &0, &f.reviewers[1]);
    assert_eq!(
        client.grant(&id).milestones.get(0).unwrap().state,
        MilestoneState::Approved
    );
    assert_eq!(client.try_release(&id, &0), Err(Ok(Error::InsufficientFunds)));

    client.fund(&id, &f.funder, &usdc(M1));
    client.release(&id, &0);
    assert_eq!(escrow_events(&env, &f.escrow), 1);
    let g = client.grant(&id);
    assert_eq!(g.milestones.get(0).unwrap().state, MilestoneState::Released);
    assert_eq!(g.released, usdc(M1));
    assert_eq!(token.balance(&f.wallet), usdc(M1));

    assert_eq!(client.try_release(&id, &0), Err(Ok(Error::InvalidState)));
    assert_eq!(client.try_release(&id, &1), Err(Ok(Error::InvalidState)));
}

// ---------------------------------------------------------------------------
// reject
// ---------------------------------------------------------------------------

#[test]
fn reject_clears_approvals_and_allows_resubmission() {
    let env = Env::default();
    env.mock_all_auths();
    let f = setup(&env);
    let client = GrantEscrowClient::new(&env, &f.escrow);
    let id = create_and_fund(&env, &f);
    client.submit_evidence(&id, &1, &hash(&env, 0xE2));
    client.approve(&id, &1, &f.reviewers[0]);

    client.reject(&id, &1, &f.reviewers[1], &hash(&env, 0xBB));
    assert_eq!(escrow_events(&env, &f.escrow), 1);
    let m = client.grant(&id).milestones.get(1).unwrap();
    assert_eq!(m.state, MilestoneState::Rejected);
    assert_eq!(m.evidence_hash, Some(hash(&env, 0xE2)));
    assert_eq!(client.approvals(&id, &1).len(), 0);
    assert_eq!(client.grant(&id).released, 0);

    // Nothing to approve or reject until resubmission.
    assert_eq!(
        client.try_approve(&id, &1, &f.reviewers[2]),
        Err(Ok(Error::InvalidState))
    );
    assert_eq!(
        client.try_reject(&id, &1, &f.reviewers[2], &hash(&env, 0xBC)),
        Err(Ok(Error::InvalidState))
    );

    client.submit_evidence(&id, &1, &hash(&env, 0xE3));
    let m = client.grant(&id).milestones.get(1).unwrap();
    assert_eq!(m.state, MilestoneState::Submitted);
    assert_eq!(m.evidence_hash, Some(hash(&env, 0xE3)));
    // The earlier vote does not carry over: reviewer 0 votes again.
    client.approve(&id, &1, &f.reviewers[0]);
    client.approve(&id, &1, &f.reviewers[1]);
    assert_eq!(
        client.grant(&id).milestones.get(1).unwrap().state,
        MilestoneState::Released
    );
    assert_eq!(TokenClient::new(&env, &f.token).balance(&f.wallet), usdc(M2));
}

#[test]
fn reject_rejects_non_reviewers_and_requires_their_authorization() {
    let env = Env::default();
    env.mock_all_auths();
    let f = setup(&env);
    let client = GrantEscrowClient::new(&env, &f.escrow);
    let id = create_and_fund(&env, &f);
    client.submit_evidence(&id, &0, &hash(&env, 0xE1));
    assert_eq!(
        client.try_reject(&id, &0, &f.funder, &hash(&env, 0xBB)),
        Err(Ok(Error::NotReviewer))
    );
    env.mock_auths(&[MockAuth {
        address: &f.funder,
        invoke: &MockAuthInvoke {
            contract: &f.escrow,
            fn_name: "reject",
            args: (id, 0u32, f.reviewers[0].clone(), hash(&env, 0xBB)).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    assert!(client
        .try_reject(&id, &0, &f.reviewers[0], &hash(&env, 0xBB))
        .is_err());
    assert_eq!(
        client.grant(&id).milestones.get(0).unwrap().state,
        MilestoneState::Submitted
    );
}

// ---------------------------------------------------------------------------
// clawback
// ---------------------------------------------------------------------------

#[test]
fn clawback_after_deadline_returns_unreleased_funds_and_closes() {
    let env = Env::default();
    env.mock_all_auths();
    let f = setup(&env);
    let client = GrantEscrowClient::new(&env, &f.escrow);
    let token = TokenClient::new(&env, &f.token);
    let id = create_and_fund(&env, &f);
    client.submit_evidence(&id, &0, &hash(&env, 0xE1));
    client.approve(&id, &0, &f.reviewers[0]);
    client.approve(&id, &0, &f.reviewers[1]);
    client.submit_evidence(&id, &1, &hash(&env, 0xE2));

    assert_eq!(client.try_clawback(&id), Err(Ok(Error::DeadlineNotPassed)));
    env.ledger().set_timestamp(f.deadline);
    assert_eq!(client.try_clawback(&id), Err(Ok(Error::DeadlineNotPassed)));

    env.ledger().set_timestamp(f.deadline + 1);
    let returned = client.clawback(&id);
    assert_eq!(escrow_events(&env, &f.escrow), 1);
    assert_eq!(returned, usdc(M2 + M3));
    assert_eq!(token.balance(&f.funder), usdc(M2 + M3));
    assert_eq!(token.balance(&f.escrow), 0);
    let g = client.grant(&id);
    assert!(g.closed);
    assert_eq!(g.funded, usdc(M1));
    assert_eq!(g.released, usdc(M1));
    // History is kept: milestone 2 stays "Submitted" but can never be paid.
    assert_eq!(g.milestones.get(1).unwrap().state, MilestoneState::Submitted);

    assert_eq!(client.try_clawback(&id), Err(Ok(Error::GrantClosed)));
    assert_eq!(
        client.try_approve(&id, &1, &f.reviewers[0]),
        Err(Ok(Error::GrantClosed))
    );
    assert_eq!(
        client.try_reject(&id, &1, &f.reviewers[0], &hash(&env, 0xBB)),
        Err(Ok(Error::GrantClosed))
    );
    assert_eq!(
        client.try_submit_evidence(&id, &2, &hash(&env, 0xE3)),
        Err(Ok(Error::GrantClosed))
    );
    assert_eq!(
        client.try_fund(&id, &f.funder, &usdc(1)),
        Err(Ok(Error::GrantClosed))
    );
    assert_eq!(client.try_release(&id, &1), Err(Ok(Error::GrantClosed)));
}

#[test]
fn clawback_with_everything_released_has_nothing_to_return() {
    let env = Env::default();
    env.mock_all_auths();
    let f = setup(&env);
    let client = GrantEscrowClient::new(&env, &f.escrow);
    let id = create(&env, &f);
    client.fund(&id, &f.funder, &usdc(M1));
    client.submit_evidence(&id, &0, &hash(&env, 0xE1));
    client.approve(&id, &0, &f.reviewers[0]);
    client.approve(&id, &0, &f.reviewers[1]);
    env.ledger().set_timestamp(f.deadline + DAY);
    assert_eq!(client.try_clawback(&id), Err(Ok(Error::NothingToClawback)));
    assert!(!client.grant(&id).closed);
}

#[test]
fn clawback_requires_the_funders_authorization() {
    let env = Env::default();
    env.mock_all_auths();
    let f = setup(&env);
    let client = GrantEscrowClient::new(&env, &f.escrow);
    let id = create_and_fund(&env, &f);
    env.ledger().set_timestamp(f.deadline + 1);
    for who in [&f.wallet, &f.reviewers[0]] {
        env.mock_auths(&[MockAuth {
            address: who,
            invoke: &MockAuthInvoke {
                contract: &f.escrow,
                fn_name: "clawback",
                args: (id,).into_val(&env),
                sub_invokes: &[],
            },
        }]);
        assert!(client.try_clawback(&id).is_err());
    }
    assert!(!client.grant(&id).closed);
    env.mock_auths(&[MockAuth {
        address: &f.funder,
        invoke: &MockAuthInvoke {
            contract: &f.escrow,
            fn_name: "clawback",
            args: (id,).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    client.clawback(&id);
    assert_eq!(env.auths()[0].0, f.funder);
}

// ---------------------------------------------------------------------------
// scenario: the whole journey with the real policy wallet
// ---------------------------------------------------------------------------

/// One row of data/seed/payments.csv: category, payee index, amount in cents
/// and the wallet error expected, if any.
struct SeedPayment {
    category: &'static str,
    payee: usize,
    cents: i128,
    expect: Option<WalletError>,
}

const SEED_PAYMENTS: [SeedPayment; 20] = [
    SeedPayment { category: "personnel", payee: 0, cents: 125_000, expect: None },
    SeedPayment { category: "personnel", payee: 1, cents: 200_000, expect: None },
    SeedPayment { category: "equipment", payee: 2, cents: 187_550, expect: None },
    SeedPayment { category: "fieldwork", payee: 3, cents: 34_075, expect: None },
    SeedPayment { category: "fieldwork", payee: 3, cents: 12_000, expect: None },
    SeedPayment { category: "training", payee: 4, cents: 45_000, expect: None },
    SeedPayment { category: "overhead", payee: 5, cents: 40_000, expect: None },
    // 1,875.50 + 800.00 would exceed the 2,500.00 equipment cap.
    SeedPayment { category: "equipment", payee: 2, cents: 80_000, expect: Some(WalletError::CategoryCapExceeded) },
    SeedPayment { category: "personnel", payee: 0, cents: 125_000, expect: None },
    SeedPayment { category: "fieldwork", payee: 6, cents: 61_525, expect: None },
    SeedPayment { category: "training", payee: 4, cents: 30_000, expect: None },
    SeedPayment { category: "personnel", payee: 1, cents: 100_000, expect: None },
    SeedPayment { category: "overhead", payee: 5, cents: 40_000, expect: None },
    // Payee 7 is the blocked vendor: never allowlisted.
    SeedPayment { category: "fieldwork", payee: 7, cents: 25_000, expect: Some(WalletError::PayeeNotAllowed) },
    SeedPayment { category: "equipment", payee: 2, cents: 60_000, expect: None },
    SeedPayment { category: "training", payee: 4, cents: 42_550, expect: None },
    SeedPayment { category: "fieldwork", payee: 6, cents: 50_000, expect: None },
    SeedPayment { category: "personnel", payee: 0, cents: 49_999, expect: None },
    SeedPayment { category: "fieldwork", payee: 3, cents: 42_400, expect: None },
    SeedPayment { category: "equipment", payee: 2, cents: 2_450, expect: None },
];

#[test]
fn scenario_three_milestones() {
    let env = Env::default();
    env.mock_all_auths();
    let f = setup(&env);
    let escrow = GrantEscrowClient::new(&env, &f.escrow);
    let token = TokenClient::new(&env, &f.token);

    // The grantee institution's wallet: policy from data/seed/budget.json,
    // seven allowlisted vendors, one blocked.
    let vendors: std::vec::Vec<Address> = (0..8).map(|_| Address::generate(&env)).collect();
    let mut allow = Vec::new(&env);
    for v in &vendors[..7] {
        allow.push_back(v.clone());
    }
    let cat = |name: &str, cap: i128| Category {
        name: Symbol::new(&env, name),
        cap: usdc(cap),
        spent: 0,
    };
    let policy = Policy {
        categories: vec![
            &env,
            cat("personnel", 600_000),
            cat("equipment", 250_000),
            cat("fieldwork", 200_000),
            cat("training", 120_000),
            cat("overhead", 80_000),
        ],
        payees: allow,
        per_tx_max: usdc(300_000),
    };
    let grantee_pubkey = BytesN::from_array(&env, &[0x5Au8; 32]);
    let wallet_addr = env.register(
        PolicyWallet,
        (grantee_pubkey, f.funder.clone(), f.token.clone(), policy),
    );
    let wallet = PolicyWalletClient::new(&env, &wallet_addr);

    // 1. Funder creates the grant and funds it in two tranches.
    let id = escrow.create_grant(
        &f.funder,
        &wallet_addr,
        &f.token,
        &seed_milestones(&env),
        &reviewers_vec(&env, &f),
        &2,
        &f.deadline,
    );
    escrow.fund(&id, &f.funder, &usdc(2_500_000));
    escrow.fund(&id, &f.funder, &usdc(1_250_000));
    assert_eq!(escrow.grant(&id).funded, usdc(M1 + M2 + M3));
    assert_eq!(token.balance(&f.funder), 0);

    // 2. Milestone 1: the grantee submits through the wallet, 2 of 3 approve.
    env.ledger().set_timestamp(START + 40 * DAY);
    wallet.submit_evidence(&f.escrow, &id, &0, &hash(&env, 0xE1));
    escrow.approve(&id, &0, &f.reviewers[0]);
    escrow.approve(&id, &0, &f.reviewers[2]);
    assert_eq!(
        escrow.grant(&id).milestones.get(0).unwrap().state,
        MilestoneState::Released
    );
    assert_eq!(token.balance(&wallet_addr), usdc(M1));

    // 3. Twenty vendor payments across the five budget lines.
    let mut accepted = 0u64;
    let mut expected_balance: std::vec::Vec<i128> = std::vec::Vec::new();
    expected_balance.resize(vendors.len(), 0);
    for (i, p) in SEED_PAYMENTS.iter().enumerate() {
        let category = Symbol::new(&env, p.category);
        let memo = hash(&env, i as u8 + 1);
        let result = wallet.try_pay(&category, &vendors[p.payee], &usdc(p.cents), &memo);
        match p.expect {
            None => {
                accepted += 1;
                assert_eq!(result, Ok(Ok(accepted)), "payment {} should succeed", i + 1);
                expected_balance[p.payee] += usdc(p.cents);
            }
            Some(err) => {
                assert_eq!(result, Err(Ok(err)), "payment {} should fail", i + 1);
            }
        }
    }
    assert_eq!(accepted, 18);
    assert_eq!(wallet.payment_count(), 18);
    for (v, expected) in vendors.iter().zip(expected_balance.iter()) {
        assert_eq!(token.balance(v), *expected);
    }
    assert_eq!(token.balance(&vendors[7]), 0);

    let ledger = wallet.ledger();
    let spent = |i: u32| ledger.get(i).unwrap().spent;
    assert_eq!(spent(0), usdc(599_999)); // personnel  5,999.99 of 6,000
    assert_eq!(spent(1), usdc(250_000)); // equipment  2,500.00 of 2,500 (at cap)
    assert_eq!(spent(2), usdc(200_000)); // fieldwork  2,000.00 of 2,000 (at cap)
    assert_eq!(spent(3), usdc(117_550)); // training   1,175.50 of 1,200
    assert_eq!(spent(4), usdc(80_000)); //  overhead     800.00 of 800 (at cap)
    let total_spent = usdc(599_999 + 250_000 + 200_000 + 117_550 + 80_000);
    assert_eq!(token.balance(&wallet_addr), usdc(M1) - total_spent);
    assert_eq!(token.balance(&wallet_addr), usdc(2_451)); // 24.51 left

    // 4. Milestone 2: first report rejected, resubmitted, approved.
    env.ledger().set_timestamp(START + 95 * DAY);
    wallet.submit_evidence(&f.escrow, &id, &1, &hash(&env, 0xE2));
    escrow.reject(&id, &1, &f.reviewers[1], &hash(&env, 0xBB));
    assert_eq!(
        escrow.grant(&id).milestones.get(1).unwrap().state,
        MilestoneState::Rejected
    );
    env.ledger().set_timestamp(START + 105 * DAY);
    wallet.submit_evidence(&f.escrow, &id, &1, &hash(&env, 0xE3));
    escrow.approve(&id, &1, &f.reviewers[0]);
    escrow.approve(&id, &1, &f.reviewers[1]);
    let g = escrow.grant(&id);
    assert_eq!(g.milestones.get(1).unwrap().state, MilestoneState::Released);
    assert_eq!(g.released, usdc(M1 + M2));
    assert_eq!(token.balance(&wallet_addr), usdc(2_451 + M2));

    // 5. Milestone 3 never arrives; after the deadline the funder claws back.
    env.ledger().set_timestamp(f.deadline + DAY);
    assert_eq!(escrow.clawback(&id), usdc(M3));
    let g = escrow.grant(&id);
    assert!(g.closed);
    assert_eq!(g.milestones.get(2).unwrap().state, MilestoneState::Pending);
    assert_eq!(token.balance(&f.funder), usdc(M3));
    assert_eq!(token.balance(&f.escrow), 0);
    assert_eq!(
        wallet.try_submit_evidence(&f.escrow, &id, &2, &hash(&env, 0xE4)).is_err(),
        true
    );

    // The wallet keeps working for the released tranches under its policy.
    assert_eq!(
        wallet.try_pay(&Symbol::new(&env, "overhead"), &vendors[5], &usdc(1), &hash(&env, 0xF0)),
        Err(Ok(WalletError::CategoryCapExceeded))
    );
}
