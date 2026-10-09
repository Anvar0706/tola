// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.20;

/**
 * @title IERC721Receiver
 * @notice Standard ERC-721 receiver interface to safely receive token transfers.
 */
interface IERC721Receiver {
    function onERC721Received(
        address operator,
        address from,
        uint256 tokenId,
        bytes calldata data
    ) external returns (bytes4);
}

/**
 * @title ITolaExternalAssetToken
 * @notice Reference interface for TOLA Network Tokenized External Assets (TLS-721 / ERC-721).
 * Represents on-chain tokenized representations of external platform assets (e.g., usernames,
 * gifts, game items, digital collectibles, social handles) minted via platform-authorized proofs.
 *
 * CANONICAL ARCHITECTURAL ROLE:
 * The native TOLA execution engine (`ExternalAssetContractExecutor` at canonical address
 * `0x0000000000000000000000000000000000007721`) is the authoritative consensus implementation on TOLA.
 * This Solidity contract serves as the official reference specification and external integration interface.
 *
 * CRITICAL ARCHITECTURAL INVARIANT:
 * TOLA NFT ownership represents blockchain ownership of the representation on TOLA.
 * It does NOT automatically grant or synchronize legal or technical control over the original external platform asset.
 * TOLA does NOT maintain or mandate a centralized verifier registry; any external platform
 * defines its own authorization mechanism.
 */
interface ITolaExternalAssetToken {
    enum ExternalAssetType {
        MESSENGER_USERNAME,  // 0 -> 1 in protocol
        MESSENGER_NUMBER,    // 1 -> 2 in protocol
        MESSENGER_GIFT,      // 2 -> 3 in protocol
        MESSENGER_BADGE,     // 3 -> 4 in protocol
        SOCIAL_USERNAME,     // 4 -> 5 in protocol
        SOCIAL_NUMBER,       // 5 -> 6 in protocol
        DIGITAL_COLLECTIBLE, // 6 -> 7 in protocol
        OTHER_EXTERNAL_ASSET // 7 -> 8 in protocol
    }

    struct ExternalAssetRecord {
        uint256 tokenId;
        bytes32 canonicalId;
        string sourcePlatform;
        uint8 assetType;
        string externalAssetId;
        string metadataUri;
        address authorizer;
        uint64 mintedAt;
    }

    event ExternalAssetMinted(
        uint256 indexed tokenId,
        bytes32 indexed canonicalId,
        address indexed owner,
        string sourcePlatform,
        uint8 assetType,
        string externalAssetId,
        address authorizer
    );

    event Transfer(address indexed from, address indexed to, uint256 indexed tokenId);
    event Approval(address indexed owner, address indexed approved, uint256 indexed tokenId);
    event ApprovalForAll(address indexed owner, address indexed operator, bool approved);

    function mintWithAttestation(
        uint256 nonce,
        uint64 expiresAt,
        string calldata sourcePlatform,
        uint8 assetType,
        string calldata externalAssetId,
        string calldata metadataUri,
        bytes calldata signature
    ) external returns (uint256 tokenId);

    function getExternalAssetRecord(uint256 tokenId) external view returns (ExternalAssetRecord memory);
    function getCanonicalIdToToken(bytes32 canonicalId) external view returns (uint256);
    function ownerOf(uint256 tokenId) external view returns (address);
    function balanceOf(address owner) external view returns (uint256);
    function transferFrom(address from, address to, uint256 tokenId) external;
    function safeTransferFrom(address from, address to, uint256 tokenId) external;
    function safeTransferFrom(address from, address to, uint256 tokenId, bytes calldata data) external;
    function tokenURI(uint256 tokenId) external view returns (string memory);
    function burn(uint256 tokenId) external;
}

/**
 * @title TolaExternalAssetToken
 * @notice Production reference contract for Tokenized External Assets on TOLA Network.
 * Implements duplicate mint protection, platform-neutral authorization proofs,
 * replay defense, burn mechanics, and full ERC-721 interoperability.
 */
