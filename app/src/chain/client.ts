/**
 * Transaction composition for both contracts.
 *
 * Offline (no RPC): arguments are converted with the embedded contract spec
 * (`contract.Spec.funcArgsToScVals`) into an `invokeContractFunction`
 * operation, printed as XDR next to the equivalent `stellar contract invoke`
 * command line. Online (`SOROBAN_RPC_URL` set and reachable): the same call
 * goes through `contract.Client`, is simulated, signed and sent.
 */
import {
  Account,
  BASE_FEE,
  Keypair,
  Operation,
  StrKey,
  TransactionBuilder,
  authorizeEntry,
  contract,
  rpc,
  xdr,
  type Transaction,
} from "@stellar/stellar-sdk";

/** Wallet functions whose authorization comes from the grantee's Ed25519 key. */
export const WALLET_GRANTEE_FUNCTIONS = new Set(["pay", "submit_evidence"]);

/**
 * Sign an authorization entry for the policy wallet, a custom account: the
 * grantee key signs the Soroban auth payload and the signature travels as a
 * plain 64-byte ScVal (the wallet's `Signature = BytesN<64>`), not as the
 * `[ { public_key, signature } ]` vector a G account uses. The host then
 * invokes the wallet's `__check_auth` with this payload and signature.
 */
export function authorizeWalletEntry(
  entry: xdr.SorobanAuthorizationEntry,
  granteeKeypair: Keypair,
  validUntilLedgerSeq: number,
  networkPassphrase: string,
  walletAddress: string,
): Promise<xdr.SorobanAuthorizationEntry> {
  return authorizeEntry(
    entry,
    async (_preimage, payload) => ({
      signatureScVal: xdr.ScVal.scvBytes(granteeKeypair.sign(Buffer.from(payload))),
      address: walletAddress,
    }),
    validUntilLedgerSeq,
    networkPassphrase,
    walletAddress,
  );
}
import { GRANT_ESCROW_SPEC, POLICY_WALLET_SPEC } from "./specs.js";
import type { Config } from "../config.js";

export type ContractName = "grant_escrow" | "policy_wallet";

const specCache = new Map<ContractName, contract.Spec>();

export function specFor(name: ContractName): contract.Spec {
  let spec = specCache.get(name);
  if (!spec) {
    spec = new contract.Spec(name === "grant_escrow" ? GRANT_ESCROW_SPEC : POLICY_WALLET_SPEC);
    specCache.set(name, spec);
  }
  return spec;
}

export interface Invocation {
  contract: ContractName;
  contractId: string;
  fn: string;
  args: Record<string, unknown>;
  scArgs: xdr.ScVal[];
  /** Base64 XDR of the invokeContractFunction operation. */
  operationXdr: string;
  /** Base64 XDR of an unsigned transaction envelope built on a placeholder account. */
  transactionXdr: string;
  /** Equivalent stellar-cli command. */
  cli: string;
}

/** A syntactically valid placeholder contract id for offline composition. */
export const PLACEHOLDER_CONTRACT_ID = StrKey.encodeContract(Buffer.alloc(32));
export const PLACEHOLDER_SOURCE = StrKey.encodeEd25519PublicKey(Buffer.alloc(32));

function cliArg(value: unknown): string {
  if (typeof value === "bigint") return String(value);
  if (typeof value === "string") return value;
  if (value instanceof Uint8Array || Buffer.isBuffer(value)) return Buffer.from(value).toString("hex");
  return `'${JSON.stringify(value, (_k, v) => (typeof v === "bigint" ? String(v) : v instanceof Uint8Array ? Buffer.from(v).toString("hex") : v))}'`;
}

