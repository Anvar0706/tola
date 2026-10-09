export interface ExtendedKey {
  key: Uint8Array;
  chainCode: Uint8Array;
}

export declare const CANONICAL_DERIVATION_PATH = "m/44'/9999'/0'/0/0";
export declare const LEGACY_TESTNET_PROVISIONAL_PATH = "m/44'/102'/0'/0/0";

export declare function deriveMasterNode(seed: Uint8Array): ExtendedKey;
export declare function deriveChildNode(parent: ExtendedKey, index: number): ExtendedKey;
export declare function derivePath(seed: Uint8Array, path: string): ExtendedKey;
