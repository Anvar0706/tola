import { Address } from './address.js';
import { UnsignedTransaction, Transaction, Secp256k1Signature } from './types.js';

export declare class TolaWallet {
  readonly publicKeyPoint: { x: bigint; y: bigint };
  readonly address: Address;

  constructor(privateKey: bigint, options?: { mnemonic?: string | null; derivationPath?: string | null });

  static generate(wordCount?: 12 | 24, passphrase?: string): TolaWallet;
  static fromMnemonic(mnemonic: string, passphrase?: string): TolaWallet;
  static fromMnemonicWithPath(mnemonic: string, passphrase?: string, path?: string): TolaWallet;
  static fromMnemonicLegacyTestnet(mnemonic: string, passphrase?: string): TolaWallet;
  static fromPrivateKey(privateKey: bigint | Uint8Array | string): TolaWallet;
  static fromPrivateKeyBytes(bytes: Uint8Array): TolaWallet;
  static fromPrivateKeyHex(hexStr: string): TolaWallet;

  get mnemonic(): string | null;
  get derivationPath(): string | null;
  get privateKeyHex(): string;
  exportPrivateKeyHex(): string;
  get rawAddress(): string;
  get publicKeyBytes(): Uint8Array;
  getPublicKey(compressed?: boolean): Uint8Array;
  getAddress(network?: 'mainnet' | 'testnet' | 'devnet' | string): string;
  addressBech32m(network?: 'mainnet' | 'testnet' | 'devnet' | string): string;

  signDigest(digest32: Uint8Array): Promise<Secp256k1Signature>;
  signTransaction(tx: UnsignedTransaction): Promise<Transaction>;
  signMessage(message: string | Uint8Array): Promise<string>;

  toJSON(): { address: string; rawAddress: string };
}
