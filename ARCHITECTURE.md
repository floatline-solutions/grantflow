# GrantFlow architecture

## Components

```
                funder                     reviewers (quorum k of n)          grantee institution
                  |                               |                                   |
   create_grant / fund / clawback        approve / reject                pay / submit_evidence (Ed25519 key)
                  v                               v                                   v
        +------------------------------------------------+        +-----------------------------------------+
        |              grant_escrow (Soroban)            |        |     policy_wallet (Soroban custom account)|
        |  Grant { funder, grantee_wallet, token,        | release|  pubkey, funder, token,                  |
        |          milestones[], reviewers[], quorum,    |------->|  Policy { categories[{name,cap,spent}],   |
        |          deadline, funded, released, closed }  | (SAC   |           payees[], per_tx_max }          |
        |  Approvals(id, idx) -> Vec<Address>            | xfer)  |  __check_auth: only pay / submit_evidence |
        +------------------------------------------------+        +-----------------------------------------+
                  |  events                                              |  events        | token.transfer
                  v                                                      v                v
        grant_created, funded, evidence_submitted,             paid{category, payee, amount,     vendors /
        milestone_approved, milestone_released,                     memo_hash, seq, category_spent}  anchor
        milestone_rejected, clawed_back                        policy_updated, payee_added/removed, key_rotated

        app/ (TypeScript, @stellar/stellar-sdk 17)
          cli: create | fund | submit | review | pay | ledger | export   -> contract.Spec / contract.Client
          ai:  EvidenceProvider { HeuristicProvider, LlmProvider }       -> checklist for reviewers
          ledger: paid events -> per-category totals, CSV
```

Token: any SEP-41 token through `soroban_sdk::token::TokenClient`; in practice the USDC
Stellar Asset Contract. Tests register a SAC with `register_stellar_asset_contract_v2`.

## grant_escrow

**Storage.** `Grant(id)` and `Approvals(id, idx)` in persistent storage (a grant lives for
months); `NextId` in instance storage. Every write extends the entry's TTL (see *TTL and
archival*).

**Functions.**

