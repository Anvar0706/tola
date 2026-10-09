/**
 * Core type definitions and network constants for TOLA Network.
 */

export const NETWORKS = {
  mainnet: { name: 'Mainnet', hrp: 'tola', chainId: 9999n },
  testnet: { name: 'Testnet', hrp: 'ttola', chainId: 102n },
  devnet: { name: 'Devnet', hrp: 'dtola', chainId: 1337n },
};

export const MAX_MEMO_BYTES = 256;

