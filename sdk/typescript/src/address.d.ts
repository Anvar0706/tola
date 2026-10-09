export declare class Address {
  readonly bytes: Uint8Array;
  constructor(bytes: Uint8Array);
  static ZERO(): Address;
  static fromBytes(bytes: Uint8Array): Address;
  static fromHex(hexStr: string): Address;
  static fromBech32m(s: string): { address: Address; hrp: string };
  toHex(): string;
  toBech32m(networkOrHrp?: string): string;
  equals(other: Address): boolean;
  toString(): string;
}
