import { NodeHealth, TransactionReceipt } from './types.js';

export interface RequestArguments {
  readonly method: string;
  readonly params?: readonly unknown[] | object;
}

export declare class JsonRpcProvider {
  readonly url: string;
  readonly timeoutMs: number;
  constructor(url?: string, timeoutMs?: number);
  send<T = any>(method: string, params?: any[]): Promise<T>;
  getChainId(): Promise<bigint>;
  getBlockNumber(): Promise<bigint>;
  getBalance(address: string, blockTag?: string): Promise<bigint>;
  getTransactionCount(address: string, blockTag?: string): Promise<bigint>;
  sendRawTransaction(signedHex: string): Promise<string>;
  getBlockByNumber(blockNum: bigint | number | string, fullTxs?: boolean): Promise<any>;
  getBlockByHash(blockHash: string, fullTxs?: boolean): Promise<any>;
  getTransactionByHash(txHash: string): Promise<any>;
  getTransactionReceipt(txHash: string): Promise<TransactionReceipt | null>;
  waitForTransactionReceipt(txHash: string, timeoutMs?: number, pollIntervalMs?: number): Promise<TransactionReceipt>;
  getHealth(): Promise<NodeHealth>;
  getValidators(): Promise<any>;
}

export declare class TolaProvider {
  constructor(rpcUrl?: string);
  request(args: RequestArguments): Promise<unknown>;
  on(event: string, listener: Function): this;
  removeListener(event: string, listener: Function): this;
  emit(event: string, ...args: any[]): boolean;
}
