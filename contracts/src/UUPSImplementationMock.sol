// SPDX-License-Identifier: MIT
pragma solidity 0.8.24;

import "@openzeppelin/contracts/proxy/utils/UUPSUpgradeable.sol";

/**
 * @title UUPSImplementationMock
 * @notice Logic contract implementing OpenZeppelin UUPSUpgradeable for canonical EIP-1967 testing.
 */
contract UUPSImplementationMock is UUPSUpgradeable {
    uint256 public value;
    address public owner;

    event ValueSet(uint256 indexed newValue);

    function initialize(address initialOwner, uint256 initialValue) external {
        require(owner == address(0), "Already initialized");
        owner = initialOwner;
        value = initialValue;
    }

    function setValue(uint256 newValue) external {
        value = newValue;
        emit ValueSet(newValue);
    }

    function _authorizeUpgrade(address newImplementation) internal view override {
        require(msg.sender == owner, "Only owner can upgrade");
        require(newImplementation != address(0), "Invalid new implementation");
    }
}

/**
 * @title UUPSImplementationV2Mock
 * @notice V2 implementation for testing UUPS upgrade delegation.
 */
contract UUPSImplementationV2Mock is UUPSUpgradeable {
    uint256 public value;
    address public owner;

    event ValueMultiplied(uint256 indexed newValue);

    function multiplyValue(uint256 factor) external {
        value = value * factor;
        emit ValueMultiplied(value);
    }

    function _authorizeUpgrade(address newImplementation) internal view override {
        require(msg.sender == owner, "Only owner can upgrade");
        require(newImplementation != address(0), "Invalid new implementation");
    }
}
