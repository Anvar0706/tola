// SPDX-License-Identifier: MIT
pragma solidity 0.8.24;

import "@openzeppelin/contracts/token/ERC721/extensions/ERC721URIStorage.sol";
import "@openzeppelin/contracts/token/ERC721/IERC721Receiver.sol";

/**
 * @title ERC721Mock
 * @notice OpenZeppelin v5.0.2 ERC-721 implementation with URI storage and receiver callback testing.
 */
contract ERC721Mock is ERC721URIStorage {
    constructor(string memory name, string memory symbol) ERC721(name, symbol) {}

    function mint(address to, uint256 tokenId) external {
        _mint(to, tokenId);
    }

    function safeMint(address to, uint256 tokenId, string memory uri) external {
        _safeMint(to, tokenId);
        _setTokenURI(tokenId, uri);
    }

    function setTokenURI(uint256 tokenId, string memory uri) external {
        _setTokenURI(tokenId, uri);
    }
}

/**
 * @title ERC721ReceiverMock
 * @notice Mock receiver that records onERC721Received invocations.
 */
contract ERC721ReceiverMock is IERC721Receiver {
    bytes4 public constant RETURN_MAGIC = IERC721Receiver.onERC721Received.selector;
    address public lastOperator;
    address public lastFrom;
    uint256 public lastTokenId;
    bytes public lastData;

    function onERC721Received(
        address operator,
        address from,
        uint256 tokenId,
        bytes calldata data
    ) external override returns (bytes4) {
        lastOperator = operator;
        lastFrom = from;
        lastTokenId = tokenId;
        lastData = data;
        return RETURN_MAGIC;
    }
}
