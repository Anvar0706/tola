import { Address } from '../address.js';

export class ExternalAssetError extends Error {}

export const ExternalAssetType: {
  readonly IDENTITY_IDENTIFIER: 'IDENTITY_IDENTIFIER';
  readonly VIRTUAL_NUMBER: 'VIRTUAL_NUMBER';
  readonly GIFT_ASSET: 'GIFT_ASSET';
  readonly ATTESTATION_BADGE: 'ATTESTATION_BADGE';
  readonly SOCIAL_IDENTIFIER: 'SOCIAL_IDENTIFIER';
  readonly ACCESS_CREDENTIAL: 'ACCESS_CREDENTIAL';
  readonly DIGITAL_COLLECTIBLE: 'DIGITAL_COLLECTIBLE';
  readonly OTHER_EXTERNAL_ASSET: 'OTHER_EXTERNAL_ASSET';
  readonly REAL_WORLD_ASSET: 'REAL_WORLD_ASSET';
  readonly GAMING_ASSET: 'GAMING_ASSET';
  readonly MESSENGER_USERNAME: 'MESSENGER_USERNAME';
  readonly MESSENGER_NUMBER: 'MESSENGER_NUMBER';
  readonly MESSENGER_GIFT: 'MESSENGER_GIFT';
  readonly MESSENGER_BADGE: 'MESSENGER_BADGE';
  readonly SOCIAL_USERNAME: 'SOCIAL_USERNAME';
  readonly SOCIAL_NUMBER: 'SOCIAL_NUMBER';
};

export const EXTERNAL_ASSET_TYPE_IDS: {
  readonly IDENTITY_IDENTIFIER: 1;
  readonly VIRTUAL_NUMBER: 2;
  readonly GIFT_ASSET: 3;
  readonly ATTESTATION_BADGE: 4;
  readonly SOCIAL_IDENTIFIER: 5;
  readonly ACCESS_CREDENTIAL: 6;
  readonly DIGITAL_COLLECTIBLE: 7;
  readonly OTHER_EXTERNAL_ASSET: 8;
  readonly REAL_WORLD_ASSET: 9;
  readonly GAMING_ASSET: 10;
  readonly MESSENGER_USERNAME: 1;
  readonly MESSENGER_NUMBER: 2;
  readonly MESSENGER_GIFT: 3;
  readonly MESSENGER_BADGE: 4;
  readonly SOCIAL_USERNAME: 5;
  readonly SOCIAL_NUMBER: 6;
};

export function getAssetTypeId(assetType: string | number): number;

export function computeExternalAssetCanonicalId(
  sourcePlatform: string,
  assetType: string | number,
  externalAssetId: string
): Uint8Array;

export function computeExternalAssetAttestationDigest(
  chainId: number | bigint,
  nonce: number | bigint,
  destinationWallet: Address | string,
  expiresAt: number | bigint,
  sourcePlatform: string,
  assetType: string | number,
  externalAssetId: string
): Uint8Array;

export interface ExternalAssetAttestation {
  chainId: number;
  nonce: number;
  destinationWallet: string;
  expiresAt: number;
  sourcePlatform: string;
  assetType: string;
  externalAssetId: string;
  verifierAddress: string;
  signature: {
    r: string;
    s: string;
    v: number;
  };
}

export function signExternalAssetAttestation(
  verifierWallet: any,
  chainId: number | bigint,
  nonce: number | bigint,
  destinationWallet: Address | string,
  expiresAt: number | bigint,
  sourcePlatform: string,
  assetType: string | number,
  externalAssetId: string
): Promise<ExternalAssetAttestation>;

export function verifyExternalAssetAttestation(
  attestation: ExternalAssetAttestation,
  expectedVerifier: Address | string,
  currentTime?: number
): boolean;

export function validateExternalAssetMetadata(metadata: Record<string, any>): boolean;

export const EXTERNAL_ASSET_CONTRACT_ADDRESS: string;

export function encodeMintExternalAssetCalldata(
  attestation: ExternalAssetAttestation,
  metadataUri?: string
): Uint8Array;

export function encodeTransferExternalAssetCalldata(
  from: Address | string,
  to: Address | string,
  tokenId: bigint | number | string
): Uint8Array;

export function encodeBurnExternalAssetCalldata(
  tokenId: bigint | number | string
): Uint8Array;
