export interface Point {
  x: bigint;
  y: bigint;
}

export declare const P: bigint;
export declare const N: bigint;
export declare const HALF_N: bigint;
export declare const G: Point;

export declare function pointAdd(p1: Point | null, p2: Point | null): Point | null;
export declare function pointMultiply(k: bigint, p?: Point): Point;
export declare function compressPublicKey(point: Point): Uint8Array;
export declare function uncompressedPublicKey(point: Point): Uint8Array;
export declare function publicKeyToAddress(point: Point): Uint8Array;
export declare function bytesToBigint(bytes: Uint8Array): bigint;
export declare function bigintToBytes(value: bigint, length: number): Uint8Array;
export declare function signDigest(
  privateKey: bigint,
  digest32: Uint8Array
): Promise<{ r: Uint8Array; s: Uint8Array; v: number }>;