/** Build the invocation without touching the network. */
export function compose(
  name: ContractName,
  contractId: string,
  fn: string,
  args: Record<string, unknown>,
  networkPassphrase: string,
  sourcePublicKey: string = PLACEHOLDER_SOURCE,
): Invocation {
  const spec = specFor(name);
  const scArgs = spec.funcArgsToScVals(fn, args);
  const operation = Operation.invokeContractFunction({ contract: contractId, function: fn, args: scArgs });
  const source = new Account(sourcePublicKey, "0");
  const tx: Transaction = new TransactionBuilder(source, { fee: BASE_FEE, networkPassphrase })
    .addOperation(operation)
    .setTimeout(300)
    .build();
  const cliArgs = Object.entries(args)
    .map(([k, v]) => `--${k} ${cliArg(v)}`)
    .join(" ");
  return {
    contract: name,
    contractId,
    fn,
    args,
    scArgs,
    operationXdr: operation.toXDR("base64"),
    transactionXdr: tx.toXDR(),
    cli: `stellar contract invoke --id ${contractId} --network testnet --source-account <key> -- ${fn} ${cliArgs}`,
  };
}

/** True when an RPC endpoint is configured and answers getHealth. */
export async function rpcReachable(config: Config): Promise<boolean> {
  if (!config.rpcUrl) return false;
  try {
    const server = new rpc.Server(config.rpcUrl, { allowHttp: config.rpcUrl.startsWith("http:") });
    const health = await server.getHealth();
    return health.status === "healthy";
  } catch {
    return false;
  }
}

/** Simulate, sign and send through contract.Client. Only called with a reachable RPC. */
export async function submit(
  config: Config,
  name: ContractName,
  contractId: string,
  fn: string,
  args: Record<string, unknown>,
  signerSecret: string,
): Promise<{ hash: string; result: unknown }> {
  if (!config.rpcUrl) throw new Error("SOROBAN_RPC_URL is not set");
  const keypair = Keypair.fromSecret(signerSecret);
  const signer = contract.basicNodeSigner(keypair, config.networkPassphrase);
  const client = new contract.Client(specFor(name), {
    contractId,
    networkPassphrase: config.networkPassphrase,
    rpcUrl: config.rpcUrl,
    allowHttp: config.rpcUrl.startsWith("http:"),
    publicKey: keypair.publicKey(),
    signTransaction: signer.signTransaction,
    signAuthEntry: signer.signAuthEntry,
  });
  const method = (client as unknown as Record<string, (a: Record<string, unknown>) => Promise<contract.AssembledTransaction<unknown>>>)[fn];
  if (typeof method !== "function") throw new Error(`function ${fn} not in the ${name} spec`);
  const assembled = await method.call(client, args);
  if (name === "policy_wallet" && WALLET_GRANTEE_FUNCTIONS.has(fn)) {
    // The wallet (contract address) must authorize; the grantee key signs for it.
    await assembled.signAuthEntries({
      address: contractId,
      authorizeEntry: (entry, _signer, validUntil, passphrase) =>
        authorizeWalletEntry(entry, keypair, validUntil, passphrase, contractId),
    });
  }
  const sent = await assembled.signAndSend();
  const hash = sent.sendTransactionResponse?.hash ?? "";
  return { hash, result: sent.result };
}

/** Fetch `paid` events of the wallet from RPC. */
export async function fetchWalletEvents(config: Config, walletId: string, startLedger?: number): Promise<unknown[]> {
  if (!config.rpcUrl) throw new Error("SOROBAN_RPC_URL is not set");
  const server = new rpc.Server(config.rpcUrl, { allowHttp: config.rpcUrl.startsWith("http:") });
  const latest = await server.getLatestLedger();
  const from = startLedger ?? Math.max(1, latest.sequence - 17_280 * 7);
  const res = await server.getEvents({
    startLedger: from,
    filters: [{ type: "contract", contractIds: [walletId], topics: [["*", "*", "*"]] }],
    limit: 1000,
  });
  return res.events.map((e) => ({
    ledger: e.ledger,
    ledgerClosedAt: e.ledgerClosedAt,
    contractId: e.contractId?.toString(),
    id: e.id,
    txHash: e.txHash,
    topic: e.topic.map((t) => t.toXDR("base64")),
    value: e.value.toXDR("base64"),
  }));
}
