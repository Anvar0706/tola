import { Address } from './address.js';

export declare const DEFAULT_NUM_SHARDS: number;
export declare const DOMAIN_RECEIPT_HASH: Uint8Array;
export declare const DOMAIN_RECEIPT_NULLIFIER: Uint8Array;

export declare function computeShardId(address: string | Address, numShards?: number): number;

export declare function computeReceiptNullifier(
  sourceShard: number,
  targetShard: number,
  receiptNonce: bigint | number,
  senderAddress: string | Address
): Uint8Array;

export declare function verifyMerkleInclusionProof(
  root: Uint8Array,
  leaf: Uint8Array,
  proof: Uint8Array[],
  index: number
): boolean;
