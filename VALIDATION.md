# GrantFlow validation

What we know, what we assume, what we built, and what would prove or kill the idea.
Nothing in this file is a claim of adoption: no user, funder, interview, pilot or partner
exists yet.

## Evidence tiers

### Researched evidence

- Sub-grantees in LMICs pre-finance activities and claim reimbursement in arrears;
  "payment in arrears and the need to locally fund research activities upfront can result in
  funding being diverted from health programmes until reimbursement" (Lancet Global Health,
  cited in `research/05-problems-infra-data-ai-climate.md`, P22).
- Donor disbursements in Sub-Saharan Africa are "extremely low/slow" or "stop-go" (MPRA
  87106, same dossier); African universities cite irregular budget disbursement and weak
  grant-management systems (Frontiers in Education 2026); Senegalese students went unpaid
  because of grant delays (Standard Media) — all P22.
- Incumbents stop at approval: Fluxx, SmartSimple, Submittable manage workflows, not
  cross-border money; Open Collective is a fiscal host on Stripe/PayPal rails; Allo v2 is in
  maintenance mode; DeSci DAOs fund crypto-native work; StellarGrantProtocol has milestone
  escrow but no funder (`research/05`, P22 alternatives; `research/08-final-selection.md`, 02).
- Stellar has USDC, anchors in KE/NG/GH/UG and MoneyGram cash-out, custom accounts
  (`__check_auth`) as a policy-signer primitive, and sub-cent fees (`research/08`, 02, "Why
  Stellar"; `research/01-stellar-ecosystem.md`).

### Observed facts (this repository)

- Both contracts execute in the Soroban host: 43 tests pass (`cargo test`), including the
  full 3-milestone journey with 20 vendor payments and real Ed25519 signatures through
  `__check_auth`; wasm builds at 16.7 KB (escrow) and 18.4 KB (wallet).
- The app's 26 tests pass offline; the policy simulation in TypeScript produces the same 18
  accepted / 2 rejected outcome as the contract on the same seed CSV.
- On the seed reports the heuristic evidence checker marks the complete milestone-1 report 5/5
  found, the draft milestone-2 report 1 found / 3 uncertain / 1 missing (the three "will be
  delivered" promises are flagged as such), and the empty milestone-3 placeholder 0/5.
- Testnet deployment was **not** performed; the sandbox had no route to Stellar testnet.

### Team assumptions

- A milestone costs dozens of person-hours across funder, prime and grantee (reconciliation,
  receipts, wires); the scale is unquantified in public sources (`research/05`, P22 "Cost").
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

None yet. No interviews, no pilot, no funder, no grantee, no measurement.

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
2. **Did anyone outside the team use it?** No.
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
