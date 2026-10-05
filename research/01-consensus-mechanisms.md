# Research: Consensus Mechanisms for Layer-1 Blockchains

## 1. Evaluation of Candidate Consensus Protocols

| Protocol Family | Representative Systems | Finality Speed | Byzantine Fault Tolerance | Complexity | Suitability for TOLA |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **Classic PoW** | Bitcoin, Litecoin | Probabilistic (~60 min) | 50% Crash/Byzantine | Low | **Unsuitable**: High energy consumption, poor latency for messenger micropayments. |
| **Nakamoto PoS** | Cardano (Ouroboros) | Probabilistic (~minutes) | 50% Byzantine | Medium | **Suboptimal**: High latency, lacks instant finality. |
| **BFT-PoS (2-Step)** | Tendermint, Cosmos | Deterministic (1-2 sec) | 33% Byzantine ($< 1/3$) | Medium | **High Suitability**: Instant finality, predictable block times, robust open-source reference implementations. |
| **Pipelined BFT** | HotStuff, AptosBFT | Deterministic (1-2 sec) | 33% Byzantine ($< 1/3$) | Medium-High | **High Suitability**: Linear message complexity $O(n)$ during view change via pacemakers. |
| **DAG-based BFT** | Narwhal-Bullshark, Sui | Sub-second | 33% Byzantine | High | **Future Candidate**: Excellent mempool throughput, but high architectural complexity for initial phase. |

## 2. Recommendation for TOLA Network
Adopt a **Proof-of-Stake with BFT Finality** architecture. For the initial phase, a Tendermint/HotStuff-derived deterministic BFT engine delivers instant finality, predictable latency, and high safety under adversarial network conditions.
