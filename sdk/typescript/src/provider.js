/**
 * JSON-RPC 2.0 Provider and EIP-1193 Compatible Web3 Provider for TOLA Network.
 * Supports standard Ethereum methods (eth_*, net_*, web3_*) and native protocol extensions (tola_*).
 */

export class JsonRpcProvider {
  constructor(url = 'http://127.0.0.1:8545', timeoutMs = 10000) {
    this.url = url.endsWith('/') ? url.slice(0, -1) : url;
    this.timeoutMs = timeoutMs;
    this._requestId = 1;
  }

  async send(method, params = []) {
    const id = this._requestId++;
    const payload = {
      jsonrpc: '2.0',
      id,
      method,
      params,
    };

    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), this.timeoutMs);

    try {
      const response = await fetch(this.url, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
        },
        body: JSON.stringify(payload),
        signal: controller.signal,
      });

      if (!response.ok) {
        throw new Error(`HTTP Error ${response.status}: ${response.statusText}`);
      }

      const json = await response.json();
      if (json.error) {
        const err = new Error(json.error.message || 'JSON-RPC Error');
        err.code = json.error.code || -32603;
        err.data = json.error.data;
        throw err;
      }

      return json.result;
    } finally {
      clearTimeout(timer);
    }
  }

  async getChainId() {
    const hex = await this.send('eth_chainId');
    return BigInt(hex);
  }

  async getBlockNumber() {
    const hex = await this.send('eth_blockNumber');
    return BigInt(hex);
  }

  async getBalance(address, blockTag = 'latest') {
    const hex = await this.send('eth_getBalance', [address, blockTag]);
    return BigInt(hex);
  }

  async getTransactionCount(address, blockTag = 'latest') {
    const hex = await this.send('eth_getTransactionCount', [address, blockTag]);
    return BigInt(hex);
  }

  async sendRawTransaction(signedHex) {
    const formatted = signedHex.startsWith('0x') ? signedHex : '0x' + signedHex;
    return await this.send('eth_sendRawTransaction', [formatted]);
  }

  async getBlockByNumber(blockNum, fullTxs = false) {
    const tag = typeof blockNum === 'bigint' || typeof blockNum === 'number'
      ? '0x' + BigInt(blockNum).toString(16)
      : blockNum;
    return await this.send('eth_getBlockByNumber', [tag, fullTxs]);
  }

  async getBlockByHash(blockHash, fullTxs = false) {
    return await this.send('eth_getBlockByHash', [blockHash, fullTxs]);
  }

  async getTransactionByHash(txHash) {
    return await this.send('eth_getTransactionByHash', [txHash]);
  }

  async getTransactionReceipt(txHash) {
    return await this.send('eth_getTransactionReceipt', [txHash]);
  }

  async waitForTransactionReceipt(txHash, timeoutMs = 30000, pollIntervalMs = 1000) {
    const startTime = Date.now();
    while (Date.now() - startTime < timeoutMs) {
      const receipt = await this.getTransactionReceipt(txHash);
      if (receipt) {
        return receipt;
      }
      await new Promise((resolve) => setTimeout(resolve, pollIntervalMs));
    }
    throw new Error(`Timeout waiting for transaction receipt: ${txHash}`);
  }

  async getHealth() {
    return await this.send('tola_getHealth');
  }

  async getValidators() {
    return await this.send('tola_getValidators');
  }
}

/**
 * EIP-1193 Compliant Ethereum/TOLA Provider Interface for Web dApps and Telegram Mini Apps.
 */
export class TolaProvider {
  constructor(rpcUrl = 'http://127.0.0.1:8545') {
    this._provider = new JsonRpcProvider(rpcUrl);
    this._listeners = new Map();
  }

  async request(args) {
    if (!args || typeof args.method !== 'string') {
      throw new Error('Invalid request arguments: method must be a string');
    }
    const params = Array.isArray(args.params) ? args.params : [];
    return await this._provider.send(args.method, params);
  }

  on(event, listener) {
    if (!this._listeners.has(event)) {
      this._listeners.set(event, new Set());
    }
    this._listeners.get(event).add(listener);
    return this;
  }

  removeListener(event, listener) {
    this._listeners.get(event)?.delete(listener);
    return this;
  }

  emit(event, ...args) {
    const set = this._listeners.get(event);
    if (!set || set.size === 0) return false;
    for (const fn of set) {
      fn(...args);
    }
    return true;
  }
}
