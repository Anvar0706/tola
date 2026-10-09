import { TolaWallet } from '../wallet.js';
import { ClearSigningPrompt } from './clear_signing.js';
import { DeviceRegistry } from './session.js';

export interface AppWalletBridgeOptions {
  onClearSign?: (prompt: ClearSigningPrompt) => Promise<boolean>;
  provider?: any;
  chainId?: number | bigint;
  expectedDomain?: string;
  deviceRegistry?: DeviceRegistry;
}

export interface PaymentRequest {
  domain: string;
  to: string;
  amount: bigint | string | number;
  assetSymbol?: string;
  actionName?: string;
  memo?: string | null;
  maxFee?: bigint | string | number;
  nonce?: bigint | string | number;
  chainId?: number | bigint;
  deviceId?: string;
}

export declare class AppWalletBridge {
  wallet: TolaWallet;
  onClearSign: (prompt: ClearSigningPrompt) => Promise<boolean>;
  provider: any;
  chainId: bigint;
  expectedDomain: string | null;
  deviceRegistry: DeviceRegistry | null;

  constructor(wallet: TolaWallet, options?: AppWalletBridgeOptions);
  requestAccounts(): Promise<string[]>;
  requestPayment(request: PaymentRequest): Promise<{ status: string; tx: any }>;
  handleConnectEnvelope(envelope: any, session?: any): Promise<any>;
  handleMessage(message: { id: number | string; method: string; params?: any[] }): Promise<any>;
}
