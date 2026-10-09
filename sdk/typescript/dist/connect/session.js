/**
 * TOLA Connect Handshake & End-to-End Encrypted Session Protocol.
 * Manages secure communication channels between Applications and User Wallets.
 */

async function getWebCrypto() {
  if (typeof globalThis !== 'undefined' && globalThis.crypto?.subtle) {
    return globalThis.crypto;
  }
  const { webcrypto } = await import('node:crypto');
  return webcrypto;
}

/**
 * The 11 canonical TOLA Connect protocol message types.
 */
export const ConnectMessageType = {
  SESSION_INIT: 'SESSION_INIT',
  SESSION_ACCEPT: 'SESSION_ACCEPT',
  DEVICE_REGISTER: 'DEVICE_REGISTER',
  DEVICE_REVOKE: 'DEVICE_REVOKE',
  SIGN_REQUEST: 'SIGN_REQUEST',
  SIGN_APPROVAL: 'SIGN_APPROVAL',
  SIGN_REJECTION: 'SIGN_REJECTION',
  TRANSACTION_REQUEST: 'TRANSACTION_REQUEST',
  TRANSACTION_APPROVAL: 'TRANSACTION_APPROVAL',
  TRANSACTION_SUBMISSION: 'TRANSACTION_SUBMISSION',
  TRANSACTION_RESULT: 'TRANSACTION_RESULT',
};

/**
 * Multi-device authorization and session manager.
 */
export class DeviceRegistry {
  constructor() {
    this.devices = new Map(); // deviceId -> record
  }

  registerDevice(deviceId, deviceName, publicKey = null, timestamp = Math.floor(Date.now() / 1000)) {
    if (!deviceId || typeof deviceId !== 'string') {
      throw new Error('Device ID must be a non-empty string');
    }
    const existing = this.devices.get(deviceId);
    if (existing) {
      if (existing.isRevoked) {
        throw new Error(`Cannot re-register revoked device '${deviceId}'`);
      }
      throw new Error(`Device '${deviceId}' is already registered`);
    }
    const record = {
      deviceId,
      deviceName: deviceName || deviceId,
      publicKey,
      authorizedAt: timestamp,
      isRevoked: false,
    };
    this.devices.set(deviceId, record);
    return record;
  }

  revokeDevice(deviceId) {
    const dev = this.devices.get(deviceId);
    if (!dev) {
      throw new Error(`Device '${deviceId}' not found`);
    }
    dev.isRevoked = true;
    return dev;
  }

  isAuthorized(deviceId) {
    const dev = this.devices.get(deviceId);
    return !!dev && !dev.isRevoked;
  }

  listDevices() {
    return Array.from(this.devices.values());
  }
}

export class TolaConnectSession {
  constructor(sessionId, domain, appName, sessionKey, createdAt, expiresAt) {
    this.sessionId = sessionId;
    this.domain = domain;
    this.appName = appName;
    this.sessionKey = sessionKey; // Uint8Array(32)
    this.createdAt = createdAt;
    this.expiresAt = expiresAt;
    this.lastSeenNonce = 0n;
  }

  toJSON() {
    return {
      sessionId: this.sessionId,
      domain: this.domain,
      appName: this.appName,
      sessionKey: '[REDACTED]',
      createdAt: this.createdAt,
      expiresAt: this.expiresAt,
    };
  }

  [typeof Symbol !== 'undefined' && Symbol.for ? Symbol.for('nodejs.util.inspect.custom') : 'inspect']() {
    return `TolaConnectSession(sessionId="${this.sessionId}", domain="${this.domain}", key=[REDACTED])`;
  }

  /**
   * Establishes a session using ECDH (P-256) key agreement with HKDF-SHA256 derivation.
   */
  static async fromECDH(localPrivateKey, remotePublicKey, domain, appName, ttlSeconds = 86400) {
    const crypto = await getWebCrypto();
    const sharedBits = await crypto.subtle.deriveBits(
      { name: 'ECDH', public: remotePublicKey },
      localPrivateKey,
      256
    );
    const hkdfKey = await crypto.subtle.importKey(
      'raw',
      sharedBits,
      { name: 'HKDF' },
      false,
      ['deriveKey', 'deriveBits']
    );
    const sessionKeyBits = new Uint8Array(await crypto.subtle.deriveBits(
      {
        name: 'HKDF',
        hash: 'SHA-256',
        salt: new TextEncoder().encode('TOLA_CONNECT_KDF_SALT_V1'),
        info: new TextEncoder().encode(`TOLA_CONNECT_SESSION:${domain}`),
      },
      hkdfKey,
      256
    ));
    const now = Math.floor(Date.now() / 1000);
    const idBytes = new Uint8Array(16);
    crypto.getRandomValues(idBytes);
    const sessionId = Array.from(idBytes).map(b => b.toString(16).padStart(2, '0')).join('');
    return new TolaConnectSession(sessionId, domain, appName, sessionKeyBits, now, now + ttlSeconds);
  }

  static async create(domain, appName, ttlSeconds = 86400) {
    const crypto = await getWebCrypto();
    const keyBytes = new Uint8Array(32);
    crypto.getRandomValues(keyBytes);

    const idBytes = new Uint8Array(16);
    crypto.getRandomValues(idBytes);
    const sessionId = Array.from(idBytes).map(b => b.toString(16).padStart(2, '0')).join('');

    const now = Math.floor(Date.now() / 1000);
    return new TolaConnectSession(
      sessionId,
      domain,
      appName,
      keyBytes,
      now,
      now + ttlSeconds
    );
  }

