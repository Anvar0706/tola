# TOLA Network: Comprehensive Failure Model & Recovery Runbook v0.1

## 1. Byzantine & Distributed Failure Catalog

| Failure ID | Failure Description | Protocol Defense | Automatic Recovery Behavior |
| :--- | :--- | :--- | :--- |
| **FAIL-01** | **Leader Crash / Silence** | Pacemaker timeout $\tau(R)$ expires | Honest nodes broadcast timeout; advance round $R+1$; elect next leader in $< 2.5\text{ s}$. |
| **FAIL-02** | **Validator Equivocation** | Slashing evidence detector | Conflicting signatures submitted to `SlashingEngine`; validator stake burned $10\%$, permanently jailed. |
| **FAIL-03** | **Network Partition (50/50)** | Quorum requires $\ge 2/3 + 1$ | Both partitions halt block commit; zero forks formed; safety 100% preserved. |
| **FAIL-04** | **Network Partition (70/30)** | Majority has quorum | Majority partition continues finalization; minority halts safely; auto-resyncs when healed. |
| **FAIL-05** | **Packet Reordering / Drops** | Monotonic view numbers & buffering | Nodes buffer future round votes; drop stale rounds; request missing QCs via sync protocol. |
| **FAIL-06** | **Sudden Power Loss (kill -9)** | MDBX ACID shadow paging | On reboot, MDBX rolls back uncommitted page; node reads canonical tip and resumes sync in $< 3\text{ s}$. |
| **FAIL-07** | **Disk Full During Commit** | Storage space watermark guard | Node halts gracefully at $95\%$ disk usage; alerts operator before state corruption occurs. |
| **FAIL-08** | **NTP Clock Drift ($> 5\text{ s}$)**| Header timestamp drift guard | Blocks with $\text{Timestamp} > \text{LocalTime} + 5\text{ s}$ rejected; node logs NTP warning. |
| **FAIL-09** | **P2P Eclipse Attack** | Kademlia subnet diversity rules | Limits peer connections to max 2 per `/24` IPv4 subnet; connects to trusted hardcoded bootnodes. |
| **FAIL-10** | **Mempool Flooding (DDoS)** | Priority fee queue & size limits | Low-fee spam evicted; sender nonces bounded to depth 16; zero memory exhaustion. |
