export declare function generateMnemonic(wordCount?: 12 | 24): string;
export declare function validateMnemonic(mnemonic: string): string;
export declare function mnemonicToSeed(mnemonic: string, passphrase?: string): Uint8Array;