contract TolaExternalAssetToken is ITolaExternalAssetToken {
    string public constant name = "TOLA Tokenized External Asset";
    string public constant symbol = "TEXT";

    bytes4 private constant _ERC721_RECEIVED = 0x150b7a02;

    address public owner;
    uint256 private _nextTokenId = 1;

    // Token ID => ExternalAssetRecord
    mapping(uint256 => ExternalAssetRecord) private _records;

    // Token ID => On-chain owner address
    mapping(uint256 => address) private _owners;

    // Owner address => token count
    mapping(address => uint256) private _balances;

    // Canonical ID => Token ID (enforces duplicate mint protection)
    mapping(bytes32 => uint256) public canonicalIdToToken;

    // Authorization Digest => Used status (enforces replay protection)
    mapping(bytes32 => bool) public usedAttestations;

    // Token ID => Approved operator
    mapping(uint256 => address) private _tokenApprovals;

    // Owner => Operator => Approved status
    mapping(address => mapping(address => bool)) private _operatorApprovals;

    error UnauthorizedCaller();
    error ZeroAddress();
    error TokenDoesNotExist(uint256 tokenId);
    error AttestationExpired(uint64 expiresAt, uint256 currentTime);
    error AttestationAlreadyUsed(bytes32 digest);
    error InvalidVerifierSignature(address recoveredSigner);
    error AssetAlreadyMinted(bytes32 canonicalId, uint256 existingTokenId);
    error NotOwnerOrApproved();
    error InvalidAssetType(uint8 assetType);
    error EmptyString();
    error UnsafeRecipient();

    modifier onlyOwner() {
        if (msg.sender != owner) revert UnauthorizedCaller();
        _;
    }

    constructor() {
        owner = msg.sender;
    }

    /**
     * @notice Computes canonical 32-byte identity for duplicate mint prevention.
     */
    function computeCanonicalId(
        string memory sourcePlatform,
        uint8 assetType,
        string memory externalAssetId
    ) public pure returns (bytes32) {
        bytes memory prefix = bytes("TOLA_EXTERNAL_ASSET_CANONICAL_V1:");
        bytes memory srcBytes = bytes(sourcePlatform);
        bytes memory idBytes = bytes(externalAssetId);

        return keccak256(
            abi.encodePacked(
                prefix,
                uint32(srcBytes.length),
                srcBytes,
                assetType,
                uint32(idBytes.length),
                idBytes
            )
        );
    }

    /**
     * @notice Computes canonical 32-byte digest for external asset mint authorization.
     */
    function computeAttestationDigest(
        uint256 chainId,
        uint256 nonce,
        address destinationWallet,
        uint64 expiresAt,
        string memory sourcePlatform,
        uint8 assetType,
        string memory externalAssetId
    ) public pure returns (bytes32) {
        bytes memory prefix = bytes("TOLA_EXTERNAL_ASSET_ATTESTATION_V1:");
        bytes memory srcBytes = bytes(sourcePlatform);
        bytes memory idBytes = bytes(externalAssetId);

        return keccak256(
            abi.encodePacked(
                prefix,
                uint64(chainId),
                uint64(nonce),
                destinationWallet,
                expiresAt,
                uint32(srcBytes.length),
                srcBytes,
                assetType,
                uint32(idBytes.length),
                idBytes
            )
        );
    }

    /**
     * @notice Mints a TOLA collectible representing an externally authorized platform asset.
     */
    function mintWithAttestation(
        uint256 nonce,
        uint64 expiresAt,
        string calldata sourcePlatform,
        uint8 assetType,
        string calldata externalAssetId,
        string calldata metadataUri,
        bytes calldata signature
    ) external override returns (uint256) {
        if (bytes(sourcePlatform).length == 0 || bytes(externalAssetId).length == 0) {
            revert EmptyString();
        }
        if (assetType == 0 || assetType > 8) {
            revert InvalidAssetType(assetType);
        }
        if (block.timestamp > expiresAt) {
            revert AttestationExpired(expiresAt, block.timestamp);
        }

        bytes32 digest = computeAttestationDigest(
            block.chainid,
            nonce,
            msg.sender,
            expiresAt,
            sourcePlatform,
            assetType,
            externalAssetId
        );

        if (usedAttestations[digest]) {
            revert AttestationAlreadyUsed(digest);
        }

        bytes32 canonicalId = computeCanonicalId(sourcePlatform, assetType, externalAssetId);
        if (canonicalIdToToken[canonicalId] != 0) {
            revert AssetAlreadyMinted(canonicalId, canonicalIdToToken[canonicalId]);
        }

        // Recover external platform authorizer from 65-byte Secp256k1 signature
        address authorizer = _recoverSigner(digest, signature);
        if (authorizer == address(0)) {
            revert InvalidVerifierSignature(authorizer);
        }

        usedAttestations[digest] = true;

        uint256 tokenId = _nextTokenId++;
        _owners[tokenId] = msg.sender;
        _balances[msg.sender] += 1;
        canonicalIdToToken[canonicalId] = tokenId;

        _records[tokenId] = ExternalAssetRecord({
            tokenId: tokenId,
            canonicalId: canonicalId,
            sourcePlatform: sourcePlatform,
            assetType: assetType,
            externalAssetId: externalAssetId,
            metadataUri: metadataUri,
            authorizer: authorizer,
            mintedAt: uint64(block.timestamp)
        });

        emit ExternalAssetMinted(
            tokenId,
            canonicalId,
            msg.sender,
            sourcePlatform,
            assetType,
            externalAssetId,
            authorizer
        );
        emit Transfer(address(0), msg.sender, tokenId);

        return tokenId;
    }

    function getExternalAssetRecord(uint256 tokenId) external view override returns (ExternalAssetRecord memory) {
        if (_owners[tokenId] == address(0)) revert TokenDoesNotExist(tokenId);
        return _records[tokenId];
    }

    function getCanonicalIdToToken(bytes32 canonicalId) external view override returns (uint256) {
        return canonicalIdToToken[canonicalId];
    }

    function ownerOf(uint256 tokenId) public view override returns (address) {
        address o = _owners[tokenId];
        if (o == address(0)) revert TokenDoesNotExist(tokenId);
        return o;
    }

    function balanceOf(address ownerAddr) public view override returns (uint256) {
        if (ownerAddr == address(0)) revert ZeroAddress();
        return _balances[ownerAddr];
    }

    function tokenURI(uint256 tokenId) public view override returns (string memory) {
        if (_owners[tokenId] == address(0)) revert TokenDoesNotExist(tokenId);
        return _records[tokenId].metadataUri;
    }

    function transferFrom(address from, address to, uint256 tokenId) public override {
        address currentOwner = ownerOf(tokenId);
        if (currentOwner != from) revert NotOwnerOrApproved();
        if (to == address(0)) revert ZeroAddress();

        if (msg.sender != currentOwner &&
            _tokenApprovals[tokenId] != msg.sender &&
            !_operatorApprovals[currentOwner][msg.sender]) {
            revert NotOwnerOrApproved();
        }

        _tokenApprovals[tokenId] = address(0);
        _owners[tokenId] = to;
        _balances[from] -= 1;
        _balances[to] += 1;

        emit Transfer(from, to, tokenId);
    }

    function safeTransferFrom(address from, address to, uint256 tokenId) public override {
        safeTransferFrom(from, to, tokenId, "");
    }

    function safeTransferFrom(address from, address to, uint256 tokenId, bytes calldata data) public override {
        transferFrom(from, to, tokenId);
        if (_isContract(to)) {
            try IERC721Receiver(to).onERC721Received(msg.sender, from, tokenId, data) returns (bytes4 retval) {
                if (retval != _ERC721_RECEIVED) revert UnsafeRecipient();
            } catch {
                revert UnsafeRecipient();
            }
        }
    }

    function approve(address to, uint256 tokenId) external {
        address o = ownerOf(tokenId);
        if (msg.sender != o && !_operatorApprovals[o][msg.sender]) {
            revert NotOwnerOrApproved();
        }
        _tokenApprovals[tokenId] = to;
        emit Approval(o, to, tokenId);
    }

    function setApprovalForAll(address operator, bool approved) external {
        if (operator == address(0)) revert ZeroAddress();
        _operatorApprovals[msg.sender][operator] = approved;
        emit ApprovalForAll(msg.sender, operator, approved);
    }

    function getApproved(uint256 tokenId) external view returns (address) {
        ownerOf(tokenId);
        return _tokenApprovals[tokenId];
    }

    function isApprovedForAll(address o, address operator) external view returns (bool) {
        return _operatorApprovals[o][operator];
    }

    /**
     * @notice Burn a tokenized external asset representation on TOLA.
     */
    function burn(uint256 tokenId) external override {
        address currentOwner = ownerOf(tokenId);
        if (msg.sender != currentOwner &&
            _tokenApprovals[tokenId] != msg.sender &&
            !_operatorApprovals[currentOwner][msg.sender]) {
            revert NotOwnerOrApproved();
        }

        _tokenApprovals[tokenId] = address(0);
        _owners[tokenId] = address(0);
        _balances[currentOwner] -= 1;

        emit Transfer(currentOwner, address(0), tokenId);
    }

    /**
     * @notice ERC-165 interface detection.
     */
    function supportsInterface(bytes4 interfaceId) external pure returns (bool) {
        return
            interfaceId == 0x01ffc9a7 || // ERC-165
            interfaceId == 0x80ac58cd || // ERC-721
            interfaceId == 0x5b5e139f;   // ERC-721 Metadata
    }

    function _isContract(address account) internal view returns (bool) {
        return account.code.length > 0;
    }

    function _recoverSigner(bytes32 hash, bytes calldata signature) internal pure returns (address) {
        if (signature.length != 65) return address(0);

        bytes32 r;
        bytes32 s;
        uint8 v;

        assembly {
            r := calldataload(signature.offset)
            s := calldataload(add(signature.offset, 32))
            v := byte(0, calldataload(add(signature.offset, 64)))
        }

        if (v < 27) {
            v += 27;
        }

        if (v != 27 && v != 28) {
            return address(0);
        }

        return ecrecover(hash, v, r, s);
    }
}
