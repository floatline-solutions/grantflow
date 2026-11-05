# Security policy

GrantFlow escrows and routes token balances. Please report vulnerabilities privately via
GitHub's "Report a vulnerability" (Security tab) rather than a public issue.

Status: the contracts are unaudited and intended for testnet pilots; do not use them with
real funds until an independent audit is published. Areas of most interest: the policy
wallet's `__check_auth` (signature and function allowlist), per-category cap accounting,
quorum and release logic in `grant_escrow`, and clawback after the deadline.
