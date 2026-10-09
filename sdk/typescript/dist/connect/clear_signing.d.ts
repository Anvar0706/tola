import { Address } from '../address.js';

export interface ClearSigningPrompt {
  domain: string;
  actionName: string;
  targetAddress: Address | string;
  amount: bigint;
  assetSymbol: string;
  maxFee: bigint;
  nonce?: bigint | number;
  chainId: bigint | number;
  memo?: string | null;
}

export declare class ClearSigningValidationError extends Error {}

export declare function validateClearSigningPrompt(prompt: ClearSigningPrompt): boolean;
export declare function formatClearSigningSummary(prompt: ClearSigningPrompt): string;
