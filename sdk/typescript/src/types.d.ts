/**
 * Core type definitions for TOLA Network TypeScript SDK.
 */

export type HexString = `0x${string}` | string;

export interface NetworkConfig {
  name: string;
  hrp: string;
  chainId: bigint;
}

export declare const NETWORKS: Record<string, NetworkConfig>;
export declare const MAX_MEMO_BYTES: number;


export interface Secp256k1Signature {
  type: 'Secp256k1';
  r: Uint8Array; // 32 bytes
  s: Uint8Array; // 32 bytes
  v: number;     // 0 or 1
}

export interface Ed25519Signature {
  type: 'Ed25519';
  signature: Uint8Array; // 64 bytes
}

export type Signature = Secp256k1Signature | Ed25519Signature;

export interface UnsignedTransaction {
  chainId: bigint;
  nonce: bigint;
  from: string; // 20-byte hex address
  to?: string | null; // 20-byte hex address or null/undefined
  value: bigint;
  gasLimit: bigint;
  maxFeePerGas: bigint;
  maxPriorityFeePerGas: bigint;
  validUntilBlock: bigint;
  data?: Uint8Array | HexString;
  /** Optional UTF-8 memo. Max 256 bytes. null/empty string = no memo. */
  memo?: string | null;
}


export interface Transaction extends UnsignedTransaction {
  signature: Signature;
}

export interface TransactionReceipt {
  transactionHash: string;
  transactionIndex: number;
  blockHash: string;
  blockNumber: number;
  from: string;
  to: string | null;
  gasUsed: number;
  status: string;
  logs: any[];
}

export interface BlockHeader {
  height: number;
  round: number;
  epoch: number;
  parentHash: string;
  stateRoot: string;
  transactionsRoot: string;
  timestamp: number;
  proposer: string;
}

export interface NodeHealth {
  status: string;
  chainId: number;
  blockHeight: number;
  finalizedHeight: number;
  currentRound: number;
  peerCount: number;
  stateRoot: string;
  mempoolSize: number;
}
