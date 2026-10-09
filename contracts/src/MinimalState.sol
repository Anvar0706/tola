// SPDX-License-Identifier: MIT
pragma solidity 0.8.24;

/**
 * @title MinimalState
 * @notice Basic state contract for testing constructor args, getters, setters, and return values.
 */
contract MinimalState {
    uint256 private _value;
    address private _owner;
    bool private _flag;

    event ValueChanged(uint256 indexed oldValue, uint256 indexed newValue);
    event OwnerChanged(address indexed oldOwner, address indexed newOwner);
    event FlagChanged(bool indexed oldFlag, bool indexed newFlag);

    constructor(uint256 initialValue, address initialOwner, bool initialFlag) {
        _value = initialValue;
        _owner = initialOwner;
        _flag = initialFlag;
    }

    function getValue() external view returns (uint256) {
        return _value;
    }

    function getOwner() external view returns (address) {
        return _owner;
    }

    function getFlag() external view returns (bool) {
        return _flag;
    }

    function setValue(uint256 newValue) external {
        uint256 old = _value;
        _value = newValue;
        emit ValueChanged(old, newValue);
    }

    function setOwner(address newOwner) external {
        address old = _owner;
        _owner = newOwner;
        emit OwnerChanged(old, newOwner);
    }

    function setFlag(bool newFlag) external {
        bool old = _flag;
        _flag = newFlag;
        emit FlagChanged(old, newFlag);
    }
}