| Function | Auth | Effect |
|---|---|---|
| `create_grant(funder, grantee_wallet, token, milestones, reviewers, quorum, deadline) -> id` | funder | validates amounts > 0, reviewers non-empty and unique, `1 <= quorum <= reviewers`, deadline in the future; milestone states are forced to `Pending` |
| `fund(id, from, amount)` | `from` | transfers `amount` into the escrow; anyone may fund up to the milestone total (`Overfunded` otherwise) |
| `submit_evidence(id, idx, evidence_hash)` | grantee wallet | `Pending`/`Rejected` -> `Submitted`; stores the report hash; clears approvals; flags late submissions in the event |
| `approve(id, idx, reviewer)` | reviewer | one vote per reviewer per submission; at quorum transfers the amount to the grantee wallet (`Released`) or, if the escrow is short, marks it `Approved` for a later `release` |
| `release(id, idx)` | none | completes an `Approved` milestone once funds arrived (the reviewers' decision is already on chain) |
| `reject(id, idx, reviewer, reason_hash)` | reviewer | `Submitted` -> `Rejected`; approvals cleared; the grantee may resubmit |
| `clawback(id) -> amount` | funder | only after `deadline`; returns `funded - released` to the funder and closes the grant; every further transition fails with `GrantClosed` |
| `grant(id)`, `approvals(id, idx)`, `grant_count()` | none | views |

`MilestoneState` is `Pending | Submitted | Approved | Rejected | Released`. `Rejected` is
kept as an observable state (the reason hash is in the event) rather than resetting to
`Pending`, so a dashboard can show what happened; for submission purposes it behaves like
`Pending`.

Failure modes handled: reviewer quorum unreachable -> the funder claws back after the
deadline; funds arrive after approval -> `release`; a reviewer tries to vote twice ->
`AlreadyApproved`; a funder tries to vote -> `NotReviewer`.

## policy_wallet

A Soroban **custom account**: a contract that implements
`soroban_sdk::auth::CustomAccountInterface`. Whenever the wallet's own address must
authorise something (`env.current_contract_address().require_auth()` inside `pay`), the
host calls the wallet's `__check_auth` with the signature payload, the signature and the
list of `Context`s being authorised.

**Storage** (instance): grantee `Pubkey`, `Funder`, `Token`, `Policy`, `PaySeq`.

**Functions.**

| Function | Auth | Effect |
|---|---|---|
| `__constructor(grantee_pubkey, funder, token, policy)` | deployer | validates the policy; `spent` starts at 0 regardless of input |
| `pay(category, payee, amount, memo_hash) -> seq` | **the wallet itself** (grantee signature via `__check_auth`) | `amount > 0`; payee allowlisted; `amount <= per_tx_max`; category exists; `spent + amount <= cap`; then `token.transfer(wallet, payee, amount)`, `spent += amount`, event `paid` |
| `submit_evidence(escrow, grant_id, idx, evidence_hash)` | the wallet itself | forwards to `escrow.submit_evidence`; the escrow's `grantee_wallet.require_auth()` is satisfied because the wallet is the direct invoker |
| `set_policy(policy)` | funder | replaces categories, payees and ceiling; `spent` is carried over by category name, and a cap below existing spend is rejected |
| `add_payee`, `remove_payee` | funder | allowlist maintenance |
| `rotate_key(new_pubkey)` | funder | key loss or staff change |
| `policy()`, `ledger()`, `payment_count()`, `funder()`, `token()`, `grantee_pubkey()` | none | views; `ledger()` is spend per category |

### The authorization flow of a payment

```
 grantee laptop / CLI                                  Stellar network (Soroban host)
 --------------------                                  -----------------------------------------------
 1. compose  pay(category, payee, amount, memo_hash)
    (contract.Spec -> ScVals; simulate to get the
     SorobanAuthorizationEntry for the WALLET address)
 2. payload = sha256( HashIdPreimage::SorobanAuthorization
                      { network_id, nonce, expiration,
                        invocation = wallet.pay(args) } )
 3. sig = Ed25519_sign(grantee_key, payload)
    entry.credentials.signature = ScVal::Bytes(sig)      // the wallet's Signature type is BytesN<64>
 4. submit tx  ----------------------------------------> 5. wallet.pay(...) runs
                                                            env.current_contract_address().require_auth()
                                                        6. host looks up the auth entry for the wallet address,
                                                           checks nonce (no replay) and expiration, computes the
                                                           same payload, and calls
                                                           wallet.__check_auth(payload, sig, [Context::Contract{
                                                               contract: wallet, fn_name: "pay", args }])
                                                        7. __check_auth:
                                                             - every context must be Context::Contract
                                                             - contract must be this wallet   (else ForeignContract)
                                                             - fn_name must be pay or submit_evidence
                                                                                              (else UnauthorizedFunction)
                                                             - ed25519_verify(pubkey, payload, sig)  (host traps if bad)
                                                        8. back in pay: policy checks
                                                             payee in allowlist, amount <= per_tx_max,
                                                             category known, spent + amount <= cap
                                                        9. token.transfer(wallet, payee, amount)
                                                           (SAC; the wallet is the invoker, no extra auth)
                                                       10. spent += amount; event paid{category, payee, amount,
                                                           memo_hash, seq, category_spent}
```

Steps 6-7 are exercised in `contracts/policy_wallet/src/test.rs` with real
`ed25519-dalek` keys, both by calling `__check_auth` directly and by submitting a signed
`SorobanAuthorizationEntry` through the host (`env.set_auths`). The same preimage and
signature format is produced by `app/src/chain/client.ts::authorizeWalletEntry` and
verified in `app/test/chain.test.ts`.

**What the grantee key cannot do.** A signature over `token.transfer(wallet, x, amount)`
is refused (`ForeignContract`): the only path out of the wallet is `pay`, and `pay`
enforces the policy. A signature over `set_policy`, `add_payee` or `rotate_key` is refused
(`UnauthorizedFunction`); those require the funder's authorization instead. Bundling a
valid `pay` with a foreign call in one entry is refused as a whole. Contract creation on
the wallet's behalf is refused (`UnsupportedContext`).

**Why `submit_evidence` is also allowed.** The escrow requires the grantee wallet's
authorization to submit evidence. Rather than letting the grantee key authorise a call on
the escrow contract, the wallet exposes its own `submit_evidence` and forwards the call;
the rule stays "the grantee key only ever authorises functions of the wallet", and the
wallet's function set is the whole policy surface.

### Why a custom account instead of a multisig

- A classic multisig (funder + grantee signers with thresholds) gates *who* signs, not
  *what* is signed. It cannot say "any amount up to 2,000 USDC to these seven vendors under
  `fieldwork`, but nothing else"; a co-signer would have to review every payment by hand,
  which is exactly the reviewer time the product is meant to remove.
- The custom account puts the budget rule where the money moves. The grantee signs one
  payment at a time with an ordinary Ed25519 key (a hardware key, a phone, the CLI); the
  contract decides whether that signature can move anything. The funder never holds the
  grantee's key and the grantee never holds an unconstrained balance.
- The policy is data, not code: the funder can raise a cap or add a vendor with
  `set_policy`/`add_payee` without redeploying, and every change is an event in the same
  ledger as the payments.
- Key loss is recoverable (`rotate_key`) without moving funds, and replay is prevented by
  the host's nonce handling on authorization entries, not by application code.

## Data flow in the app

- `create` hashes each milestone spec (sha256 of the normalised markdown) into
  `spec_hash`; `submit` hashes the report into `evidence_hash`; `review --reject` hashes
  the reason. The chain stores 32-byte commitments; the documents stay off chain with the
  parties.
- `review` runs the evidence checker: `parseDeliverables` extracts the numbered list from
  the spec; `HeuristicProvider` scores keyword coverage per deliverable (global coverage of
  the report weighted 0.6, best two-sentence window 0.4), cites the best sentence, and
  downgrades matches whose text reads like a promise ("will be delivered", "ongoing").
  `LlmProvider` asks the model for strict JSON against `EVIDENCE_JSON_SCHEMA`, validates,
  retries once, and downgrades any quote that is not verbatim in the report. Both return the
  same shape; the reviewer signs `approve` or `reject` separately.
- `pay --batch` and `ledger --simulate` apply `app/src/policy.ts`, an offline mirror of the
  contract's checks in the same order, so a finance officer can preview which payments a
  CSV would produce before signing them one by one.
- `ledger` / `export` decode `paid` events (topics `["paid", category, payee]`, data map
  `{amount, category_spent, memo_hash, seq}`) from Soroban RPC `getEvents` into totals and a
  CSV (`seq, ledger, closed_at, category, payee, amount_usdc, memo_hash, tx_hash`).

## Trust assumptions

- The funder is trusted to set a sensible policy and to use `clawback` only after the
  deadline; the contract enforces the deadline, not the fairness of the policy.
- Reviewers are trusted individually; the quorum limits the damage of one careless or
  malicious reviewer. There is no reviewer rotation or funder override before the deadline.
- The grantee institution controls one Ed25519 key. Whoever holds it can spend within the
  policy; the funder can rotate it but cannot spend.
- Vendors receive USDC at a Stellar address (a G account with a trustline, or an anchor /
  mobile-money contract). Local-currency cash-out is an anchor (SEP-24) concern outside these
  contracts.
- Off-chain documents are committed by hash only; a party must keep the documents to prove
  what a hash refers to.

## TTL and archival

Soroban ledger entries expire. Grants live for months, so `grant_escrow` writes grants and
approvals to **persistent** storage and extends their TTL on every write (threshold 7 days,
extend to 30 days of ledgers at ~5 s per ledger); `policy_wallet` keeps its small state in
**instance** storage and bumps it on every state change. A grant idle for more than 30 days
(no funding, submission, vote) can be archived; it is then restored with a
`RestoreFootprint` operation before use (the app's `contract.Client` handles restore on
simulation). A production deployment should run a keeper that extends TTLs for open grants
monthly, or extend to the full grant duration at creation.

## Why Stellar

- USDC with anchors in Kenya, Nigeria, Ghana, Uganda and Senegal-adjacent corridors for
  local-currency vendor payments; MoneyGram for cash; Stellar Disbursement Platform for bulk
  payouts.
- Soroban custom accounts (`__check_auth`) are the natural policy-signer primitive;
  authorisation entries carry nonces and expirations, so replay protection is a host
  guarantee.
- Sub-cent fees on hundreds of small vendor payments per milestone; ~5 s finality.
- Allo on Arbitrum is forkable but does not solve the fiat exit for the grantee.

## Limits and known gaps

- No funder override before the deadline and no reviewer replacement; a stuck quorum waits
  for the deadline.
- One policy per wallet; multi-grant institutions deploy one wallet per grant.
- The wallet has no funder "sweep" of unspent balances after a grant ends; unspent funds
  remain payable under the policy. A closing rule is a product decision to be validated with
  a fiscal sponsor.
- `submit_evidence` accepts any 32-byte hash; document storage and access are off chain.
- The heuristic checker is keyword coverage; it reports the presence of concrete language,
  not the quality of the work. The LLM provider was tested only with a stubbed client.
- Nothing has been deployed: `scripts/deploy-testnet.sh` was written against stellar-cli 28
  but never run.
