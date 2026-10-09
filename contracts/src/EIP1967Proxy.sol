// SPDX-License-Identifier: MIT
pragma solidity 0.8.24;

import "@openzeppelin/contracts/proxy/ERC1967/ERC1967Proxy.sol";

/**
 * @title EIP1967ProxyWrapper
 * @notice OpenZeppelin ERC1967Proxy deployment wrapper.
 */
contract EIP1967ProxyWrapper is ERC1967Proxy {
    constructor(address logic, bytes memory data) payable ERC1967Proxy(logic, data) {}
}
