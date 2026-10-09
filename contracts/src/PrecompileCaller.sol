// SPDX-License-Identifier: MIT
pragma solidity 0.8.24;

/**
 * @title PrecompileCaller
 * @notice Solidity contract invoking Ethereum precompiles (0x01 through 0x08) and testing OOG containment.
 */
contract PrecompileCaller {
    function callEcrecover(
        bytes32 hash,
        uint8 v,
        bytes32 r,
        bytes32 s
    ) external pure returns (address) {
        return ecrecover(hash, v, r, s);
    }

    function callSha256(bytes memory data) external pure returns (bytes32) {
        return sha256(data);
    }

    function callRipemd160(bytes memory data) external pure returns (bytes20) {
        return ripemd160(data);
    }

    function callIdentity(bytes memory data) external view returns (bytes memory) {
        (bool ok, bytes memory out) = address(0x04).staticcall(data);
        require(ok, "identity failed");
        return out;
    }

    function callModexp(
        bytes memory input
    ) external view returns (bytes memory) {
        (bool ok, bytes memory out) = address(0x05).staticcall(input);
        require(ok, "modexp failed");
        return out;
    }

    function callEcAdd(bytes memory input) external view returns (bytes memory) {
        (bool ok, bytes memory out) = address(0x06).staticcall(input);
        require(ok, "ecAdd failed");
        return out;
    }

    function callEcPairing(bytes memory input) external view returns (bool) {
        (bool ok, bytes memory out) = address(0x08).staticcall(input);
        require(ok, "ecPairing failed");
        require(out.length >= 32, "pairing short returndata");
        uint256 res = abi.decode(out, (uint256));
        return res == 1;
    }

    function callPrecompileWithBoundedGas(
        address target,
        bytes memory input,
        uint256 gasLimit
    ) external view returns (bool success, bytes memory returndata) {
        (success, returndata) = target.staticcall{gas: gasLimit}(input);
    }
}
