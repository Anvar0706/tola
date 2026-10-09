// SPDX-License-Identifier: MIT
pragma solidity 0.8.24;

/**
 * @title TargetContract
 * @notice Target contract for testing caller context, state changes, and revert conditions.
 */
contract TargetContract {
    address public lastCaller;
    uint256 public valueStored;

    event ValueStored(address indexed caller, uint256 val);

    function storeValue(uint256 val) external returns (address, uint256) {
        lastCaller = msg.sender;
        valueStored = val;
        emit ValueStored(msg.sender, val);
        return (msg.sender, val);
    }

    function readValue() external view returns (address, uint256) {
        return (lastCaller, valueStored);
    }

    function revertWithError(string memory message) external pure {
        revert(message);
    }
}

/**
 * @title CallRouter
 * @notice Contract dispatching CALL and STATICCALL, checking caller context and propagating reverts.
 */
contract CallRouter {
    function routeCall(
        address target,
        bytes memory data
    ) external returns (bool success, bytes memory returndata) {
        (success, returndata) = target.call(data);
    }

    function routeStaticCall(
        address target,
        bytes memory data
    ) external view returns (bool success, bytes memory returndata) {
        (success, returndata) = target.staticcall(data);
    }

    function routeCallWithRevertPropagation(
        address target,
        bytes memory data
    ) external returns (bytes memory) {
        (bool success, bytes memory returndata) = target.call(data);
        if (!success) {
            assembly {
                let returndata_size := mload(returndata)
                revert(add(32, returndata), returndata_size)
            }
        }
        return returndata;
    }
}
