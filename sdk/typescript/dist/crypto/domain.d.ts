export declare const DOMAIN_TRANSACTION_PAYLOAD: Uint8Array;
export declare const DOMAIN_CONSENSUS_PREVOTE: Uint8Array;
export declare const DOMAIN_CONSENSUS_PRECOMMIT: Uint8Array;

export declare function domainSeparatedHash(
  domainTag: Uint8Array,
  chainId: bigint,
  epoch: bigint,
  payloadHash32: Uint8Array
): Uint8Array;
