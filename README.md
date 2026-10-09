# GrantFlow

Milestone grant disbursement into policy-constrained sub-grantee wallets on Stellar/Soroban.

A funder escrows a grant in USDC, reviewers release each milestone by quorum, and the
money lands in a **policy wallet** that only lets the grantee pay allowlisted vendors
within per-category budget caps. Every payment emits an event with its budget category, so
the funder has a live ledger instead of a pile of receipts to reconcile months later.

## Problem

Sub-grantees in low- and middle-income countries must spend first and claim reimbursement
in arrears; funds arrive weeks late through correspondent banks and get diverted from
programmes in the meantime, while funders audit receipts after the fact
(`research/05-problems-infra-data-ai-climate.md`, P22; `research/08-final-selection.md`, 02).
Grant-management tools (Fluxx, SmartSimple, Submittable, Open Collective) stop at the approval
step; the money still takes the slow path and spending control is retrospective.

**User:** principal investigator and finance officer at the sub-grantee institution.
**Problem owner / buyer:** funder programme officer, prime-institution grants manager,
fiscal sponsors, DFI programmes.

## What the MVP does

| Piece | What it is |
|---|---|
| `contracts/grant_escrow` | Soroban contract: grants with milestones, reviewer set and quorum; `create_grant`, `fund`, `submit_evidence`, `approve` (release at quorum), `release`, `reject`, `clawback` after the deadline, `grant`/`approvals` views. Events on every transition. |
| `contracts/policy_wallet` | Soroban **custom account** (`__check_auth`) owned by the grantee key. `pay(category, payee, amount, memo_hash)` is the only thing the grantee key can authorise, and only within the funder's policy: allowlisted payees, per-payment ceiling, spend cap per category. Funder-only `set_policy`, `add_payee`, `remove_payee`, `rotate_key`. `policy()` and `ledger()` views. |
| `app/` | TypeScript CLI `grantflow create\|fund\|submit\|review\|pay\|ledger\|export` composing transactions offline from the embedded contract specs (submits only when an RPC is reachable), plus the **evidence checker** (`app/src/ai/`): given a milestone spec and a report, one `found / missing / uncertain` verdict per deliverable with a citation. Heuristic provider by default; LLM provider when `LLM_API_KEY` is set. Reviewers decide; the checker never signs. |
| `data/seed/` | A realistic 3-milestone grant (USDC 37,500) for a Senegalese health-research institution, a 5-line budget, 8 vendors (one blocked), 20 messy vendor payments, and four milestone reports. |
| `scripts/` | `build.sh` (tests, wasm, spec embedding, app tests), `gen-specs.sh`, `deploy-testnet.sh` (deployed successfully to testnet). |

Docs: [ARCHITECTURE.md](ARCHITECTURE.md) (contracts, auth flow, trust assumptions),
[VALIDATION.md](VALIDATION.md) (evidence, metric, experiment plan), [DEMO.md](DEMO.md)
(exact commands and expected output).

## Quickstart

Prerequisites: Rust 1.94 with target `wasm32v1-none`, stellar-cli 28, Node 22 (see
`/TOOLCHAIN.md`).

```sh
cd stellar/grantflow
cargo test                       # 43 contract tests in the Soroban host, incl. the full scenario
stellar contract build           # target/wasm32v1-none/release/{grant_escrow,policy_wallet}.wasm
./scripts/gen-specs.sh           # embed the contract specs into app/src/chain/specs.ts
cd app && npm install && npm test   # 26 offline tests: evidence checker, CSV export, CLI parsing, tx composition
```

Or run everything at once with `./scripts/build.sh`.

Try the CLI offline (every command prints the composed invocation and the equivalent
`stellar contract invoke` command; nothing is sent without `--submit` and a reachable
`SOROBAN_RPC_URL`):

```sh
cd app
node dist/src/cli.js create --grant ../data/seed/grant.json
node dist/src/cli.js review --grant-id 0 --milestone 1 --report ../data/seed/reports/m2-report-v1.md
node dist/src/cli.js pay --batch ../data/seed/payments.csv
node dist/src/cli.js ledger --simulate ../data/seed/payments.csv
node dist/src/cli.js export --simulate ../data/seed/payments.csv --out ledger.csv
```

To point the CLI at a network, contract ids and signing keys, export the variables from
`.env.example` or copy it to `app/.env` and run `node --env-file=.env dist/src/cli.js ...`.

## What the tests prove

- **grant_escrow (19 tests):** every function, every auth check with `mock_auths` and
  `env.auths()` assertions, every deadline rule, quorum release, rejection and
  resubmission, clawback closing the grant, and `scenario_three_milestones`: create and
  fund (two tranches), milestone 1 released to the real policy wallet, 20 seed payments with
  the over-cap and the non-allowlisted payment rejected, milestone 2 rejected then
  resubmitted and approved, deadline passes and the funder claws back milestone 3.
- **policy_wallet (24 tests):** policy checks in contract order, funder-only administration,
  `__check_auth` with real Ed25519 signatures (valid, wrong key, wrong payload, wrong
  function, foreign contract, create-contract context, empty context), and end-to-end
  through the host with a signed `SorobanAuthorizationEntry`: a payment succeeds, the same
  entry cannot be replayed, a forged signature fails, and a grantee signature over
  `token.transfer` cannot move funds around the policy.
- **app (26 tests):** amount parsing of messy input, seed consistency, the offline policy
  simulation reproducing the contract scenario (18 accepted, 2 rejected), the evidence
  checker on complete / partial / empty fixtures, strict-JSON validation and retry of the
  LLM provider with a stubbed client, RPC event decoding and CSV shape, CLI parsing, and
  transaction composition (including the custom-account signature format).

## Status

**functional locally.** Contracts execute in the Soroban host via `cargo test`; wasm builds
for `wasm32v1-none`; the app and its tests run offline. The contracts are deployed to Testnet: `grant_escrow` at [CBSOIBLTRBUFLZFJZA6JOFDD2FUCWNWSOOH3XAM2QJJOW4HQAVWGXYPN](https://stellar.expert/explorer/testnet/contract/CBSOIBLTRBUFLZFJZA6JOFDD2FUCWNWSOOH3XAM2QJJOW4HQAVWGXYPN) and `policy_wallet` at [CC4UKCMJEU5SXSQ74CKIUO6DEAKSDQDFADDM2WPAZ46EYN6J2CUWDIGL](https://stellar.expert/explorer/testnet/contract/CC4UKCMJEU5SXSQ74CKIUO6DEAKSDQDFADDM2WPAZ46EYN6J2CUWDIGL). No users, no pilot, no partner: see [VALIDATION.md](VALIDATION.md).
