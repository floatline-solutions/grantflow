#![cfg(test)]
extern crate std;

use super::*;
use ed25519_dalek::{Signer, SigningKey};
use soroban_sdk::{
    auth::ContractContext,
    contract, contractimpl,
    testutils::{Address as _, Events as _, Ledger as _, MockAuth, MockAuthInvoke},
    token::{StellarAssetClient, TokenClient},
    vec,
    xdr::{self, WriteXdr},
    Address, Bytes, BytesN, Env, IntoVal, Symbol, Val, Vec,
};

/// USDC has 7 decimals on Stellar: 1 cent = 100_000 stroops.
fn usdc(cents: i128) -> i128 {
    cents * 100_000
}

fn cat(env: &Env, name: &str, cap_cents: i128) -> Category {
    Category {
        name: Symbol::new(env, name),
        cap: usdc(cap_cents),
        spent: 0,
    }
}

/// Milestone-1 budget from data/seed/budget.json: five lines, 12,500 USDC.
fn seed_policy(env: &Env, payees: &[Address]) -> Policy {
    let mut allow = Vec::new(env);
    for p in payees {
        allow.push_back(p.clone());
    }
    Policy {
        categories: vec![
            env,
            cat(env, "personnel", 600_000),
            cat(env, "equipment", 250_000),
            cat(env, "fieldwork", 200_000),
            cat(env, "training", 120_000),
            cat(env, "overhead", 80_000),
        ],
        payees: allow,
        per_tx_max: usdc(300_000),
    }
}

struct Fixture {
    funder: Address,
    token: Address,
    wallet: Address,
    signing_key: SigningKey,
    payees: std::vec::Vec<Address>,
}

fn setup(env: &Env) -> Fixture {
    let funder = Address::generate(env);
    let token_admin = Address::generate(env);
    let token = env.register_stellar_asset_contract_v2(token_admin).address();
    let signing_key = SigningKey::from_bytes(&[42u8; 32]);
    let pubkey = BytesN::from_array(env, &signing_key.verifying_key().to_bytes());
    let payees: std::vec::Vec<Address> = (0..3).map(|_| Address::generate(env)).collect();
    let policy = seed_policy(env, &payees);
    let wallet = env.register(PolicyWallet, (pubkey, funder.clone(), token.clone(), policy));
    Fixture {
        funder,
        token,
        wallet,
        signing_key,
        payees,
    }
}

fn mint(env: &Env, token: &Address, to: &Address, amount: i128) {
    StellarAssetClient::new(env, token).mint(to, &amount);
}

fn memo(env: &Env, n: u8) -> BytesN<32> {
    BytesN::from_array(env, &[n; 32])
}

fn wallet_events(env: &Env, wallet: &Address) -> usize {
    env.events()
        .all()
        .filter_by_contract(wallet)
        .events()
        .len()
}

/// Build a Soroban authorization entry for `signer` over a root call
/// `contract.fn_name(args)`, signed with the grantee's Ed25519 key exactly as
/// a wallet would sign it for submission to the network.
fn signed_entry(
    env: &Env,
    sk: &SigningKey,
    signer: &Address,
    contract: &Address,
    fn_name: &str,
    args: &[Val],
    nonce: i64,
    expiration: u32,
) -> xdr::SorobanAuthorizationEntry {
    let sc_args: std::vec::Vec<xdr::ScVal> = args.iter().map(|v| v.into_val(env)).collect();
    let invocation = xdr::SorobanAuthorizedInvocation {
        function: xdr::SorobanAuthorizedFunction::ContractFn(xdr::InvokeContractArgs {
            contract_address: contract.into(),
            function_name: xdr::ScSymbol(fn_name.try_into().unwrap()),
            args: sc_args.try_into().unwrap(),
        }),
        sub_invocations: Default::default(),
    };
    let preimage = xdr::HashIdPreimage::SorobanAuthorization(
        xdr::HashIdPreimageSorobanAuthorization {
            network_id: xdr::Hash(env.ledger().network_id().to_array()),
            nonce,
            signature_expiration_ledger: expiration,
            invocation: invocation.clone(),
        },
    );
    let preimage_xdr = preimage.to_xdr(xdr::Limits::none()).unwrap();
    let payload = env
        .crypto()
        .sha256(&Bytes::from_slice(env, &preimage_xdr))
        .to_array();
    let signature = sk.sign(&payload).to_bytes();
    xdr::SorobanAuthorizationEntry {
        credentials: xdr::SorobanCredentials::Address(xdr::SorobanAddressCredentials {
            address: signer.into(),
            nonce,
            signature_expiration_ledger: expiration,
            signature: xdr::ScVal::Bytes(xdr::ScBytes(signature.to_vec().try_into().unwrap())),
        }),
        root_invocation: invocation,
    }
}

