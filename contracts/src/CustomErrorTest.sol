// SPDX-License-Identifier: MIT
pragma solidity 0.8.24;

/**
 * @title CustomErrorTest
 * @notice Tests Solidity 0.8+ custom error emission and selector matching.
 */
contract CustomErrorTest {
    error Unauthorized(address caller, uint256 needed);

    uint256 public balanceRequired;

    constructor(uint256 required) {
        balanceRequired = required;
    }

    function testUnauthorized(uint256 provided) external view returns (bool) {
        if (provided < balanceRequired) {
            revert Unauthorized(msg.sender, balanceRequired);
        }
        return true;
    }

    function triggerRevertTx(uint256 provided) external {
        if (provided < balanceRequired) {
            revert Unauthorized(msg.sender, balanceRequired);
        }
        balanceRequired = provided;
    }
}
