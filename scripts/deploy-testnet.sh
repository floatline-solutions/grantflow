#!/usr/bin/env bash
# Deploy GrantFlow to Stellar testnet and run the seed journey.
#
# NOT EXECUTED in the environment this project was built in: the sandbox
# could not reach soroban-testnet.stellar.org, horizon or friendbot. The
# script is written against stellar-cli 28 and the contract interfaces that
# `cargo test` exercises; expect to adjust flags if the CLI changes.
#
# Prerequisites: stellar-cli 28, built wasm files (scripts/build.sh), node 22.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
NETWORK="${NETWORK:-testnet}"
WASM="$ROOT/target/wasm32v1-none/release"
SEED="$ROOT/data/seed"
ENV_OUT="${ENV_OUT:-$ROOT/app/.env}"

# USDC on testnet is a classic asset; its SAC is deployed with `stellar contract asset deploy`.
USDC_ISSUER="${USDC_ISSUER:-GBBD47IF6LWK7P7MDEVSCWR7DPUWV3NY3DTQEVFL4NAT4AQH3ZLLFLA5}"

echo "== keys (generated and funded through friendbot)"
for k in funder grantee reviewer1 reviewer2 reviewer3 vendor1; do
  stellar keys generate --network "$NETWORK" --fund "$k" 2>/dev/null || echo "key $k exists"
done
FUNDER=$(stellar keys address funder)
GRANTEE=$(stellar keys address grantee)
R1=$(stellar keys address reviewer1); R2=$(stellar keys address reviewer2); R3=$(stellar keys address reviewer3)
VENDOR1=$(stellar keys address vendor1)

echo "== token: for a self-contained demo we mint a test asset instead of real USDC"
# Real USDC would be: stellar contract asset deploy --asset USDC:$USDC_ISSUER ...
# and the funder would fund the escrow after receiving USDC from an anchor.
stellar keys generate --network "$NETWORK" --fund issuer 2>/dev/null || true
ISSUER=$(stellar keys address issuer)
TOKEN=$(stellar contract asset deploy --network "$NETWORK" --source-account issuer --asset "USDT:$ISSUER" 2>/dev/null \
  || stellar contract id asset --network "$NETWORK" --asset "USDT:$ISSUER")
echo "token SAC: $TOKEN"
# Trustlines + mint for the funder (classic payment path through the SAC admin).
stellar tx new change-trust --network "$NETWORK" --source-account funder --line "USDT:$ISSUER" >/dev/null
stellar tx new payment --network "$NETWORK" --source-account issuer --destination "$FUNDER" --asset "USDT:$ISSUER" --amount 375000000000 >/dev/null

echo "== deploy grant_escrow"
ESCROW=$(stellar contract deploy --network "$NETWORK" --source-account funder --wasm "$WASM/grant_escrow.wasm" --alias grant_escrow)
echo "escrow: $ESCROW"