// ---------------------------------------------------------------------------
// constructor and views
// ---------------------------------------------------------------------------

#[test]
fn constructor_stores_policy_and_starts_spent_at_zero() {
    let env = Env::default();
    let funder = Address::generate(&env);
    let token = Address::generate(&env);
    let payee = Address::generate(&env);
    let pubkey = BytesN::from_array(&env, &[9u8; 32]);
    let mut policy = seed_policy(&env, &[payee.clone()]);
    // A funder-supplied non-zero `spent` must not leak into the ledger.
    let mut first = policy.categories.get(0).unwrap();
    first.spent = usdc(100);
    policy.categories.set(0, first);

    let wallet = env.register(
        PolicyWallet,
        (pubkey.clone(), funder.clone(), token.clone(), policy.clone()),
    );
    let client = PolicyWalletClient::new(&env, &wallet);

    let stored = client.policy();
    assert_eq!(stored.per_tx_max, usdc(300_000));
    assert_eq!(stored.payees, vec![&env, payee]);
    assert_eq!(stored.categories.len(), 5);
    for c in client.ledger().iter() {
        assert_eq!(c.spent, 0);
    }
    assert_eq!(client.payment_count(), 0);
    assert_eq!(client.funder(), funder);
    assert_eq!(client.token(), token);
    assert_eq!(client.grantee_pubkey(), pubkey);
}

#[test]
#[should_panic(expected = "Error(Contract, #7)")]
fn constructor_rejects_zero_per_tx_max() {
    let env = Env::default();
    let payee = Address::generate(&env);
    let mut policy = seed_policy(&env, &[payee]);
    policy.per_tx_max = 0;
    env.register(
        PolicyWallet,
        (
            BytesN::from_array(&env, &[1u8; 32]),
            Address::generate(&env),
            Address::generate(&env),
            policy,
        ),
    );
}

#[test]
#[should_panic(expected = "Error(Contract, #7)")]
fn constructor_rejects_duplicate_category_names() {
    let env = Env::default();
    let payee = Address::generate(&env);
    let mut policy = seed_policy(&env, &[payee]);
    policy.categories.push_back(cat(&env, "personnel", 1));
    env.register(
        PolicyWallet,
        (
            BytesN::from_array(&env, &[1u8; 32]),
            Address::generate(&env),
            Address::generate(&env),
            policy,
        ),
    );
}

// ---------------------------------------------------------------------------
// pay
// ---------------------------------------------------------------------------

#[test]
fn pay_transfers_updates_ledger_and_emits_event() {
    let env = Env::default();
    env.mock_all_auths();
    let f = setup(&env);
    let client = PolicyWalletClient::new(&env, &f.wallet);
    let token = TokenClient::new(&env, &f.token);
    mint(&env, &f.token, &f.wallet, usdc(1_250_000));

    let seq = client.pay(
        &Symbol::new(&env, "personnel"),
        &f.payees[0],
        &usdc(125_000),
        &memo(&env, 1),
    );
    assert_eq!(seq, 1);
    // Events of the last invocation: one `Paid` from the wallet (the token
    // contract's own transfer event is filtered out).
    assert_eq!(wallet_events(&env, &f.wallet), 1);
    // The wallet itself is the address that had to authorize `pay`.
    let auths = env.auths();
    assert_eq!(auths.len(), 1);
    assert_eq!(auths[0].0, f.wallet);
    match &auths[0].1.function {
        soroban_sdk::testutils::AuthorizedFunction::Contract((contract, name, _)) => {
            assert_eq!(*contract, f.wallet);
            assert_eq!(*name, symbol_short!("pay"));
        }
        other => panic!("unexpected authorized function {:?}", other),
    }

    assert_eq!(token.balance(&f.payees[0]), usdc(125_000));
    assert_eq!(token.balance(&f.wallet), usdc(1_125_000));
    assert_eq!(client.payment_count(), 1);
    let ledger = client.ledger();
    assert_eq!(ledger.get(0).unwrap().spent, usdc(125_000));
    assert_eq!(ledger.get(1).unwrap().spent, 0);
}

