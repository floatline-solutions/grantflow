# Changelog

## Unreleased

### Added
- CLI escrow lifecycle commands: `release`, `clawback`, `extend-deadline`, `cancel`, with option validation.
- GitHub Actions CI (contract tests and app tests), issue and pull request templates, Dependabot config.
- `LICENSE` (Apache-2.0), `CONTRIBUTING.md`, `SECURITY.md`, `.gitignore`.

### Fixed
- Wallet-signed transactions (`submit_evidence`, `pay`) failed on a live network with a resource-limit error;
  the app now re-simulates after the grantee signs the wallet auth entry.
- Require `@stellar/stellar-sdk` 17.4 (17.1 could not parse protocol-30 ledger headers).
- Documentation referenced files that do not exist and used a wrong quickstart path.

### Validated
- Full grant journey executed on Stellar testnet; hashes in `VALIDATION.md`.

## 0.1.0
- `grant_escrow` and `policy_wallet` Soroban contracts, `grantflow` CLI with evidence checker, seed data and offline tests.