echo "== deploy policy_wallet with the grantee's Ed25519 public key and the milestone-1 policy"
GRANTEE_PUBKEY_HEX=$(node -e "const {StrKey}=require('$ROOT/app/node_modules/@stellar/stellar-sdk');console.log(Buffer.from(StrKey.decodeEd25519PublicKey('$GRANTEE')).toString('hex'))")
POLICY=$(node -e "
const fs=require('fs');const b=JSON.parse(fs.readFileSync('$SEED/budget.json'));const p=JSON.parse(fs.readFileSync('$SEED/payees.json'));
const toStroops=s=>{s=s.trim().replace(/,/g,'');const [w,f='']=s.split('.');return (BigInt(w)*10000000n+BigInt(f.padEnd(7,'0'))).toString()};
console.log(JSON.stringify({categories:b.categories.map(c=>({name:c.name,cap:toStroops(c.cap_usdc),spent:'0'})),payees:['$VENDOR1'].concat(p.vendors.filter(v=>v.status==='active').map(v=>v.address)),per_tx_max:toStroops(b.per_tx_max_usdc)}))")
WALLET=$(stellar contract deploy --network "$NETWORK" --source-account funder --wasm "$WASM/policy_wallet.wasm" --alias policy_wallet \
  -- --grantee_pubkey "$GRANTEE_PUBKEY_HEX" --funder "$FUNDER" --token "$TOKEN" --policy "$POLICY")
echo "wallet: $WALLET"

echo "== create and fund the seed grant (3 milestones, 2-of-3 reviewers)"
DEADLINE=$(( $(date +%s) + 180*86400 ))
MILESTONES=$(node -e "
const fs=require('fs');const c=require('crypto');const g=JSON.parse(fs.readFileSync('$SEED/grant.json'));
const toStroops=s=>{s=s.trim().replace(/,/g,'').replace(/\s/g,'');const [w,f='']=s.split('.');return (BigInt(w)*10000000n+BigInt(f.padEnd(7,'0'))).toString()};
console.log(JSON.stringify(g.milestones.map(m=>({amount:toStroops(m.amount_usdc),spec_hash:c.createHash('sha256').update(m.spec_markdown.trim()+'\n').digest('hex'),due:String($(date +%s)+45*86400*(m.idx+1)),state:'Pending',evidence_hash:null}))))")
GRANT_ID=$(stellar contract invoke --network "$NETWORK" --source-account funder --id grant_escrow -- create_grant \
  --funder "$FUNDER" --grantee_wallet "$WALLET" --token "$TOKEN" --milestones "$MILESTONES" \
  --reviewers "[\"$R1\",\"$R2\",\"$R3\"]" --quorum 2 --deadline "$DEADLINE")
echo "grant id: $GRANT_ID"
stellar contract invoke --network "$NETWORK" --source-account funder --id grant_escrow -- fund --id "$GRANT_ID" --from "$FUNDER" --amount 375000000000

echo "== milestone 1: evidence, 2 approvals, release to the wallet"
EVIDENCE=$(node -e "const c=require('crypto');const fs=require('fs');console.log(c.createHash('sha256').update(fs.readFileSync('$SEED/reports/m1-report.md','utf8').trim()+'\n').digest('hex'))")
# The wallet is a custom account: the grantee key signs the auth entry for the wallet address.
# stellar-cli signs the source account; the auth entry for the wallet (contract address) is signed with
# the grantee key through the app (`grantflow submit --submit`, see app/src/chain/client.ts).
(cd "$ROOT/app" && GRANTEE_SECRET=$(stellar keys secret grantee) POLICY_WALLET_ID="$WALLET" GRANT_ESCROW_ID="$ESCROW" \
  SOROBAN_RPC_URL="https://soroban-testnet.stellar.org" node dist/src/cli.js submit --grant-id "$GRANT_ID" --milestone 0 --report "$SEED/reports/m1-report.md" --submit)
stellar contract invoke --network "$NETWORK" --source-account reviewer1 --id grant_escrow -- approve --id "$GRANT_ID" --idx 0 --reviewer "$R1"
stellar contract invoke --network "$NETWORK" --source-account reviewer3 --id grant_escrow -- approve --id "$GRANT_ID" --idx 0 --reviewer "$R3"
stellar contract invoke --network "$NETWORK" --source-account funder --id grant_escrow -- grant --id "$GRANT_ID"

echo "== a vendor payment from the wallet, signed by the grantee key"
(cd "$ROOT/app" && GRANTEE_SECRET=$(stellar keys secret grantee) POLICY_WALLET_ID="$WALLET" \
  SOROBAN_RPC_URL="https://soroban-testnet.stellar.org" node dist/src/cli.js pay --category fieldwork --payee "$VENDOR1" --amount 340.75 --memo TN-301 --submit)
stellar contract invoke --network "$NETWORK" --source-account funder --id policy_wallet -- ledger

cat > "$ENV_OUT" <<EOF
SOROBAN_RPC_URL=https://soroban-testnet.stellar.org
STELLAR_NETWORK_PASSPHRASE=Test SDF Network ; September 2015
GRANT_ESCROW_ID=$ESCROW
POLICY_WALLET_ID=$WALLET
USDC_CONTRACT_ID=$TOKEN
FUNDER_SECRET=$(stellar keys secret funder)
GRANTEE_SECRET=$(stellar keys secret grantee)
REVIEWER_SECRET=$(stellar keys secret reviewer1)
EOF
echo "wrote $ENV_OUT"
