# GrantFlow demo

Everything below runs offline and was executed in this repository. Outputs are trimmed
where marked `...`. Contract ids show the placeholder `CAAAA...BSC4` until you set
`GRANT_ESCROW_ID` / `POLICY_WALLET_ID` (or pass `--escrow` / `--wallet`); nothing is sent
without `--submit` and a reachable `SOROBAN_RPC_URL`.

## 1. Contracts: the whole journey in the Soroban host

```sh
cd stellar/grantflow
cargo test
```

Expected tail:

```
running 19 tests            # grant_escrow
...
test test::scenario_three_milestones ... ok
test result: ok. 19 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out

running 24 tests            # policy_wallet
...
test test::pay_end_to_end_with_grantee_signed_auth_entry_and_no_replay ... ok
test test::grantee_key_cannot_move_tokens_around_the_policy ... ok
test test::check_auth_rejects_functions_other_than_pay_or_submit_evidence ... ok
test result: ok. 24 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

`scenario_three_milestones` (in `contracts/grant_escrow/src/test.rs`) creates and funds the
seed grant in two tranches, releases milestone 1 to the real policy wallet after a 2-of-3
vote, runs the 20 seed payments (payment 8 fails with `CategoryCapExceeded`, payment 14 with
`PayeeNotAllowed`; the wallet ends with 24.51 USDC), has milestone 2 rejected, resubmitted and
released, and lets the funder claw back milestone 3 after the deadline.

```sh
stellar contract build
ls -la target/wasm32v1-none/release/*.wasm
#  16682  grant_escrow.wasm
#  18427  policy_wallet.wasm
```

## 2. App: build and offline tests

```sh
./scripts/gen-specs.sh            # embeds the wasm specs into app/src/chain/specs.ts
cd app && npm install && npm test
```

Expected tail:

```
ok 14 - heuristic provider on fixture "complete"
ok 15 - heuristic provider on fixture "partial"
ok 16 - heuristic provider on fixture "empty"
...
ok 25 - payments.csv parses 20 messy rows and the policy simulation matches the contract scenario
# tests 26
# pass 26
# fail 0
```

## 3. Funder: create and fund the grant

```sh
cd app
node dist/src/cli.js create --grant ../data/seed/grant.json
```

```
grant "Community health worker incentives - Kaolack and Fatick regions": 3 milestones, quorum 2/3, total 37500.00 USDC
  #0 Baseline survey and enumerator training: 12500.00 USDC, spec sha256 208e0a2f1553510cf62272f15fd67a27ba0d526a82eed3bfb5d9ac8e695c41a7
  #1 Baseline data collection and cleaned dataset: 15000.00 USDC, spec sha256 2c744c9218cf60365c5c85d994b3c3d5ec852afa83f024e28f05d0200d67eb09
  #2 Analysis, dissemination and final report: 10000.00 USDC, spec sha256 ac8e77e3d41a4f9ce6e98b8c2c7372591a6ff730768ee50c974fcc012f60db80
{
  "contract": "grant_escrow",
  "contract_id": "CAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABSC4",
  "function": "create_grant",
  "args": { "funder": "GD6KDMPX...", "grantee_wallet": "C...", "token": "C...", "milestones": [ ... ], "reviewers": [ ... ], "quorum": 2, "deadline": "1775542400" },
  "operation_xdr": "AAAAAAAAABgAAAAA...",
  "transaction_xdr_unsigned": "AAAAAgAAAAD8...",
  "stellar_cli": "stellar contract invoke --id C... --network testnet --source-account <key> -- create_grant --funder GD6KDMPX... --milestones '[...]' --reviewers '[...]' --quorum 2 --deadline 1775542400"
}
(composed offline; add --submit with SOROBAN_RPC_URL and FUNDER_SECRET to send)
```

```sh
node dist/src/cli.js fund --grant-id 0 --amount "25,000.00" --from GD6KDMPXS3M4TR7QSV2GZCT7XMNWBCRJNDRXKEVNJ4WGEPSKL2MI3RQO
```

```
{
  "contract": "grant_escrow",
  "function": "fund",
  "args": { "id": "0", "from": "GD6KDMPX...", "amount": "250000000000" },
  ...
}
```

## 4. Grantee: submit milestone evidence through the wallet

```sh
node dist/src/cli.js submit --grant-id 0 --milestone 0 --report ../data/seed/reports/m1-report.md
```

```
report ../data/seed/reports/m1-report.md: sha256 8b970ab7b6d3635c5026bbf5ca64a0aefe08127a2eb7b1659cd2747385f5a82a
{
  "contract": "policy_wallet",
  "function": "submit_evidence",
  "args": { "escrow": "C...", "grant_id": "0", "idx": 0, "evidence_hash": "8b970ab7b6d3635c5026bbf5ca64a0aefe08127a2eb7b1659cd2747385f5a82a" },
  ...
}
(composed offline; add --submit with SOROBAN_RPC_URL and GRANTEE_SECRET to send)
```

With `--submit`, the CLI signs the wallet's authorization entry with the grantee key in the
custom-account format (`authorizeWalletEntry`), which is what `__check_auth` verifies.

## 5. Reviewer: evidence checklist, then approve or reject

Milestone 1, complete report:

```sh
node dist/src/cli.js review --grant-id 0 --milestone 0 --report ../data/seed/reports/m1-report.md
```

```
Evidence check (heuristic): 5 of 5 deliverables found.
[found]     #1 Enumerator training completed for at least 12 enumerators, with attendance sheet.  (confidence 0.95)
            > We trained fourteen enumerators (12 were required), translated and pre-tested the instrument, drew the sample frame, obtained ethics clearance and planned the field work by district.
[found]     #2 Survey instrument translated into Wolof and pre-tested with 30 respondents.  (confidence 0.95)
            > The Wolof version was pre-tested with 30 respondents in two non-sample villages (Ndoffane and Gandiaye) on 12-13 November; the pre-test led to 11 wording changes, listed in Annex B.
[found]     #3 Baseline sample frame of 600 households drawn from the 2023 census list, with sampling weights.  (confidence 0.95)
            > Using the 2023 census enumeration-area list, we drew a baseline sample frame of 600 households (300 per region) ...
[found]     #4 Ethics approval letter from the CNERS committee attached.  (confidence 0.95)
            > The CNERS (Comite National d'Ethique pour la Recherche en Sante) approval letter, reference 000187/MSAS/CNERS, dated 29 October 2025, is attached as Annex C.
[found]     #5 Field data collection plan with dates per district (Kaolack, Fatick).  (confidence 0.95)
            > Data collection is planned as follows: Kaolack department 24 November - 12 December 2025; Fatick department 15 December 2025 - 9 January 2026 ...

No decision requested (--approve or --reject --reason <text>). The checklist is advice; a reviewer signs the decision.
```

Milestone 2, first draft (rejected in the scenario):

```sh
node dist/src/cli.js review --grant-id 0 --milestone 1 --report ../data/seed/reports/m2-report-v1.md \
  --reject --reason "dataset, data-quality report and financial reconciliation missing" \
  --reviewer GBZ7PORQTFGXF2XQ5ZWQXCHOQSHPS6D7ESPCXGXXGXHNU3BARNSQQSCR
```

```
Evidence check (heuristic): 1 of 5 deliverables found, 3 uncertain, 1 missing. Needs reviewer attention: #2 (uncertain), #3 (missing), #4 (uncertain), #5 (uncertain).
[found]     #1 Baseline survey completed with at least 540 of 600 households (90% response rate).  (confidence 0.92)
            > The survey was completed with 561 households out of the 600 sampled (93.5% response rate).
[uncertain] #2 Cleaned dataset delivered in CSV with a codebook.  (confidence 0.50)
            > Cleaning is ongoing and the cleaned dataset will be delivered together with the codebook in the next two weeks.
            note: matched text reads like a promise ("will be"), not delivered evidence
[missing]   #3 Data quality report: back-check results on 10% of interviews and GPS coverage map.  (confidence 0.66)
[uncertain] #4 Interim financial report reconciling the category ledger with vendor invoices.  (confidence 0.50)
            > Invoices for the period are collected and will be reconciled with the ledger in the interim financial report.
            note: matched text reads like a promise ("will be"), not delivered evidence
[uncertain] #5 Short summary of preliminary findings (2 pages).  (confidence 0.50)
            > A two-page summary will follow once cleaning is complete.
            note: matched text reads like a promise ("will follow"), not delivered evidence
reason sha256 af40d7cddb571f77a699d1989114fa3f62c1f8ed4f3f38326fc973ef11c75d09
{
  "contract": "grant_escrow",
  "function": "reject",
  "args": { "id": "0", "idx": 1, "reviewer": "GBZ7PORQ...", "reason_hash": "af40d7cd..." },
  ...
}
```

The revised report (`m2-report-v2.md`) scores 5 of 5 found; `--approve` composes
`approve(id, idx, reviewer)` instead. The empty milestone-3 placeholder scores 0 of 5.
With `LLM_API_KEY` set, `review` uses the LLM provider with the same output shape.

## 6. Grantee: vendor payments under the policy

One payment:

```sh
node dist/src/cli.js pay --category fieldwork --payee V-004 --amount 340.75 --memo TN-301
```

```
{
  "contract": "policy_wallet",
  "function": "pay",
  "args": { "category": "fieldwork", "payee": "GBJJZBMG...", "amount": "3407500000", "memo_hash": "52ff3d..." },
  ...
}
```

Preview the whole seed batch against the policy (same rules and order as the contract):

```sh
node dist/src/cli.js pay --batch ../data/seed/payments.csv
```

```
  2  personnel  V-001     1250.00  accepted (seq 1)
  3  personnel  V-002     2000.00  accepted (seq 2)
  4  equipment  V-003     1875.50  accepted (seq 3)
  5  fieldwork  V-004      340.75  accepted (seq 4)
  6  fieldwork  V-004      120.00  accepted (seq 5)
  7  training   V-005      450.00  accepted (seq 6)
  8  overhead   V-006      400.00  accepted (seq 7)
  9  equipment  V-003      800.00  rejected: CategoryCapExceeded
 10  personnel  V-001     1250.00  accepted (seq 8)
 11  fieldwork  V-007      615.25  accepted (seq 9)
 12  training   V-005      300.00  accepted (seq 10)
 13  personnel  V-002     1000.00  accepted (seq 11)
 14  overhead   V-006      400.00  accepted (seq 12)
 15  fieldwork  V-008      250.00  rejected: PayeeNotAllowed
 16  equipment  V-003      600.00  accepted (seq 13)
 17  training   V-005      425.50  accepted (seq 14)
 18  fieldwork  V-007      500.00  accepted (seq 15)
 19  personnel  V-001      499.99  accepted (seq 16)
 20  fieldwork  V-004      424.00  accepted (seq 17)
 21  equipment  V-003       24.50  accepted (seq 18)

ledger after batch:
  personnel  spent   5999.99 of   6000.00  (remaining 0.01)
  equipment  spent   2500.00 of   2500.00  (remaining 0.00)
  fieldwork  spent   2000.00 of   2000.00  (remaining 0.00)
  training   spent   1175.50 of   1200.00  (remaining 24.50)
  overhead   spent    800.00 of    800.00  (remaining 0.00)

18 accepted, 2 rejected or skipped
```

(The first column is the CSV line number; row 8 of the data is line 9.)

## 7. Funder: live ledger and export

```sh
node dist/src/cli.js ledger --simulate ../data/seed/payments.csv
```

```
18 payments
  personnel     5999.99 of 6000.00 (0.01 remaining)
  equipment     2500.00 of 2500.00 (0.00 remaining)
  fieldwork     2000.00 of 2000.00 (0.00 remaining)
  training      1175.50 of 1200.00 (24.50 remaining)
  overhead       800.00 of 800.00 (0.00 remaining)
  total        12475.49
```

```sh
node dist/src/cli.js export --simulate ../data/seed/payments.csv --out /tmp/ledger.csv
head -3 /tmp/ledger.csv
```

```
seq,ledger,closed_at,category,payee,amount_usdc,memo_hash,tx_hash
1,,2025-11-26,personnel,GDOOM6UV...,1250.00,50c194e7...,
2,,2025-11-26,personnel,GCH3I6IH...,2000.00,519b7ee0...,
```

Against a network, `ledger` and `export` read the wallet's `paid` events from Soroban RPC
(`--events <file.json>` accepts a saved `getEvents` response); `ledger` and `tx_hash` are then
filled in.

## 8. Testnet (not run here)

`scripts/deploy-testnet.sh` generates and funds keys, deploys a test asset and both
contracts, creates and funds the seed grant, submits milestone-1 evidence through the wallet
(signed by the grantee key), collects two approvals and makes one vendor payment. It was
written against stellar-cli 28 but **was not executed** in this environment, which had no
route to Stellar testnet, Horizon or friendbot.
