# GrantFlow validation

What we know, what we assume, what we built, and what would prove or kill the idea.
Nothing in this file is a claim of adoption: no user, funder, interview, pilot or partner
exists yet.

## Evidence tiers

### Researched evidence

- Sub-grantees in LMICs pre-finance activities and claim reimbursement in arrears;
  "payment in arrears and the need to locally fund research activities upfront can result in
  funding being diverted from health programmes until reimbursement" (Lancet Global Health,
  cited in the research dossier, problem P22).
- Donor disbursements in Sub-Saharan Africa are "extremely low/slow" or "stop-go" (MPRA
  87106, same dossier); African universities cite irregular budget disbursement and weak
  grant-management systems (Frontiers in Education 2026); Senegalese students went unpaid
  because of grant delays (Standard Media) — all P22.
- Incumbents stop at approval: Fluxx, SmartSimple, Submittable manage workflows, not
  cross-border money; Open Collective is a fiscal host on Stripe/PayPal rails; Allo v2 is in
  maintenance mode; DeSci DAOs fund crypto-native work; StellarGrantProtocol has milestone
  escrow but no funder (research dossier, P22 alternatives and final selection).
- Stellar has USDC, anchors in KE/NG/GH/UG and MoneyGram cash-out, custom accounts
  (`__check_auth`) as a policy-signer primitive, and sub-cent fees (final selection, "Why
  Stellar"; Stellar ecosystem notes).

### Observed facts (this repository)

- Both contracts execute in the Soroban host: 43 tests pass (`cargo test`), including the
  full 3-milestone journey with 20 vendor payments and real Ed25519 signatures through
  `__check_auth`; wasm builds at 20.4 KB (escrow) and 22.2 KB (wallet) with the current toolchain.
- The app's 29 tests pass offline; the policy simulation in TypeScript produces the same 18
  accepted / 2 rejected outcome as the contract on the same seed CSV.
- On the seed reports the heuristic evidence checker marks the complete milestone-1 report 5/5
  found, the draft milestone-2 report 1 found / 3 uncertain / 1 missing (the three "will be
  delivered" promises are flagged as such), and the empty milestone-3 placeholder 0/5.
- A full grant journey was executed on Stellar testnet on 2026-11-05 (UTC); see "Testnet end-to-end
  run" below. An earlier offline build environment had no route to testnet.

### Team assumptions

- A milestone costs dozens of person-hours across funder, prime and grantee (reconciliation,
  receipts, wires); the scale is unquantified in public sources (research dossier, P22 "Cost").
- Fiscal sponsors and DFI programmes are the wedge because they already hold funds on
  behalf of others and are measured on disbursement speed; foundations are more conservative.
- Vendors in Senegal, Kenya or Nigeria can be paid at a Stellar address or through an anchor
  to mobile money; this is documented ecosystem capability, not something we exercised.

### Hypotheses

- H1 (timing): approval-to-cash falls from 2-6 weeks to the same day when release is a
  contract call and the money is already escrowed.
- H2 (control): pre-authorised spend within category caps eliminates unreconciled spend at
  audit (X% -> 0%) because every payment is categorised before it happens.
- H3 (reviewer time): the evidence checklist halves reviewer minutes per milestone.
- H4 (adoption): a grants manager prefers a live ledger of categorised payments over
  quarterly receipt bundles, and will accept a wallet policy in place of a reimbursement
  policy.

### Simulated / demo data

`data/seed/`: a synthetic Senegalese health-research grant (USDC 37,500 in three milestones),
a five-line budget, eight vendors (one blocked), twenty vendor payments with messy amounts,
and four milestone reports written to exercise complete, partial and empty evidence. Names,
addresses and amounts are invented; the Stellar addresses were generated locally and hold
nothing.

### Actual validation

Technical validation: done on testnet, see below. User validation: none yet. No interviews, no
pilot, no funder, no grantee, no measurement; the team is the only user so far.

## Testnet end-to-end run (executed 2026-11-05 UTC)

The team ran both contracts on Stellar testnet (protocol 30) through the `grantflow` CLI with
throwaway keys (funder, grantee, three reviewers, one vendor), a wrapped test asset `USDT`
(7 decimals) and wasm built with `stellar contract build`. Every row is a transaction anyone can
open on stellar.expert.

| Item | Value |
|---|---|
| `grant_escrow` | [`CCXLCGEYREDFVFKSHDZCCDS7O4R5BMPPGFQM2FHWYE6SGJ2NLIEE5ZKO`](https://stellar.expert/explorer/testnet/contract/CCXLCGEYREDFVFKSHDZCCDS7O4R5BMPPGFQM2FHWYE6SGJ2NLIEE5ZKO) |
| `policy_wallet` | [`CCPPVWREMX53MUVG5MTPYZ3Q3ZBVCB2KUIW6WOQDTHWV3IRIZLETQJ7M`](https://stellar.expert/explorer/testnet/contract/CCPPVWREMX53MUVG5MTPYZ3Q3ZBVCB2KUIW6WOQDTHWV3IRIZLETQJ7M) |
| USDT token (SAC) | [`CC4E5PF63ZTFTY2SD2ADXJ5X2FF7YDVAAFWUT2DQO54IVFTDP4RQTJ63`](https://stellar.expert/explorer/testnet/contract/CC4E5PF63ZTFTY2SD2ADXJ5X2FF7YDVAAFWUT2DQO54IVFTDP4RQTJ63) |
| wasm sizes | `grant_escrow.wasm` 20,376 bytes, `policy_wallet.wasm` 22,175 bytes |

**Grant 0: the seed journey** (3 milestones, 2-of-3 reviewers, 37,500.00 funded).

| Step | CLI command | Result |
|---|---|---|
| Create grant | `create --submit` | [`e94d04b2…`](https://stellar.expert/explorer/testnet/tx/e94d04b2599a49a1cbfa0da9dfecf276597fdbf7e3a467364c0f6750125684b6), id 0 |
| Fund 37,500.00 | `fund --submit` | [`475ee5d8…`](https://stellar.expert/explorer/testnet/tx/475ee5d8d877fb95192f1dfcd0e47b63c649e12639d8b217b4630d1cefd2f39e) |
| Reviewer rejects a submission whose evidence hash was wrong | `review --reject --submit` | [`b679a6ed…`](https://stellar.expert/explorer/testnet/tx/b679a6ed70a28d7c0d6437d9f6fc41f8ba600289a8b3b754ebc42b8a5fa5f13e) (milestone back to `Pending`) |
| Grantee submits the real milestone-1 report through the wallet | `submit --submit` | [`c0743eba…`](https://stellar.expert/explorer/testnet/tx/c0743eba9ee4c5add7690fa2a3178221238a5123d7e21b03f2a32afbeb372531); grantee key signs the wallet auth entry |
| Two reviewers approve | `review --approve --submit` x2 | [`ae07d0f0…`](https://stellar.expert/explorer/testnet/tx/ae07d0f0fe97c4d488633ff2ad5808d192f40c0f5061a3954177b762b5646ca2), [`c22e9e1a…`](https://stellar.expert/explorer/testnet/tx/c22e9e1a6014c254e94383cd69e58f9f792fd9ca8f06e8100f0d4d5ea4f6f5ab); quorum reached, 12,500.00 released |
| Vendor payment 340.75, category fieldwork | `pay --submit` | [`0958ed92…`](https://stellar.expert/explorer/testnet/tx/0958ed9226cdf7d863c9930fc57afee1ad5cc0f0f2577a9823c1bf058cb1f62b); wallet `ledger` shows fieldwork spent 340.75 |

Observed afterwards: milestone 0 state `Released`, grant `released` 12,500.00, wallet
`total_spent` 340.75, evidence checker 5 of 5 on the milestone-1 report.

**Grants 1 to 3: the funder-side commands added in this round.**

| Step | CLI command | Result |
|---|---|---|
| Create then cancel an unfunded grant | `cancel` | [`3565fa71…`](https://stellar.expert/explorer/testnet/tx/3565fa71ce67a6e7d1bf681b7a4230854168de38f07ca5d074c492a3f254b501), [`b83ce0f9…`](https://stellar.expert/explorer/testnet/tx/b83ce0f99506943deb7b2f847eaa878fb52150061a1cf4c1508ab95de32f6482) |
| Extend a deadline (grant 2) | `extend-deadline` | [`aca2377a…`](https://stellar.expert/explorer/testnet/tx/aca2377ac3aae6fe16506c8938886cf7b92617fecdd67f1b0bd3fe682b309375) |
| Create grant 3 (1,000.00, 150 s deadline) and fund it | `create`, `fund` | [`d70866b6…`](https://stellar.expert/explorer/testnet/tx/d70866b6b0a3880814741dc1c59f20f902a3dc8901b4f94358a8e32c3b4f7021), [`48d197d5…`](https://stellar.expert/explorer/testnet/tx/48d197d5a72f51f25dab233f3285f0489a917129c0884266029ce438d30573d0) |
| Funder reclaims after the deadline | `clawback` | [`f17a538f…`](https://stellar.expert/explorer/testnet/tx/f17a538f33090385878e45b91c4ece7123f27c4038ce126a253632f105186eac); 1,000.00 returned, grant closed |

**What testing on the real network found and fixed.**
- The wallet-signed calls (`submit_evidence`, `pay`) failed on-chain with a resource-limit error
  (first failure: [`b57e5a32…`](https://stellar.expert/explorer/testnet/tx/b57e5a3284ba38710b9762f0f833b9e1c3497989124093856ade5287df67aaca)). The first simulation ran before the grantee's signature
  existed, so it did not budget for the wallet's `__check_auth`. `app/src/chain/client.ts` now
  re-simulates after signing the auth entry (about 1.6M to 2.9M instructions) and then assembles
  and sends. The offline tests could not catch this.
- `@stellar/stellar-sdk` 17.1.0 could not parse protocol-30 ledger headers; the app now requires 17.4.
- During debugging, one scratch submission recorded a wrong evidence hash on milestone 0; that is
  why the run above includes the reviewer rejection before the real resubmission.

Not exercised on testnet: the standalone `release` command (approval released the milestone directly),
milestones 1 and 2, `set_policy`, `rotate_key`, `add_payee` and
`remove_payee` (covered by the contract tests only). `scripts/deploy-testnet.sh` itself was not
run; the same steps were performed by hand with stellar-cli 25.2.


## Baseline and success metric

| Metric | Baseline (researched) | Target | How measured |
|---|---|---|---|
| Approval-to-cash lead time | 2-6 weeks through prime and correspondent banks | same day | timestamp of `milestone_approved` vs. `milestone_released`/vendor `paid` events; baseline from the sponsor's records for the previous grant |
| Unreconciled spend at audit | X% of a tranche (sponsor-specific) | 0% | every `paid` event carries a category; compare the exported ledger to the sponsor's audit findings |
| Reviewer time per milestone | hours (sponsor-specific) | 50% lower with the checklist | reviewer self-reported minutes with and without the checklist on alternating milestones |
| Payments blocked by policy | n/a | > 0 in the pilot, each explained | count of rejected `pay` attempts by reason |

## Experiment: one small grant with a fiscal sponsor

1. Recruit five grants managers at fiscal sponsors or foundations funding African or South
   Asian institutions; 30-minute interviews on their reimbursement cycle, audit findings and
   what "pre-authorised spend" would change. Exit criterion: at least two say they would run
   a pilot.
2. Pilot a USD 5-20k grant with one fiscal sponsor and one grantee institution: escrow on
   testnet with a test asset first (`scripts/deploy-testnet.sh`), then mainnet USDC with an
   anchor for vendor cash-out. Three milestones, a 2-of-3 review committee, the sponsor sets
   the wallet policy from the approved budget.
3. Measure the four metrics above over the grant. Compare with the sponsor's previous grant to
   the same institution.
4. Kill criteria: the sponsor's counsel refuses custody of USDC by the grantee; vendors
   cannot be paid (no anchor route, no wallet); reviewer time does not fall; or the grantee
   institution needs cash for costs the policy cannot express.

## Killer questions

1. **Would the user care if it disappeared?** Untested. Plausible from evidence: the pain
   is cash timing for the grantee and reconciliation for the funder; both are documented
   (P22). Nobody has used this.
2. **Did anyone outside the team use it?** No. The testnet run above was performed by the team.
3. **Before/after measured?** No. Baselines are researched ranges, not measurements; the
   metrics and instruments are defined above.
4. **Value lost without AI?** Small by design. Without the evidence checker the escrow,
   wallet and ledger work unchanged; reviewers read reports as today. The checker is a triage
   aid whose value (H3) is unmeasured. If the model is wrong the cost is a reviewer's minute
   (false "missing") or a misleading "found" that the mandatory citation exposes; the
   heuristic fallback and the verbatim-quote check are the guard rails.
5. **Reason to keep using after demo?** Hypothesis: each new milestone and each new grant
   repeats the same release and payment cycle; the funder's ledger only has value if all
   payments go through it.
6. **Would someone pay?** Hypothesis: 0.5-1% of flow or a per-grant SaaS fee paid by the
   funder, justified by reconciliation hours and leakage; no price test yet.
7. **Does the chain create value?** Yes, argued: the release rule (quorum) and the budget
   rule (policy wallet) are executed rather than promised, and funder, prime and grantee
   share one state without a bank in the middle. A database could hold the same ledger but
   could not stop a payment.
8. **Why this chain?** USDC with local anchors for the grantee's fiat exit, custom
   accounts as the policy-signer primitive, sub-cent fees on many small payments. Arbitrum
   has forkable grant contracts (Allo) but no fiat exit for the grantee.
