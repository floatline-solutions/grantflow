import { Networks } from "@stellar/stellar-sdk";

export interface Config {
  rpcUrl: string | null;
  networkPassphrase: string;
  escrowId: string | null;
  walletId: string | null;
  tokenId: string | null;
  funderSecret: string | null;
  granteeSecret: string | null;
  reviewerSecret: string | null;
  seedDir: string | null;
  llmConfigured: boolean;
}

/** Every variable the app reads; see .env.example. */
export function loadConfig(env: NodeJS.ProcessEnv = process.env): Config {
  const s = (k: string): string | null => {
    const v = env[k]?.trim();
    return v ? v : null;
  };
  return {
    rpcUrl: s("SOROBAN_RPC_URL"),
    networkPassphrase: s("STELLAR_NETWORK_PASSPHRASE") ?? Networks.TESTNET,
    escrowId: s("GRANT_ESCROW_ID"),
    walletId: s("POLICY_WALLET_ID"),
    tokenId: s("USDC_CONTRACT_ID"),
    funderSecret: s("FUNDER_SECRET"),
    granteeSecret: s("GRANTEE_SECRET"),
    reviewerSecret: s("REVIEWER_SECRET"),
    seedDir: s("SEED_DIR"),
    llmConfigured: s("LLM_API_KEY") !== null,
  };
}
