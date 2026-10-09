export declare const ConnectMessageType: {
  readonly SESSION_INIT: 'SESSION_INIT';
  readonly SESSION_ACCEPT: 'SESSION_ACCEPT';
  readonly DEVICE_REGISTER: 'DEVICE_REGISTER';
  readonly DEVICE_REVOKE: 'DEVICE_REVOKE';
  readonly SIGN_REQUEST: 'SIGN_REQUEST';
  readonly SIGN_APPROVAL: 'SIGN_APPROVAL';
  readonly SIGN_REJECTION: 'SIGN_REJECTION';
  readonly TRANSACTION_REQUEST: 'TRANSACTION_REQUEST';
  readonly TRANSACTION_APPROVAL: 'TRANSACTION_APPROVAL';
  readonly TRANSACTION_SUBMISSION: 'TRANSACTION_SUBMISSION';
  readonly TRANSACTION_RESULT: 'TRANSACTION_RESULT';
};

export interface DeviceRecord {
  deviceId: string;
  deviceName: string;
  publicKey: string | null;
  authorizedAt: number;
  isRevoked: boolean;
}

export declare class DeviceRegistry {
  devices: Map<string, DeviceRecord>;
  registerDevice(deviceId: string, deviceName?: string, publicKey?: string | null, timestamp?: number): DeviceRecord;
  revokeDevice(deviceId: string): DeviceRecord;
  isAuthorized(deviceId: string): boolean;
  listDevices(): DeviceRecord[];
}

export interface ConnectEnvelope {
  version: string;
  sessionId: string;
  type: string;
  requestId: string;
  nonce: number;
  timestamp: number;
  expiresAt: number;
  domain: string;
  origin: string;
  appName: string;
  payload: any;
}

export declare class TolaConnectSession {
  sessionId: string;
  domain: string;
  appName: string;
  sessionKey: Uint8Array;
  createdAt: number;
  expiresAt: number;
  lastSeenNonce: bigint;

  constructor(
    sessionId: string,
    domain: string,
    appName: string,
    sessionKey: Uint8Array,
    createdAt: number,
    expiresAt: number
  );

  static create(domain: string, appName: string, ttlSeconds?: number): Promise<TolaConnectSession>;
  isExpired(): boolean;
  createEnvelope(type: string, payload: any, requestId: string | number, nonce: number | bigint, origin?: string): ConnectEnvelope;
  validateEnvelope(envelope: ConnectEnvelope): boolean;
  encrypt(data: string | object): Promise<string>;
  decrypt(base64Payload: string): Promise<string>;
}
