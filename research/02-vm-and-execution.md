# Research: Virtual Machine & Execution Environments

## 1. Candidate Virtual Machines

| VM Family | Execution Model | Ecosystem Maturity | Tooling & Languages | Determinism | Suitability for TOLA |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **EVM** | Stack-based, 256-bit words | Extremely High | Solidity, Vyper, Hardhat, Foundry | High (with strict spec) | **Selected for Phase 1**: Maximum developer adoption, battle-tested smart contracts. |
| **WASM** | Register/Stack 32/64-bit | High | Rust, C++, AssemblyScript | High (sandboxed) | **Phase 2 Candidate**: High raw execution performance; can run alongside EVM. |
| **Actor TVM** | Actor message passing, Cells | Niche | FunC, Tact | High | **Rejected for Core**: Unnecessarily steep developer learning curve and lack of synchronous composability. |

## 2. Strategic Decision
Implement a **Modular VM Architecture** where the execution engine is decoupled from state storage and consensus via a clean interface. Provide EVM compatibility first to unlock thousands of existing developer tools and audited contract libraries, while designing the state interface to support a secondary high-performance WASM runtime in future phases.
