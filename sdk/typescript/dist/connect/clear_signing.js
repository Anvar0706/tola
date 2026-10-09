/**
 * Clear-signing schema and validation for TOLA Connect & Mini Apps.
 * Ensures users clearly understand domain, target, amount, fees, and memo before signing.
 */

import { Address } from '../address.js';

export class ClearSigningValidationError extends Error {
  constructor(message) {
    super(message);
    this.name = 'ClearSigningValidationError';
  }
}

/**
 * Validates a clear-signing prompt.
 * @param {Object} prompt
 */
export function validateClearSigningPrompt(prompt) {
  if (!prompt || typeof prompt !== 'object') {
    throw new ClearSigningValidationError('Prompt must be an object');
  }
  if (!prompt.domain || typeof prompt.domain !== 'string' || prompt.domain.trim().length === 0) {
    throw new ClearSigningValidationError('Domain must be a non-empty string');
  }
  if (!prompt.actionName || typeof prompt.actionName !== 'string' || prompt.actionName.trim().length === 0) {
    throw new ClearSigningValidationError('Action name must be a non-empty string');
  }
  if (!prompt.targetAddress) {
    throw new ClearSigningValidationError('Target address is required');
  }
  if (typeof prompt.amount !== 'bigint' || prompt.amount < 0n) {
    throw new ClearSigningValidationError('Amount must be a non-negative bigint');
  }
  if (!prompt.assetSymbol || typeof prompt.assetSymbol !== 'string') {
    throw new ClearSigningValidationError('Asset symbol must be a non-empty string');
  }
  if (typeof prompt.maxFee !== 'bigint' || prompt.maxFee < 0n) {
    throw new ClearSigningValidationError('Max fee must be a non-negative bigint');
  }
  if (typeof prompt.chainId !== 'bigint' && typeof prompt.chainId !== 'number') {
    throw new ClearSigningValidationError('Chain ID is required');
  }
  if (BigInt(prompt.chainId) <= 0n) {
    throw new ClearSigningValidationError('Chain ID must be greater than zero');
  }
  if (prompt.memo !== undefined && prompt.memo !== null) {
    if (typeof prompt.memo !== 'string') {
      throw new ClearSigningValidationError('Memo must be a string');
    }
    const byteLen = new TextEncoder().encode(prompt.memo).length;
    if (byteLen > 256) {
      throw new ClearSigningValidationError(`Memo exceeds maximum length of 256 bytes (got ${byteLen})`);
    }
  }
  return true;
}

/**
 * Formats a clear-signing prompt into a human-readable display string.
 * @param {Object} prompt
 * @returns {string}
 */
export function formatClearSigningSummary(prompt) {
  validateClearSigningPrompt(prompt);
  const targetStr = prompt.targetAddress instanceof Address 
    ? prompt.targetAddress.toBech32m() 
    : String(prompt.targetAddress);

  const wholeTola = Number(prompt.amount / 1000000000000000000n);
  const fraction = (prompt.amount % 1000000000000000000n).toString().padStart(18, '0').slice(0, 4);
  const memoStr = prompt.memo ? prompt.memo.trim() : '(None)';

  return [
    `=== TOLA Connect Clear-Signing Authorization ===`,
    `Domain:     ${prompt.domain.trim()}`,
    `Action:     ${prompt.actionName.trim()}`,
    `Target:     ${targetStr}`,
    `Amount:     ${wholeTola}.${fraction} ${prompt.assetSymbol.trim()}`,
    `Max Fee:    ${prompt.maxFee.toString()} gas`,
    `Chain ID:   ${prompt.chainId.toString()}`,
    `Memo:       ${memoStr}`,
  ].join('\n');
}