#[test]
fn pay_requires_the_wallets_own_authorization() {
    let env = Env::default();
    let f = setup(&env);
    env.mock_all_auths();
    mint(&env, &f.token, &f.wallet, usdc(100_000));
    let client = PolicyWalletClient::new(&env, &f.wallet);
    let category = Symbol::new(&env, "personnel");
    let amount = usdc(10_000);
    let m = memo(&env, 2);

    // Authorization by the funder (or anyone else) is not the wallet's.
    let stranger = Address::generate(&env);
    env.mock_auths(&[MockAuth {
        address: &stranger,
        invoke: &MockAuthInvoke {
            contract: &f.wallet,
            fn_name: "pay",
            args: (category.clone(), f.payees[0].clone(), amount, m.clone()).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    assert!(client.try_pay(&category, &f.payees[0], &amount, &m).is_err());
    assert_eq!(TokenClient::new(&env, &f.token).balance(&f.payees[0]), 0);
}

#[test]
fn pay_rejects_payee_not_on_allowlist() {
    let env = Env::default();
    env.mock_all_auths();
    let f = setup(&env);
    mint(&env, &f.token, &f.wallet, usdc(100_000));
    let client = PolicyWalletClient::new(&env, &f.wallet);
    let blocked = Address::generate(&env);
    assert_eq!(
        client.try_pay(&Symbol::new(&env, "fieldwork"), &blocked, &usdc(25_000), &memo(&env, 3)),
        Err(Ok(Error::PayeeNotAllowed))
    );
    assert_eq!(client.payment_count(), 0);
}

#[test]
fn pay_rejects_amount_over_per_tx_max() {
    let env = Env::default();
    env.mock_all_auths();
    let f = setup(&env);
    mint(&env, &f.token, &f.wallet, usdc(1_000_000));
    let client = PolicyWalletClient::new(&env, &f.wallet);
    assert_eq!(
        client.try_pay(
            &Symbol::new(&env, "personnel"),
            &f.payees[0],
            &usdc(300_001),
            &memo(&env, 4)
        ),
        Err(Ok(Error::AmountExceedsPerTxMax))
    );
    // Exactly the ceiling is fine.
    client.pay(
        &Symbol::new(&env, "personnel"),
        &f.payees[0],
        &usdc(300_000),
        &memo(&env, 4),
    );
}

#[test]
fn pay_rejects_when_category_cap_would_be_exceeded() {
    let env = Env::default();
    env.mock_all_auths();
    let f = setup(&env);
    mint(&env, &f.token, &f.wallet, usdc(1_000_000));
    let client = PolicyWalletClient::new(&env, &f.wallet);
    let equipment = Symbol::new(&env, "equipment");
    client.pay(&equipment, &f.payees[2], &usdc(187_550), &memo(&env, 5));
    assert_eq!(
        client.try_pay(&equipment, &f.payees[2], &usdc(80_000), &memo(&env, 6)),
        Err(Ok(Error::CategoryCapExceeded))
    );
    // Up to the cap exactly is allowed.
    client.pay(&equipment, &f.payees[2], &usdc(62_450), &memo(&env, 7));
    assert_eq!(client.ledger().get(1).unwrap().spent, usdc(250_000));
    assert_eq!(
        client.try_pay(&equipment, &f.payees[2], &1, &memo(&env, 8)),
        Err(Ok(Error::CategoryCapExceeded))
    );
}

#[test]
fn pay_rejects_unknown_category_and_non_positive_amount() {
    let env = Env::default();
    env.mock_all_auths();
    let f = setup(&env);
    mint(&env, &f.token, &f.wallet, usdc(100_000));
    let client = PolicyWalletClient::new(&env, &f.wallet);
    assert_eq!(
        client.try_pay(&Symbol::new(&env, "travel"), &f.payees[0], &usdc(100), &memo(&env, 9)),
        Err(Ok(Error::CategoryUnknown))
    );
    assert_eq!(
        client.try_pay(&Symbol::new(&env, "personnel"), &f.payees[0], &0, &memo(&env, 9)),
        Err(Ok(Error::InvalidAmount))
    );
    assert_eq!(
        client.try_pay(&Symbol::new(&env, "personnel"), &f.payees[0], &-5, &memo(&env, 9)),
        Err(Ok(Error::InvalidAmount))
    );
}

#[test]
fn pay_fails_when_wallet_has_no_balance() {
    let env = Env::default();
    env.mock_all_auths();
    let f = setup(&env);
    let client = PolicyWalletClient::new(&env, &f.wallet);
    assert!(client
        .try_pay(&Symbol::new(&env, "personnel"), &f.payees[0], &usdc(100), &memo(&env, 10))
        .is_err());
    assert_eq!(client.payment_count(), 0);
}

// ---------------------------------------------------------------------------
// funder administration
// ---------------------------------------------------------------------------

#[test]
fn set_policy_by_funder_keeps_spent_per_category() {
    let env = Env::default();
    env.mock_all_auths();
    let f = setup(&env);
    mint(&env, &f.token, &f.wallet, usdc(1_000_000));
    let client = PolicyWalletClient::new(&env, &f.wallet);
    client.pay(&Symbol::new(&env, "personnel"), &f.payees[0], &usdc(125_000), &memo(&env, 1));

    let new_policy = Policy {
        categories: vec![
            &env,
            cat(&env, "personnel", 700_000),
            cat(&env, "dissemination", 50_000),
        ],
        payees: vec![&env, f.payees[0].clone()],
        per_tx_max: usdc(150_000),
    };
    client.set_policy(&new_policy);
    assert_eq!(wallet_events(&env, &f.wallet), 1);

    let stored = client.policy();
    assert_eq!(stored.categories.len(), 2);
    assert_eq!(stored.categories.get(0).unwrap().spent, usdc(125_000));
    assert_eq!(stored.categories.get(0).unwrap().cap, usdc(700_000));
    assert_eq!(stored.categories.get(1).unwrap().spent, 0);
    assert_eq!(stored.per_tx_max, usdc(150_000));
    assert_eq!(stored.payees.len(), 1);

    // The removed payee can no longer be paid; the lowered ceiling applies.
    assert_eq!(
        client.try_pay(&Symbol::new(&env, "personnel"), &f.payees[1], &usdc(100), &memo(&env, 2)),
        Err(Ok(Error::PayeeNotAllowed))
    );
    assert_eq!(
        client.try_pay(
            &Symbol::new(&env, "personnel"),
            &f.payees[0],
            &usdc(150_001),
            &memo(&env, 2)
        ),
        Err(Ok(Error::AmountExceedsPerTxMax))
    );
}

#[test]
fn set_policy_rejects_cap_below_existing_spend() {
    let env = Env::default();
    env.mock_all_auths();
    let f = setup(&env);
    mint(&env, &f.token, &f.wallet, usdc(1_000_000));
    let client = PolicyWalletClient::new(&env, &f.wallet);
    client.pay(&Symbol::new(&env, "personnel"), &f.payees[0], &usdc(125_000), &memo(&env, 1));
    let mut policy = seed_policy(&env, &f.payees);
    let mut first = policy.categories.get(0).unwrap();
    first.cap = usdc(100_000);
    policy.categories.set(0, first);
    assert_eq!(client.try_set_policy(&policy), Err(Ok(Error::InvalidPolicy)));
}

#[test]
fn set_policy_rejects_anyone_but_the_funder() {
    let env = Env::default();
    let f = setup(&env);
    let client = PolicyWalletClient::new(&env, &f.wallet);
    let policy = seed_policy(&env, &f.payees);
    let intruder = Address::generate(&env);
    env.mock_auths(&[MockAuth {
        address: &intruder,
        invoke: &MockAuthInvoke {
            contract: &f.wallet,
            fn_name: "set_policy",
            args: (policy.clone(),).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    assert!(client.try_set_policy(&policy).is_err());

    // The funder's authorization is what the contract requires.
    env.mock_auths(&[MockAuth {
        address: &f.funder,
        invoke: &MockAuthInvoke {
            contract: &f.wallet,
            fn_name: "set_policy",
            args: (policy.clone(),).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    client.set_policy(&policy);
    assert_eq!(env.auths()[0].0, f.funder);
}

#[test]
fn add_and_remove_payee_by_funder() {
    let env = Env::default();
    env.mock_all_auths();
    let f = setup(&env);
    mint(&env, &f.token, &f.wallet, usdc(100_000));
    let client = PolicyWalletClient::new(&env, &f.wallet);
    let vendor = Address::generate(&env);

    client.add_payee(&vendor);
    assert_eq!(wallet_events(&env, &f.wallet), 1);
    assert_eq!(client.try_add_payee(&vendor), Err(Ok(Error::PayeeAlreadyListed)));
    assert_eq!(client.policy().payees.len(), 4);
    client.pay(&Symbol::new(&env, "overhead"), &vendor, &usdc(10_000), &memo(&env, 1));

    client.remove_payee(&vendor);
    assert_eq!(wallet_events(&env, &f.wallet), 1);
    assert_eq!(client.try_remove_payee(&vendor), Err(Ok(Error::PayeeNotFound)));
    assert_eq!(client.policy().payees.len(), 3);
    assert_eq!(
        client.try_pay(&Symbol::new(&env, "overhead"), &vendor, &usdc(10_000), &memo(&env, 2)),
        Err(Ok(Error::PayeeNotAllowed))
    );
}

#[test]
fn add_payee_rejects_anyone_but_the_funder() {
    let env = Env::default();
    let f = setup(&env);
    let client = PolicyWalletClient::new(&env, &f.wallet);
    let vendor = Address::generate(&env);
    let intruder = Address::generate(&env);
    env.mock_auths(&[MockAuth {
        address: &intruder,
        invoke: &MockAuthInvoke {
            contract: &f.wallet,
            fn_name: "add_payee",
            args: (vendor.clone(),).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    assert!(client.try_add_payee(&vendor).is_err());
    env.mock_auths(&[MockAuth {
        address: &intruder,
        invoke: &MockAuthInvoke {
            contract: &f.wallet,
            fn_name: "remove_payee",
            args: (f.payees[0].clone(),).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    assert!(client.try_remove_payee(&f.payees[0]).is_err());
}

#[test]
fn rotate_key_by_funder_switches_the_accepted_signature() {
    let env = Env::default();
    env.mock_all_auths();
    let f = setup(&env);
    let client = PolicyWalletClient::new(&env, &f.wallet);
    let new_key = SigningKey::from_bytes(&[77u8; 32]);
    let new_pub = BytesN::from_array(&env, &new_key.verifying_key().to_bytes());

    client.rotate_key(&new_pub);
    assert_eq!(wallet_events(&env, &f.wallet), 1);
    assert_eq!(client.grantee_pubkey(), new_pub);

    let payload = BytesN::from_array(&env, &[5u8; 32]);
    let contexts = vec![
        &env,
        Context::Contract(ContractContext {
            contract: f.wallet.clone(),
            fn_name: symbol_short!("pay"),
            args: vec![&env],
        }),
    ];
    let old_sig = BytesN::from_array(&env, &f.signing_key.sign(&payload.to_array()).to_bytes());
    assert!(env
        .try_invoke_contract_check_auth::<Error>(&f.wallet, &payload, old_sig.into_val(&env), &contexts)
        .is_err());
    let new_sig = BytesN::from_array(&env, &new_key.sign(&payload.to_array()).to_bytes());
    assert_eq!(
        env.try_invoke_contract_check_auth::<Error>(&f.wallet, &payload, new_sig.into_val(&env), &contexts),
        Ok(())
    );
}

#[test]
fn rotate_key_rejects_anyone_but_the_funder() {
    let env = Env::default();
    let f = setup(&env);
    let client = PolicyWalletClient::new(&env, &f.wallet);
    let new_pub = BytesN::from_array(&env, &[3u8; 32]);
    let intruder = Address::generate(&env);
    env.mock_auths(&[MockAuth {
        address: &intruder,
        invoke: &MockAuthInvoke {
            contract: &f.wallet,
            fn_name: "rotate_key",
            args: (new_pub.clone(),).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    assert!(client.try_rotate_key(&new_pub).is_err());
}

// ---------------------------------------------------------------------------
// __check_auth with real Ed25519 signatures
// ---------------------------------------------------------------------------

fn pay_context(env: &Env, wallet: &Address) -> Vec<Context> {
    vec![
        env,
        Context::Contract(ContractContext {
            contract: wallet.clone(),
            fn_name: symbol_short!("pay"),
            args: vec![
                env,
                Symbol::new(env, "fieldwork").into_val(env),
                Address::generate(env).into_val(env),
                usdc(34_075).into_val(env),
                memo(env, 1).into_val(env),
            ],
        }),
    ]
}

#[test]
fn check_auth_accepts_a_valid_signature_for_pay() {
    let env = Env::default();
    let f = setup(&env);
    let payload = BytesN::from_array(&env, &[11u8; 32]);
    let sig = BytesN::from_array(&env, &f.signing_key.sign(&payload.to_array()).to_bytes());
    assert_eq!(
        env.try_invoke_contract_check_auth::<Error>(
            &f.wallet,
            &payload,
            sig.into_val(&env),
            &pay_context(&env, &f.wallet)
        ),
        Ok(())
    );
}

#[test]
fn check_auth_accepts_a_valid_signature_for_submit_evidence() {
    let env = Env::default();
    let f = setup(&env);
    let payload = BytesN::from_array(&env, &[12u8; 32]);
    let sig = BytesN::from_array(&env, &f.signing_key.sign(&payload.to_array()).to_bytes());
    let contexts = vec![
        &env,
        Context::Contract(ContractContext {
            contract: f.wallet.clone(),
            fn_name: Symbol::new(&env, "submit_evidence"),
            args: vec![&env],
        }),
    ];
    assert_eq!(
        env.try_invoke_contract_check_auth::<Error>(&f.wallet, &payload, sig.into_val(&env), &contexts),
        Ok(())
    );
}

#[test]
fn check_auth_rejects_an_invalid_signature() {
    let env = Env::default();
    let f = setup(&env);
    let payload = BytesN::from_array(&env, &[11u8; 32]);
    let contexts = pay_context(&env, &f.wallet);

    // Signed by a different key.
    let other = SigningKey::from_bytes(&[1u8; 32]);
    let wrong_key_sig = BytesN::from_array(&env, &other.sign(&payload.to_array()).to_bytes());
    assert!(env
        .try_invoke_contract_check_auth::<Error>(&f.wallet, &payload, wrong_key_sig.into_val(&env), &contexts)
        .is_err());

    // Right key, different payload.
    let other_payload = BytesN::from_array(&env, &[13u8; 32]);
    let stale_sig =
        BytesN::from_array(&env, &f.signing_key.sign(&other_payload.to_array()).to_bytes());
    assert!(env
        .try_invoke_contract_check_auth::<Error>(&f.wallet, &payload, stale_sig.into_val(&env), &contexts)
        .is_err());

    // Garbage.
    let garbage = BytesN::from_array(&env, &[0u8; 64]);
    assert!(env
        .try_invoke_contract_check_auth::<Error>(&f.wallet, &payload, garbage.into_val(&env), &contexts)
        .is_err());
}

#[test]
fn check_auth_rejects_functions_other_than_pay_or_submit_evidence() {
    let env = Env::default();
    let f = setup(&env);
    let payload = BytesN::from_array(&env, &[11u8; 32]);
    let sig: Val = BytesN::from_array(&env, &f.signing_key.sign(&payload.to_array()).to_bytes())
        .into_val(&env);

    // A funder-only function on the wallet itself.
    let set_policy = vec![
        &env,
        Context::Contract(ContractContext {
            contract: f.wallet.clone(),
            fn_name: Symbol::new(&env, "set_policy"),
            args: vec![&env],
        }),
    ];
    assert_eq!(
        env.try_invoke_contract_check_auth::<Error>(&f.wallet, &payload, sig, &set_policy),
        Err(Ok(Error::UnauthorizedFunction))
    );

    // A direct token transfer from the wallet, bypassing the policy.
    let transfer = vec![
        &env,
        Context::Contract(ContractContext {
            contract: f.token.clone(),
            fn_name: symbol_short!("transfer"),
            args: vec![
                &env,
                f.wallet.clone().into_val(&env),
                Address::generate(&env).into_val(&env),
                usdc(1).into_val(&env),
            ],
        }),
    ];
    assert_eq!(
        env.try_invoke_contract_check_auth::<Error>(&f.wallet, &payload, sig, &transfer),
        Err(Ok(Error::ForeignContract))
    );

    // A valid `pay` bundled with a foreign call is rejected as a whole.
    let mut mixed = pay_context(&env, &f.wallet);
    mixed.append(&transfer);
    assert_eq!(
        env.try_invoke_contract_check_auth::<Error>(&f.wallet, &payload, sig, &mixed),
        Err(Ok(Error::ForeignContract))
    );

    // Deploying contracts on the wallet's behalf.
    let deploy = vec![
        &env,
        Context::CreateContractHostFn(soroban_sdk::auth::CreateContractHostFnContext {
            executable: soroban_sdk::auth::ContractExecutable::Wasm(BytesN::from_array(&env, &[0u8; 32])),
            salt: BytesN::from_array(&env, &[0u8; 32]),
        }),
    ];
    assert_eq!(
        env.try_invoke_contract_check_auth::<Error>(&f.wallet, &payload, sig, &deploy),
        Err(Ok(Error::UnsupportedContext))
    );

    // Nothing to authorize is not an authorization.
    assert_eq!(
        env.try_invoke_contract_check_auth::<Error>(&f.wallet, &payload, sig, &vec![&env]),
        Err(Ok(Error::NoContexts))
    );
}

// ---------------------------------------------------------------------------
// end to end through the host: signed authorization entries
// ---------------------------------------------------------------------------

#[test]
fn pay_end_to_end_with_grantee_signed_auth_entry_and_no_replay() {
    let env = Env::default();
    env.ledger().set_sequence_number(100);
    env.mock_all_auths();
    let f = setup(&env);
    mint(&env, &f.token, &f.wallet, usdc(1_250_000));
    let client = PolicyWalletClient::new(&env, &f.wallet);
    let token = TokenClient::new(&env, &f.token);

    let category = Symbol::new(&env, "fieldwork");
    let amount = usdc(34_075);
    let m = memo(&env, 21);
    let args: [Val; 4] = [
        category.into_val(&env),
        f.payees[1].clone().into_val(&env),
        amount.into_val(&env),
        m.clone().into_val(&env),
    ];
    let entry = signed_entry(
        &env,
        &f.signing_key,
        &f.wallet,
        &f.wallet,
        "pay",
        &args,
        7_001,
        500,
    );

    // Real signature, real `__check_auth`, real policy checks, real transfer.
    env.set_auths(&[entry.clone()]);
    let seq = client.pay(&category, &f.payees[1], &amount, &m);
    assert_eq!(seq, 1);
    assert_eq!(token.balance(&f.payees[1]), amount);
    assert_eq!(client.ledger().get(2).unwrap().spent, amount);

    // The same signed entry cannot be replayed: the host consumed its nonce.
    env.set_auths(&[entry]);
    assert!(client.try_pay(&category, &f.payees[1], &amount, &m).is_err());
    assert_eq!(token.balance(&f.payees[1]), amount);

    // A signature from someone else's key is refused by `__check_auth`.
    let imposter = SigningKey::from_bytes(&[3u8; 32]);
    let forged = signed_entry(&env, &imposter, &f.wallet, &f.wallet, "pay", &args, 7_002, 500);
    env.set_auths(&[forged]);
    assert!(client.try_pay(&category, &f.payees[1], &amount, &m).is_err());
    assert_eq!(client.payment_count(), 1);
}

#[test]
fn grantee_key_cannot_move_tokens_around_the_policy() {
    let env = Env::default();
    env.ledger().set_sequence_number(100);
    env.mock_all_auths();
    let f = setup(&env);
    mint(&env, &f.token, &f.wallet, usdc(1_250_000));
    let token = TokenClient::new(&env, &f.token);
    let attacker = Address::generate(&env);
    let amount = usdc(1_250_000);

    // A perfectly valid grantee signature over `token.transfer(wallet, ...)`.
    let args: [Val; 3] = [
        f.wallet.clone().into_val(&env),
        attacker.clone().into_val(&env),
        amount.into_val(&env),
    ];
    let entry = signed_entry(
        &env,
        &f.signing_key,
        &f.wallet,
        &f.token,
        "transfer",
        &args,
        9_001,
        500,
    );
    env.set_auths(&[entry]);
    assert!(token.try_transfer(&f.wallet, &attacker, &amount).is_err());
    assert_eq!(token.balance(&attacker), 0);
    assert_eq!(token.balance(&f.wallet), amount);
}

/// A stand-in for `grant_escrow::submit_evidence`: it requires the grantee
/// wallet's authorization exactly like the real escrow does.
#[contract]
pub struct MockEscrow;

#[contractimpl]
impl MockEscrow {
    pub fn __constructor(env: Env, wallet: Address) {
        env.storage().instance().set(&symbol_short!("wallet"), &wallet);
    }

    pub fn submit_evidence(env: Env, id: u64, idx: u32, evidence_hash: BytesN<32>) {
        let wallet: Address = env.storage().instance().get(&symbol_short!("wallet")).unwrap();
        wallet.require_auth();
        env.storage()
            .instance()
            .set(&symbol_short!("last"), &(id, idx, evidence_hash));
    }

    pub fn last(env: Env) -> Option<(u64, u32, BytesN<32>)> {
        env.storage().instance().get(&symbol_short!("last"))
    }
}

#[test]
fn submit_evidence_end_to_end_with_grantee_signed_auth_entry() {
    let env = Env::default();
    env.ledger().set_sequence_number(100);
    let f = setup(&env);
    let escrow = env.register(MockEscrow, (f.wallet.clone(),));
    let client = PolicyWalletClient::new(&env, &f.wallet);
    let evidence = memo(&env, 33);

    let args: [Val; 4] = [
        escrow.clone().into_val(&env),
        4u64.into_val(&env),
        1u32.into_val(&env),
        evidence.clone().into_val(&env),
    ];
    let entry = signed_entry(
        &env,
        &f.signing_key,
        &f.wallet,
        &f.wallet,
        "submit_evidence",
        &args,
        11_001,
        500,
    );
    env.set_auths(&[entry]);
    client.submit_evidence(&escrow, &4, &1, &evidence);
    assert_eq!(wallet_events(&env, &f.wallet), 1);
    assert_eq!(
        MockEscrowClient::new(&env, &escrow).last(),
        Some((4u64, 1u32, evidence.clone()))
    );

    // Calling the escrow directly with a grantee-signed entry is refused,
    // because the context is a foreign contract.
    let direct_args: [Val; 3] = [
        4u64.into_val(&env),
        2u32.into_val(&env),
        evidence.clone().into_val(&env),
    ];
    let direct = signed_entry(
        &env,
        &f.signing_key,
        &f.wallet,
        &escrow,
        "submit_evidence",
        &direct_args,
        11_002,
        500,
    );
    env.set_auths(&[direct]);
    assert!(MockEscrowClient::new(&env, &escrow)
        .try_submit_evidence(&4, &2, &evidence)
        .is_err());
}
