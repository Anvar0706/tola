# TOLA Network — Testnet Faucet Architecture & Operations

## 1. Overview

The TOLA Testnet Faucet is an automated, rate-limited token distribution service providing developers and community validators with testnet coins (`tTOLA`). Testnet tokens have zero real-world economic value and exist solely for smart contract testing, gas execution, and testnet staking operations.

---

## 2. Token Allocation & Distribution Rules

| Parameter | Configuration | Description |
| :--- | :--- | :--- |
| **Default Payout** | $10\text{ tTOLA}$ ($10 \times 10^{18}\text{ base units}$) | Standard distribution per request |
| **Recipient Limit** | $1\text{ request / 24 hours}$ | Enforced per recipient address |
| **IP Limit** | $1\text{ request / 24 hours}$ | Enforced per client IPv4 / IPv6 |
| **Target Gas Limit** | $21,000\text{ gas}$ | Standard transfer gas cost |
| **EIP-1559 Fee** | $1\text{ Gwei base fee} + 1\text{ Gwei priority}$ | Fast inclusion in next block |

---

## 3. Architecture & Transaction Flow

```
┌─────────────────────────────────┐
│ Developer / CLI / Web Browser   │
└────────────────┬────────────────┘
                 │ HTTP POST /request {"address": "0x..."}
                 ▼
┌────────────────────────────────────────────────────────┐
│               TOLA Faucet Service                      │
│  - Address & IP Rate Limiter (24h sliding window)      │
│  - Secp256k1 Hot Wallet Keypair                        │
│  - Transaction Builder & Nonce Tracker                 │
└────────────────┬───────────────────────────────────────┘
                 │ Signed Wire Transfer (eth_sendRawTransaction)
                 ▼
┌────────────────────────────────────────────────────────┐
│             Testnet Validator Node (RPC)               │
│  - Ingests into Mempool Priority Queue                 │
│  - Parallel Block-STM Speculative Execution            │
│  - BFT Consensus Commit & Finality                     │
└────────────────────────────────────────────────────────┘
```

---

## 4. Operational Commands (`tola-cli`)

### Running the Faucet Service
```bash
tola-cli faucet serve \
    --port 8080 \
    --faucet-key 0100000000000000000000000000000000000000000000000000000000000000 \
    --rpc-url 127.0.0.1:8545 \
    --amount 10
```

### Requesting Tokens via CLI
```bash
tola-cli faucet request \
    --to 0x9342a6e6b173e23fa0cbc78877c0b3d4ae2e48eb \
    --faucet-url http://127.0.0.1:8080
```

---

## 5. HTTP API Specification

### 1. Request Funds
- **Method:** `POST /request` or `POST /faucet`
- **Request Headers:** `Content-Type: application/json`
- **Request Body:**
  ```json
  {
    "address": "0x9342a6e6b173e23fa0cbc78877c0b3d4ae2e48eb"
  }
  ```
- **Success Response (200 OK):**
  ```json
  {
    "status": "ok",
    "tx_hash": "0x7f83b1657ff1fc53b92dc18148a1d65dfc2d4b1fa3d677284addd200126d9069",
    "recipient": "0x9342a6e6b173e23fa0cbc78877c0b3d4ae2e48eb",
    "amount_tola": 10
  }
  ```
- **Rate Limit Response (429 Too Many Requests):**
  ```json
  {
    "error": "Address rate limit exceeded. Try again in 82400 seconds"
  }
  ```

### 2. Service Status
- **Method:** `GET /status` or `GET /health`
- **Response (200 OK):**
  ```json
  {
    "status": "ok",
    "faucet_address": "0xcecc1507dc1ddd7295951c290888f095adb9044d",
    "payout_tola": 10
  }
  ```

---

## 6. Security & Hot Wallet Operations

1. **Hot Wallet Balance Management:**
   Never fund the faucet hot wallet with more than 30 days of projected payouts.
2. **Reverse Proxy Protection:**
   Place the faucet behind a reverse proxy (e.g. Nginx or Cloudflare) with rate limiting and DDoS mitigations.
3. **CORS Headers:**
   The faucet automatically responds to preflight `OPTIONS` requests, allowing direct integration into developer frontends and web applications.
