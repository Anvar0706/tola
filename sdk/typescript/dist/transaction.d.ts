import { Address } from './address.js';
import { UnsignedTransaction, Transaction, HexString } from './types.js';

export declare const MAX_MEMO_BYTES: number;

export declare function serializeUnsignedTransaction(tx: UnsignedTransaction): Uint8Array;
export declare function serializeSignedTransaction(tx: Transaction): Uint8Array;
export declare function computeTransactionDigest(tx: UnsignedTransaction): Uint8Array;

export declare class TransactionBuilder {
  constructor(chainId?: bigint | number);
  chainId(id: bigint | number): this;
  nonce(n: bigint | number): this;
  to(addr: string | Address | null): this;
  value(val: bigint | number): this;
  gasLimit(gas: bigint | number): this;
  maxFeePerGas(fee: bigint | number): this;
  maxPriorityFeePerGas(priorityFee: bigint | number): this;
  validUntilBlock(height: bigint | number): this;
  data(data: Uint8Array | HexString): this;
  /** Set optional memo (UTF-8, max 256 bytes). Empty string or null clears memo. */
  memo(text: string | null): this;
  build(fromAddress: string | Address): UnsignedTransaction;
}

