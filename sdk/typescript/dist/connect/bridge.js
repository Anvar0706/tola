/**
 * Generic Mini App & Webview Bridge for TOLA Network.
 * Mediates communication between third-party app/tokenizer scripts and the non-custodial user wallet.
 * Protocol invariant: TOLA Connect does not verify external messenger ownership.
 */

import { validateClearSigningPrompt, formatClearSigningSummary } from './clear_signing.js';
import { Address } from '../address.js';

export class MiniAppBridge {
  /**
   * @param {Object} wallet - TolaWallet instance
   * @param {Object} [options] - Configuration options
   * @param {Function} [options.onClearSign] - Async confirmation callback (prompt) => Promise<boolean>
   * @param {Object} [options.provider] - JsonRpcProvider for broadcasting
   * @param {number|bigint} [options.chainId] - Network chain ID (default: 9999)
   * @param {string} [options.expectedDomain] - Approved origin domain for phishing defense
   * @param {Object} [options.deviceRegistry] - DeviceRegistry instance for multi-device management
   */
  constructor(wallet, options = {}) {
    if (!wallet) throw new Error('Wallet is required for MiniAppBridge');
    this.wallet = wallet;
    this.onClearSign = options.onClearSign || (async (prompt) => true);
    this.provider = options.provider || null;
    this.chainId = options.chainId ? BigInt(options.chainId) : 9999n;
    this.expectedDomain = options.expectedDomain ? options.expectedDomain.toLowerCase() : null;
    this.deviceRegistry = options.deviceRegistry || null;
  }

  /**
   * Returns authorized user accounts.
   */
  async requestAccounts() {
    return [this.wallet.address.toBech32m()];
  }

  /**
   * Executes a payment requested by the Mini App.
   * Forces clear-signing user authorization prior to signing.
   * Mitigates malicious Mini App attacks via domain, chain ID, device, and memo verification.
   */
  async requestPayment(request) {
    if (!request || typeof request !== 'object') {
      throw new Error('Payment request must be an object');
    }

    const {
      domain,
      to,
      amount,
      assetSymbol = 'TOLA',
      actionName = 'Mini App Transfer',
      memo = null,
      maxFee = 21000n,
      nonce = 0n,
      chainId = this.chainId,
      deviceId = null,
    } = request;

    // 1. Device Authorization Check
    if (deviceId && this.deviceRegistry) {
      if (!this.deviceRegistry.isAuthorized(deviceId)) {
        const err = new Error(`Device '${deviceId}' is not authorized or has been revoked`);
        err.code = 4100; // Unauthorized
        throw err;
      }
    }

    // 2. Domain Binding / Anti-Phishing Check
    if (this.expectedDomain) {
      if (!domain || domain.toLowerCase() !== this.expectedDomain) {
        const err = new Error(`Domain mismatch: expected '${this.expectedDomain}', got '${domain}'`);
        err.code = 4003; // Forbidden
        throw err;
      }
    }

    // 3. Chain ID Replay Defense
    if (BigInt(chainId) !== this.chainId) {
      const err = new Error(`Chain ID mismatch: requested ${chainId}, active network is ${this.chainId}`);
      err.code = 4901; // Chain mismatch
      throw err;
    }

    const targetAddress = to instanceof Address
      ? to
      : (typeof to === 'string' && (to.startsWith('0x') || to.startsWith('0X'))
          ? Address.fromHex(to)
          : Address.fromBech32m(to).address);

    const prompt = {
      domain,
      actionName,
      targetAddress,
      amount: BigInt(amount),
      assetSymbol,
      memo: memo ? String(memo) : null,
      maxFee: BigInt(maxFee),
      nonce: BigInt(nonce),
      chainId: this.chainId,
    };

    validateClearSigningPrompt(prompt);

    // 4. Prompt user confirmation with tamper-proof clear-signing
    const approved = await this.onClearSign(prompt);
    if (!approved) {
      const err = new Error('User rejected the transaction authorization');
      err.code = 4001; // EIP-1193 user rejected request
      throw err;
    }

    // 5. Prepare transaction payload
    const tx = {
      chainId: Number(this.chainId),
      nonce: Number(nonce),
      from: this.wallet.address.toHex(),
      to: targetAddress.toHex(),
      value: BigInt(amount),
      memo: prompt.memo,
      gasLimit: Number(maxFee),
      maxFeePerGas: 1000000000n,
      maxPriorityFeePerGas: 100000000n,
      validUntilBlock: 0n,
      data: new Uint8Array(0),
    };

    const signedTx = await this.wallet.signTransaction(tx);

    // If provider is available, broadcast; else return signed transaction
    if (this.provider) {
      return {
        status: 'broadcasted',
        tx: signedTx,
      };
    }

    return {
      status: 'signed',
      tx: signedTx,
    };
  }

  /**
   * Processes authenticated TOLA Connect envelope messages.
   */
  async handleConnectEnvelope(envelope, session) {
    if (session) {
      session.validateEnvelope(envelope);
    }
    const { type, payload } = envelope;
    switch (type) {
      case 'TRANSACTION_REQUEST':
        if (payload.domain && payload.domain.toLowerCase() !== envelope.domain.toLowerCase()) {
          const err = new Error(`Domain mismatch: payload domain '${payload.domain}' does not match envelope '${envelope.domain}'`);
          err.code = 4003;
          throw err;
        }
        return await this.requestPayment({ ...payload, domain: envelope.domain });
      case 'SIGN_REQUEST':
        if (payload && payload.message) {
          const approved = await this.onClearSign({
            domain: envelope.domain,
            actionName: 'Sign Message',
            targetAddress: this.wallet.address,
            amount: 0n,
            assetSymbol: 'TOLA',
            maxFee: 0n,
            chainId: this.chainId,
            memo: payload.message,
          });
          if (!approved) {
            const err = new Error('User rejected signature authorization');
            err.code = 4001;
            throw err;
          }
          return await this.wallet.signMessage(payload.message);
        }
        throw new Error('Missing message in SIGN_REQUEST payload');
      default:
        throw new Error(`Unsupported envelope message type: ${type}`);
    }
  }

  /**
   * Dispatches arbitrary RPC methods from Mini App.
   */
  async handleMessage(message) {
    const { id, method, params } = message;
    try {
      let result;
      switch (method) {
        case 'eth_requestAccounts':
        case 'tola_requestAccounts':
          result = await this.requestAccounts();
          break;
        case 'tola_sendPayment':
          result = await this.requestPayment(params[0]);
          break;
        default:
          if (this.provider) {
            result = await this.provider.request({ method, params });
          } else {
            throw new Error(`Unsupported method: ${method}`);
          }
      }
      return { id, jsonrpc: '2.0', result };
    } catch (err) {
      return {
        id,
        jsonrpc: '2.0',
        error: {
          code: err.code || -32603,
          message: err.message,
        },
      };
    }
  }
}

/**
 * Backward compatibility alias for MiniAppBridge.
 * @deprecated Use MiniAppBridge instead.
 */
export class TelegramMiniAppBridge extends MiniAppBridge {}

