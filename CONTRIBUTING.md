# Contributing

## Setup

- Rust with the `wasm32v1-none` target, `stellar-cli`, and Node 22.
- `cargo test` runs both contracts inside the Soroban host.
- `cd app && npm ci && npm test` runs the app tests; they need no network and no keys.
- `scripts/build.sh` runs both suites, builds the wasm files and re-embeds the contract specs.

## Pull requests

- One change per pull request, with tests for new behaviour. Every contract auth rule has
  a negative test using explicit `mock_auths`; keep it that way.
- If a contract interface changes, run `scripts/gen-specs.sh` and commit
  `app/src/chain/specs.ts`, and update `ARCHITECTURE.md`. `test_snapshots/` are regenerated
  by `cargo test`.
- Commit messages are short and conventional (`feat:`, `fix:`, `docs:`, `ci:`).
- CI must pass: `cargo test` and `npm test`.

## Scope

Money-moving rules live in `contracts/`. The app composes and optionally submits
transactions and runs the evidence checker; the checker advises reviewers and never signs.