  isExpired() {
    return Math.floor(Date.now() / 1000) > this.expiresAt;
  }

  /**
   * Constructs an authenticated protocol envelope.
   */
  createEnvelope(type, payload, requestId, nonce, origin = null) {
    if (!Object.values(ConnectMessageType).includes(type)) {
      throw new Error(`Unsupported Connect message type: ${type}`);
    }
    const now = Math.floor(Date.now() / 1000);
    return {
      version: '1.0',
      sessionId: this.sessionId,
      type,
      requestId: String(requestId),
      nonce: Number(nonce),
      timestamp: now,
      expiresAt: Math.min(now + 300, this.expiresAt),
      domain: this.domain,
      origin: origin || `https://${this.domain}`,
      appName: this.appName,
      payload,
    };
  }

  /**
   * Validates an authenticated protocol envelope against replay, domain mismatch, and expiration.
   */
  validateEnvelope(envelope) {
    if (!envelope || typeof envelope !== 'object') {
      throw new Error('Envelope must be a non-null object');
    }
    if (envelope.version !== '1.0') {
      throw new Error(`Incompatible protocol version: ${envelope.version}`);
    }
    if (envelope.sessionId !== this.sessionId) {
      throw new Error(`Session ID mismatch: expected ${this.sessionId}`);
    }
    if (!Object.values(ConnectMessageType).includes(envelope.type)) {
      throw new Error(`Unknown message type: ${envelope.type}`);
    }
    if (!envelope.requestId || typeof envelope.requestId !== 'string' || envelope.requestId.trim().length === 0) {
      throw new Error('Missing or empty requestId in envelope');
    }
    if (this.isExpired()) {
      throw new Error('TOLA Connect session has expired');
    }
    const now = Math.floor(Date.now() / 1000);
    if (envelope.timestamp && envelope.timestamp > now + 60) {
      throw new Error('Message timestamp is in the future (clock skew exceeds 60s)');
    }
    if (envelope.expiresAt && now > envelope.expiresAt) {
      throw new Error('Message envelope has expired');
    }

    // Monotonic nonce check for replay immunity
    const reqNonce = BigInt(envelope.nonce);
    if (reqNonce <= this.lastSeenNonce) {
      throw new Error(`Replay attack detected: non-monotonic nonce ${reqNonce} <= ${this.lastSeenNonce}`);
    }

    // Domain & origin binding
    if (envelope.domain && envelope.domain.toLowerCase() !== this.domain.toLowerCase()) {
      throw new Error(`Domain mismatch: expected ${this.domain}, got ${envelope.domain}`);
    }

    this.lastSeenNonce = reqNonce;
    return true;
  }

  /**
   * Encrypt arbitrary string/JSON payload with AES-GCM 256.
   * Returns base64 payload: [12-byte IV || Ciphertext || 16-byte Auth Tag].
   */
  async encrypt(data, additionalData = null) {
    if (this.isExpired()) {
      throw new Error('TOLA Connect session has expired');
    }
    const crypto = await getWebCrypto();
    const iv = new Uint8Array(12);
    crypto.getRandomValues(iv);

    const encoder = new TextEncoder();
    const encodedData = typeof data === 'string' ? encoder.encode(data) : encoder.encode(JSON.stringify(data));

    const key = await crypto.subtle.importKey(
      'raw',
      this.sessionKey,
      { name: 'AES-GCM' },
      false,
      ['encrypt']
    );

    const encryptParams = { name: 'AES-GCM', iv };
    if (additionalData) {
      encryptParams.additionalData = typeof additionalData === 'string'
        ? encoder.encode(additionalData)
        : additionalData;
    }

    const ciphertext = new Uint8Array(await crypto.subtle.encrypt(
      encryptParams,
      key,
      encodedData
    ));

    const combined = new Uint8Array(iv.length + ciphertext.length);
    combined.set(iv, 0);
    combined.set(ciphertext, iv.length);

    if (typeof Buffer !== 'undefined') {
      return Buffer.from(combined).toString('base64');
    }
    let binary = '';
    for (let i = 0; i < combined.length; i++) binary += String.fromCharCode(combined[i]);
    return btoa(binary);
  }

  /**
   * Decrypt AES-GCM base64 payload.
   */
  async decrypt(base64Payload, additionalData = null) {
    if (this.isExpired()) {
      throw new Error('TOLA Connect session has expired');
    }
    const crypto = await getWebCrypto();

    let combined;
    if (typeof Buffer !== 'undefined') {
      combined = new Uint8Array(Buffer.from(base64Payload, 'base64'));
    } else {
      const binary = atob(base64Payload);
      combined = new Uint8Array(binary.length);
      for (let i = 0; i < binary.length; i++) combined[i] = binary.charCodeAt(i);
    }

    if (combined.length < 28) {
      throw new Error('Invalid encrypted payload: too short');
    }

    const iv = combined.slice(0, 12);
    const ciphertext = combined.slice(12);

    const key = await crypto.subtle.importKey(
      'raw',
      this.sessionKey,
      { name: 'AES-GCM' },
      false,
      ['decrypt']
    );

    const decryptParams = { name: 'AES-GCM', iv };
    if (additionalData) {
      decryptParams.additionalData = typeof additionalData === 'string'
        ? new TextEncoder().encode(additionalData)
        : additionalData;
    }

    const decrypted = await crypto.subtle.decrypt(
      decryptParams,
      key,
      ciphertext
    );

    const decoder = new TextDecoder();
    return decoder.decode(decrypted);
  }
}
