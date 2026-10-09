// SPDX-License-Identifier: MIT
pragma solidity 0.8.24;

import "@openzeppelin/contracts/token/ERC1155/ERC1155.sol";
import "@openzeppelin/contracts/token/ERC1155/IERC1155Receiver.sol";

/**
 * @title ERC1155Mock
 * @notice OpenZeppelin v5.0.2 ERC-1155 implementation with batch minting and transfer support.
 */
contract ERC1155Mock is ERC1155 {
    constructor(string memory uri) ERC1155(uri) {}

    function mint(address to, uint256 id, uint256 amount, bytes memory data) external {
        _mint(to, id, amount, data);
    }

    function mintBatch(
        address to,
        uint256[] memory ids,
        uint256[] memory amounts,
        bytes memory data
    ) external {
        _mintBatch(to, ids, amounts, data);
    }
}

/**
 * @title ERC1155ReceiverMock
 * @notice Mock receiver that records single and batch ERC-1155 callback invocations.
 */
contract ERC1155ReceiverMock is IERC1155Receiver {
    bytes4 public constant SINGLE_MAGIC = IERC1155Receiver.onERC1155Received.selector;
    bytes4 public constant BATCH_MAGIC = IERC1155Receiver.onERC1155BatchReceived.selector;

    uint256 public lastId;
    uint256 public lastValue;
    uint256[] public lastBatchIds;
    uint256[] public lastBatchValues;

    function onERC1155Received(
        address,
        address,
        uint256 id,
        uint256 value,
        bytes calldata
    ) external override returns (bytes4) {
        lastId = id;
        lastValue = value;
        return SINGLE_MAGIC;
    }

    function onERC1155BatchReceived(
        address,
        address,
        uint256[] calldata ids,
        uint256[] calldata values,
        bytes calldata
    ) external override returns (bytes4) {
        lastBatchIds = ids;
        lastBatchValues = values;
        return BATCH_MAGIC;
    }

    function supportsInterface(bytes4 interfaceId) external pure override returns (bool) {
        return interfaceId == type(IERC1155Receiver).interfaceId;
    }
}
