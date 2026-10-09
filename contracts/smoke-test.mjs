import { spawn } from 'child_process';
import { existsSync, writeFileSync, createWriteStream, mkdtempSync } from 'fs';
import { join, resolve } from 'path';
import { tmpdir } from 'os';
import { pathToFileURL, fileURLToPath } from 'url';
import { createRequire } from 'module';

const __dirname = resolve(fileURLToPath(import.meta.url), '..');
const repoDir = resolve(__dirname, '..');

const require = createRequire(import.meta.url);
const solc = require(join(__dirname, 'node_modules', 'solc'));

const { TolaWallet } = await import(pathToFileURL(join(repoDir, 'sdk', 'typescript', 'dist', 'wallet.js')).href);
const { JsonRpcProvider } = await import(pathToFileURL(join(repoDir, 'sdk', 'typescript', 'dist', 'provider.js')).href);
const { keccak256 } = await import(pathToFileURL(join(repoDir, 'sdk', 'typescript', 'dist', 'crypto', 'keccak256.js')).href);
const { signDigest } = await import(pathToFileURL(join(repoDir, 'sdk', 'typescript', 'dist', 'crypto', 'secp256k1.js')).href);

function toHex(bytes) {
  return Array.from(bytes).map(b => b.toString(16).padStart(2, '0')).join('');
}

function pad32(val) {
  const b = typeof val === 'bigint' ? val.toString(16) : BigInt(val).toString(16);
  return b.padStart(64, '0');
}

function encodeLength(len, offset) {
  if (len < 56) return Buffer.from([len + offset]);
  const hex = len.toString(16);
  const hexPadded = hex.length % 2 === 0 ? hex : '0' + hex;
  const lenBuf = Buffer.from(hexPadded, 'hex');
  return Buffer.concat([Buffer.from([lenBuf.length + offset + 55]), lenBuf]);
}

function rlpEncode(item) {
  if (Array.isArray(item)) {
    const enc = Buffer.concat(item.map(rlpEncode));
    return Buffer.concat([encodeLength(enc.length, 0xc0), enc]);
  }
  const buf = Buffer.isBuffer(item) ? item : (item instanceof Uint8Array ? Buffer.from(item) : Buffer.from(item));
  if (buf.length === 1 && buf[0] < 0x80) return buf;
  return Buffer.concat([encodeLength(buf.length, 0x80), buf]);
}

function toRlpBytes(num) {
  const bi = BigInt(num);
  if (bi === 0n) return Buffer.alloc(0);
  let hex = bi.toString(16);
  if (hex.length % 2 !== 0) hex = '0' + hex;
  return Buffer.from(hex, 'hex');
}

async function signType2Tx({
  chainId,
  nonce,
  maxPriorityFeePerGas,
  maxFeePerGas,
  gasLimit,
  to,
  value,
  data,
  accessList = [],
  privateKeyHex,
}) {
  const toBuf = to ? Buffer.from(to.replace(/^0x/i, ''), 'hex') : Buffer.alloc(0);
  const dataBuf = data ? Buffer.from(data.replace(/^0x/i, ''), 'hex') : Buffer.alloc(0);

  const unsignedList = [
    toRlpBytes(chainId),
    toRlpBytes(nonce),
    toRlpBytes(maxPriorityFeePerGas),
    toRlpBytes(maxFeePerGas),
    toRlpBytes(gasLimit),
    toBuf,
    toRlpBytes(value),
    dataBuf,
    accessList,
  ];

  const unsignedPayload = Buffer.concat([Buffer.from([0x02]), rlpEncode(unsignedList)]);
  const signingHash = keccak256(unsignedPayload);

  const privScalar = BigInt(privateKeyHex.startsWith('0x') ? privateKeyHex : '0x' + privateKeyHex);
  const { r, s, v } = await signDigest(privScalar, signingHash);

  const rBuf = Buffer.from(r);
  const sBuf = Buffer.from(s);

  const signedList = [
    toRlpBytes(chainId),
    toRlpBytes(nonce),
    toRlpBytes(maxPriorityFeePerGas),
    toRlpBytes(maxFeePerGas),
    toRlpBytes(gasLimit),
    toBuf,
    toRlpBytes(value),
    dataBuf,
    accessList,
    toRlpBytes(v),
    rBuf,
    sBuf,
  ];

  const rawEnvelope = Buffer.concat([Buffer.from([0x02]), rlpEncode(signedList)]);
  const txHash = '0x' + toHex(keccak256(rawEnvelope));

  return {
    rawHex: '0x' + rawEnvelope.toString('hex'),
    txHash,
  };
}

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

const results = {};

async function run() {
  console.log('================================================================');
  console.log('       TOLA NETWORK — REAL EVM END-TO-END SMOKE TEST            ');
  console.log('================================================================\n');

  let nodeBinary = join(repoDir, 'target', 'debug', 'tola-node.exe');
  if (!existsSync(nodeBinary)) {
    nodeBinary = join(repoDir, 'target', 'release', 'tola-node.exe');
  }
  if (!existsSync(nodeBinary)) {
    nodeBinary = join(repoDir, 'target', 'debug', 'tola-node');
  }

  if (!existsSync(nodeBinary)) {
    throw new Error(`tola-node binary not found at ${nodeBinary}. Please run: cargo build -p tola-node`);
  }

  // Set up dedicated temporary directory and ports
  const tmpDir = mkdtempSync(join(tmpdir(), 'tola-e2e-smoke-'));
  const logPath = join(tmpDir, 'node_stdout.log');
  const genesisPath = join(tmpDir, 'test-genesis.json');

  const rpcPort = 39545;
  const p2pPort = 39303;
  const chainId = 102n;

  // Genesis allocation for test funder (BIP-39 test vector mnemonic)
  const GENESIS_PHRASE = 'abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about';
  const genesisWallet = TolaWallet.fromMnemonic(GENESIS_PHRASE);
  const genesisAddress = genesisWallet.rawAddress; // 0x3b45664e195dbacd0035046b511491e09bd3ca8a
  const genesisPrivKeyHex = genesisWallet.exportPrivateKeyHex();
  const initialFund = 10000000000000000000000n; // 10,000 TOLA

  const genesisConfig = {
    chain_id: Number(chainId),
    genesis_time: 1770000000,
    accounts: [
      {
        address: genesisAddress,
        balance: initialFund.toString(),
      },
    ],
    validators: [],
  };

  writeFileSync(genesisPath, JSON.stringify(genesisConfig, null, 2));
  console.log(`[SETUP] Isolated data dir: ${tmpDir}`);
  console.log(`[SETUP] Genesis file:     ${genesisPath}`);
  console.log(`[SETUP] Funder account:   ${genesisAddress} (Allocated: 10,000 TOLA)`);
  console.log(`[SETUP] Target RPC:       http://127.0.0.1:${rpcPort}`);
  console.log(`[SETUP] Target P2P:       127.0.0.1:${p2pPort}\n`);

  let child = null;
  let provider = new JsonRpcProvider(`http://127.0.0.1:${rpcPort}`);

  async function startNode() {
    const logStream = createWriteStream(logPath, { flags: 'a' });
    const proc = spawn(
      nodeBinary,
      [
        '--chain-id', String(chainId),
        '--data-dir', tmpDir,
        '--p2p-addr', `127.0.0.1:${p2pPort}`,
        '--rpc-addr', `127.0.0.1:${rpcPort}`,
        '--validator',
        '--genesis', genesisPath,
        '--log-level', 'info',
      ],
      { stdio: ['ignore', 'pipe', 'pipe'] }
    );

    proc.stdout.pipe(logStream);
    proc.stderr.pipe(logStream);

    return proc;
  }

  try {
    // -------------------------------------------------------------
    // STAGE 1: Node Startup
    // -------------------------------------------------------------
    console.log('>>> [STAGE 1] Starting Real Node Daemon...');
    child = await startNode();
    const pid = child.pid;
    console.log(`    Node spawned with PID: ${pid}`);

    let ready = false;
    for (let i = 0; i < 40; i++) {
      try {
        const health = await provider.send('tola_getHealth');
        if (health && health.status === 'ok') {
          ready = true;
          break;
        }
      } catch {
        await sleep(200);
      }
    }

    if (!ready) {
      throw new Error('Node failed to report ready on tola_getHealth within timeout');
    }

    results.stage1 = {
      status: 'PASS',
      pid,
      rpcAddr: `127.0.0.1:${rpcPort}`,
      chainId: Number(chainId),
      dataDir: tmpDir,
      logPath,
    };
    console.log('    ✓ Stage 1 Node Startup: PASS\n');

    // -------------------------------------------------------------
    // STAGE 2: JSON-RPC chain ID and block height
    // -------------------------------------------------------------
    console.log('>>> [STAGE 2] Verifying JSON-RPC Interface...');
    const rpcChainId = await provider.send('eth_chainId');
    const rpcBlockNumber = await provider.send('eth_blockNumber');
    const rpcNetVersion = await provider.send('net_version');
    const rpcClientVersion = await provider.send('web3_clientVersion');
    const rpcHealth = await provider.send('tola_getHealth');

    console.log(`    eth_chainId:        ${rpcChainId} (expected 0x66)`);
    console.log(`    eth_blockNumber:    ${rpcBlockNumber} (expected 0x0)`);
    console.log(`    net_version:        ${rpcNetVersion}`);
    console.log(`    web3_clientVersion: ${rpcClientVersion}`);
    console.log(`    tola_getHealth:     ${JSON.stringify(rpcHealth)}`);

    if (rpcChainId !== '0x66' || rpcBlockNumber !== '0x0') {
      throw new Error(`Unexpected initial RPC response: chainId=${rpcChainId}, blockNumber=${rpcBlockNumber}`);
    }

    results.stage2 = {
      status: 'PASS',
      rpcChainId,
      rpcBlockNumber,
      rpcNetVersion,
      rpcClientVersion,
    };
    console.log('    ✓ Stage 2 JSON-RPC Verification: PASS\n');

    // -------------------------------------------------------------
    // STAGE 3: Test account funding and signing
    // -------------------------------------------------------------
    console.log('>>> [STAGE 3] Creating & Funding Disposable Test Account (Alice)...');
    const aliceWallet = TolaWallet.generate();
    const bobWallet = TolaWallet.generate();

    const aliceAddr = aliceWallet.rawAddress;
    const alicePrivKeyHex = aliceWallet.exportPrivateKeyHex();
    const bobAddr = bobWallet.rawAddress;

    console.log(`    Genesis Funder: ${genesisAddress}`);
    console.log(`    Alice Address:  ${aliceAddr}`);
    console.log(`    Bob Address:    ${bobAddr}`);

    const fundAmount = 100000000000000000000n; // 100 TOLA
    const { rawHex: fundRawHex, txHash: expectedFundHash } = await signType2Tx({
      chainId,
      nonce: 0n,
      maxPriorityFeePerGas: 1000000000n,
      maxFeePerGas: 1000000000n,
      gasLimit: 21000n,
      to: aliceAddr,
      value: fundAmount,
      data: null,
      privateKeyHex: genesisPrivKeyHex,
    });

    const fundTxHash = await provider.sendRawTransaction(fundRawHex);
    console.log(`    Funding Transaction Submitted: ${fundTxHash} (expected: ${expectedFundHash})`);

    // Wait for Block #1
    let h1 = 0n;
    for (let i = 0; i < 40; i++) {
      await sleep(150);
      h1 = await provider.getBlockNumber();
      if (h1 >= 1n) break;
    }
    if (h1 < 1n) throw new Error('Funding transaction block #1 was not committed');

    const aliceInitialBal = await provider.getBalance(aliceAddr);
    const aliceInitialNonce = await provider.getTransactionCount(aliceAddr);
    console.log(`    Block #1 committed! Alice balance: ${aliceInitialBal} nano-TOLA, nonce: ${aliceInitialNonce}`);

    if (aliceInitialBal !== fundAmount || aliceInitialNonce !== 0n) {
      throw new Error(`Alice balance/nonce mismatch: bal=${aliceInitialBal}, nonce=${aliceInitialNonce}`);
    }

    results.stage3 = {
      status: 'PASS',
      funderTxHash: fundTxHash,
      aliceAddress: aliceAddr,
      aliceInitialBalance: aliceInitialBal.toString(),
      aliceInitialNonce: Number(aliceInitialNonce),
      blockNumber: Number(h1),
    };
    console.log('    ✓ Stage 3 Test Account Funding: PASS\n');

    // -------------------------------------------------------------
    // STAGE 4: Signed value transfer and committed receipt
    // -------------------------------------------------------------
    console.log('>>> [STAGE 4] Submitting Signed EVM Value Transfer (Alice -> Bob)...');
    const transferAmount = 5000000000000000000n; // 5 TOLA
    const { rawHex: transferRawHex, txHash: expectedTransferHash } = await signType2Tx({
      chainId,
      nonce: 0n,
      maxPriorityFeePerGas: 1000000000n,
      maxFeePerGas: 1000000000n,
      gasLimit: 21000n,
      to: bobAddr,
      value: transferAmount,
      data: null,
      privateKeyHex: alicePrivKeyHex,
    });

    const transferTxHash = await provider.sendRawTransaction(transferRawHex);
    console.log(`    Value Transfer Submitted: ${transferTxHash} (expected: ${expectedTransferHash})`);

    // Wait for Block #2
    let h2 = 0n;
    for (let i = 0; i < 40; i++) {
      await sleep(150);
      h2 = await provider.getBlockNumber();
      if (h2 >= 2n) break;
    }
    if (h2 < 2n) throw new Error('Transfer transaction block #2 was not committed');

    // Query receipt
    const transferReceipt = await provider.getTransactionReceipt(transferTxHash);
    console.log(`    Receipt Status:       ${transferReceipt.status}`);
    console.log(`    Receipt BlockNumber:  ${transferReceipt.blockNumber}`);
    console.log(`    Receipt GasUsed:      ${transferReceipt.gasUsed}`);
    console.log(`    Receipt EffGasPrice:  ${transferReceipt.effectiveGasPrice}`);

    if (transferReceipt.status !== '0x1') {
      throw new Error(`Transfer receipt status failed: ${JSON.stringify(transferReceipt)}`);
    }

    // Query transaction by hash
    const queriedTx = await provider.getTransactionByHash(transferTxHash);
    console.log(`    Queried Tx By Hash:   from=${queriedTx.from}, to=${queriedTx.to}, value=${queriedTx.value}`);

    const bobBal = await provider.getBalance(bobAddr);
    const aliceBalAfter = await provider.getBalance(aliceAddr);
    const aliceNonceAfter = await provider.getTransactionCount(aliceAddr);

    console.log(`    Bob Balance:          ${bobBal} nano-TOLA (expected ${transferAmount})`);
    console.log(`    Alice Remaining:      ${aliceBalAfter} nano-TOLA`);
    console.log(`    Alice Nonce:          ${aliceNonceAfter} (expected 1)`);

    if (bobBal !== transferAmount) throw new Error(`Bob balance mismatch: expected ${transferAmount}, got ${bobBal}`);
    if (aliceNonceAfter !== 1n) throw new Error(`Alice nonce mismatch: expected 1, got ${aliceNonceAfter}`);

    results.stage4 = {
      status: 'PASS',
      txHash: transferTxHash,
      blockNumber: Number(h2),
      gasUsed: transferReceipt.gasUsed,
      effectiveGasPrice: transferReceipt.effectiveGasPrice,
      bobBalance: bobBal.toString(),
      aliceNonce: Number(aliceNonceAfter),
    };
    console.log('    ✓ Stage 4 Signed EVM Value Transfer: PASS\n');

    // -------------------------------------------------------------
    // STAGE 5: Deploy and interact with a Solidity contract
    // -------------------------------------------------------------
    console.log('>>> [STAGE 5] Compiling & Deploying SmokeTest Solidity Contract (EVM Type-2)...');
    const solSource = `// SPDX-License-Identifier: MIT
pragma solidity 0.8.24;

contract SmokeTest {
    uint256 public value;
    event ValueUpdated(uint256 indexed oldValue, uint256 indexed newValue);

    function setValue(uint256 newValue) external {
        uint256 old = value;
        value = newValue;
        emit ValueUpdated(old, newValue);
    }
}`;

    const solcInput = {
      language: 'Solidity',
      sources: { 'SmokeTest.sol': { content: solSource } },
      settings: {
        evmVersion: 'shanghai',
        outputSelection: { '*': { '*': ['abi', 'evm.bytecode'] } },
      },
    };

    const solcOutput = JSON.parse(solc.compile(JSON.stringify(solcInput)));
    if (solcOutput.errors && solcOutput.errors.some((e) => e.severity === 'error')) {
      throw new Error(`Solidity compilation failed: ${JSON.stringify(solcOutput.errors)}`);
    }

    const compiledContract = solcOutput.contracts['SmokeTest.sol']['SmokeTest'];
    const initcodeHex = compiledContract.evm.bytecode.object;
    console.log(`    Compiled SmokeTest with solc 0.8.24 (Shanghai EVM target). Initcode size: ${initcodeHex.length / 2} bytes`);

    // Alice deploys the contract via Type-2 transaction (to: null, nonce = 1)
    const { rawHex: deployRawHex, txHash: expectedDeployHash } = await signType2Tx({
      chainId,
      nonce: 1n,
      maxPriorityFeePerGas: 1000000000n,
      maxFeePerGas: 1000000000n,
      gasLimit: 500000n,
      to: null, // Contract creation
      value: 0n,
      data: initcodeHex,
      privateKeyHex: alicePrivKeyHex,
    });

    const deployTxHash = await provider.sendRawTransaction(deployRawHex);
    console.log(`    Contract Deployment Tx Submitted: ${deployTxHash} (expected: ${expectedDeployHash})`);

    // Wait for Block #3
    let h3 = 0n;
    for (let i = 0; i < 40; i++) {
      await sleep(150);
      h3 = await provider.getBlockNumber();
      if (h3 >= 3n) break;
    }
    if (h3 < 3n) throw new Error('Contract deployment block #3 was not committed');

    const deployReceipt = await provider.getTransactionReceipt(deployTxHash);
    console.log(`    Deploy Receipt Status:       ${deployReceipt.status}`);
    console.log(`    Deployed Contract Address:   ${deployReceipt.contractAddress}`);
    console.log(`    Deploy Gas Used:             ${deployReceipt.gasUsed}`);

    if (deployReceipt.status !== '0x1' || !deployReceipt.contractAddress) {
      throw new Error(`Deployment failed: ${JSON.stringify(deployReceipt)}`);
    }

    const contractAddress = deployReceipt.contractAddress;

    // Verify deployed runtime bytecode via eth_getCode
    const runtimeCode = await provider.send('eth_getCode', [contractAddress, 'latest']);
    console.log(`    Deployed Runtime Bytecode:   ${runtimeCode.slice(0, 34)}... (length: ${runtimeCode.length} hex chars)`);

    if (!runtimeCode || runtimeCode === '0x' || runtimeCode === '0x0') {
      throw new Error('Deployed contract has empty runtime bytecode on-chain');
    }

    results.stage5 = {
      status: 'PASS',
      deployTxHash,
      contractAddress,
      blockNumber: Number(h3),
      gasUsed: deployReceipt.gasUsed,
      runtimeCodeLength: runtimeCode.length,
    };
    console.log('    ✓ Stage 5 Contract Deployment: PASS\n');

    // -------------------------------------------------------------
    // STAGE 6: Contract function execution and storage verification
    // -------------------------------------------------------------
    console.log('>>> [STAGE 6] Interacting with Contract (Read -> setValue(42) -> Verify)...');

    const valSelector = '0x3fa4f245';
    const setSelector = '0x55241077';

    // 1. Initial read via eth_call
    const initialCallVal = await provider.send('eth_call', [{ to: contractAddress, data: valSelector }, 'latest']);
    console.log(`    Initial eth_call value():    ${initialCallVal} (expected 0)`);
    if (BigInt(initialCallVal) !== 0n) throw new Error(`Initial value mismatch: ${initialCallVal}`);

    // 2. Initial storage via eth_getStorageAt slot 0
    const initialSlot0 = await provider.send('eth_getStorageAt', [contractAddress, '0x0', 'latest']);
    console.log(`    Initial Slot 0 Storage:      ${initialSlot0} (expected 0)`);
    if (BigInt(initialSlot0) !== 0n) throw new Error(`Initial storage slot 0 mismatch: ${initialSlot0}`);

    // 3. Alice calls setValue(42) (nonce = 2)
    const calldataSet42 = setSelector + pad32(42n);
    const { rawHex: callRawHex, txHash: expectedCallHash } = await signType2Tx({
      chainId,
      nonce: 2n,
      maxPriorityFeePerGas: 1000000000n,
      maxFeePerGas: 1000000000n,
      gasLimit: 100000n,
      to: contractAddress,
      value: 0n,
      data: calldataSet42,
      privateKeyHex: alicePrivKeyHex,
    });

    const callTxHash = await provider.sendRawTransaction(callRawHex);
    console.log(`    setValue(42) Tx Submitted:  ${callTxHash} (expected: ${expectedCallHash})`);

    // Wait for Block #4
    let h4 = 0n;
    for (let i = 0; i < 40; i++) {
      await sleep(150);
      h4 = await provider.getBlockNumber();
      if (h4 >= 4n) break;
    }
    if (h4 < 4n) throw new Error('Contract call block #4 was not committed');

    const callReceipt = await provider.getTransactionReceipt(callTxHash);
    console.log(`    Call Receipt Status:         ${callReceipt.status}`);
    console.log(`    Call Gas Used:               ${callReceipt.gasUsed}`);

    if (callReceipt.status !== '0x1') {
      throw new Error(`setValue(42) receipt status failed: ${JSON.stringify(callReceipt)}`);
    }

    // 4. Verify updated value via eth_call
    const updatedCallVal = await provider.send('eth_call', [{ to: contractAddress, data: valSelector }, 'latest']);
    console.log(`    Updated eth_call value():    ${updatedCallVal} (dec: ${BigInt(updatedCallVal)})`);

    // 5. Verify updated storage via eth_getStorageAt
    const updatedSlot0 = await provider.send('eth_getStorageAt', [contractAddress, '0x0', 'latest']);
    console.log(`    Updated Slot 0 Storage:      ${updatedSlot0} (dec: ${BigInt(updatedSlot0)})`);

    if (BigInt(updatedCallVal) !== 42n || BigInt(updatedSlot0) !== 42n) {
      throw new Error(`State verification failed: callVal=${updatedCallVal}, slot0=${updatedSlot0}`);
    }

    // 6. Verify read-only simulation did NOT mutate state or increment nonce
    const simVal = await provider.send('eth_call', [{ to: contractAddress, data: valSelector }, 'latest']);
    const aliceNonceCheck = await provider.getTransactionCount(aliceAddr);
    console.log(`    Read-only simulation check:  val=${BigInt(simVal)}, aliceNonce=${aliceNonceCheck}`);
    if (aliceNonceCheck !== 3n) throw new Error(`Unexpected nonce after read simulation: ${aliceNonceCheck}`);

    results.stage6 = {
      status: 'PASS',
      callTxHash,
      blockNumber: Number(h4),
      gasUsed: callReceipt.gasUsed,
      observedValue: Number(BigInt(updatedCallVal)),
      observedSlot0: updatedSlot0,
    };
    console.log('    ✓ Stage 6 Contract Execution & Storage: PASS\n');

    // -------------------------------------------------------------
    // STAGE 7: Gas, balance, and nonce verification
    // -------------------------------------------------------------
    console.log('>>> [STAGE 7] Verifying Gas Accounting, Balances, & Nonces...');
    const aliceFinalBal = await provider.getBalance(aliceAddr);
    const aliceFinalNonce = await provider.getTransactionCount(aliceAddr);

    console.log(`    Alice Initial Balance: ${fundAmount} nano-TOLA`);
    console.log(`    Alice Final Balance:   ${aliceFinalBal} nano-TOLA`);
    console.log(`    Alice Final Nonce:     ${aliceFinalNonce} (expected 3)`);

    const transferGas = BigInt(transferReceipt.gasUsed);
    const deployGas = BigInt(deployReceipt.gasUsed);
    const callGas = BigInt(callReceipt.gasUsed);
    const totalGas = transferGas + deployGas + callGas;

    console.log(`    Transfer Gas: ${transferGas}`);
    console.log(`    Deploy Gas:   ${deployGas}`);
    console.log(`    Call Gas:     ${callGas}`);
    console.log(`    Total Gas:    ${totalGas}`);

    const gasPrice = 1000000000n; // 1 Gwei
    const totalGasCost = totalGas * gasPrice;
    const expectedAliceBal = fundAmount - transferAmount - totalGasCost;

    console.log(`    Expected Alice Balance: ${expectedAliceBal}`);
    console.log(`    Observed Alice Balance: ${aliceFinalBal}`);

    if (aliceFinalBal !== expectedAliceBal) {
      throw new Error(`Balance accounting mismatch! Expected ${expectedAliceBal}, got ${aliceFinalBal}`);
    }
    if (aliceFinalNonce !== 3n) {
      throw new Error(`Nonce accounting mismatch! Expected 3, got ${aliceFinalNonce}`);
    }

    results.stage7 = {
      status: 'PASS',
      aliceBalance: aliceFinalBal.toString(),
      aliceNonce: Number(aliceFinalNonce),
      totalGasUsed: Number(totalGas),
      totalGasCost: totalGasCost.toString(),
    };
    console.log('    ✓ Stage 7 Gas, Balance, & Nonce Verification: PASS\n');

    // -------------------------------------------------------------
    // STAGE 8: Restart and persistent state verification
    // -------------------------------------------------------------
    console.log('>>> [STAGE 8] Cold Restarting Node & Verifying Persistence Across Restart...');
    const preRestartHeight = await provider.getBlockNumber();
    console.log(`    Pre-restart committed height: ${preRestartHeight}`);

    // Gracefully terminate node
    child.kill();
    await sleep(600);
    console.log('    Node stopped.');

    // Restart node with identical data directory
    console.log('    Restarting node with identical data directory...');
    child = await startNode();
    console.log(`    Restarted node PID: ${child.pid}`);

    let restartReady = false;
    for (let i = 0; i < 40; i++) {
      try {
        const health = await provider.send('tola_getHealth');
        if (health && health.status === 'ok') {
          restartReady = true;
          break;
        }
      } catch {
        await sleep(200);
      }
    }
    if (!restartReady) throw new Error('Restarted node failed to become healthy');

    // 1. Verify height did not regress
    const postRestartHeight = await provider.getBlockNumber();
    console.log(`    Post-restart height:         ${postRestartHeight} (expected >= ${preRestartHeight})`);
    if (postRestartHeight < preRestartHeight) {
      throw new Error(`Height regressed after restart: before=${preRestartHeight}, after=${postRestartHeight}`);
    }

    // 2. Verify deployed runtime bytecode remains present
    const postRestartCode = await provider.send('eth_getCode', [contractAddress, 'latest']);
    console.log(`    Post-restart contract code:  ${postRestartCode.slice(0, 34)}... (length: ${postRestartCode.length})`);
    if (postRestartCode !== runtimeCode) {
      throw new Error('Contract runtime bytecode corrupted or missing after cold restart');
    }

    // 3. Verify storage slot 0 remains 42
    const postRestartSlot0 = await provider.send('eth_getStorageAt', [contractAddress, '0x0', 'latest']);
    console.log(`    Post-restart Slot 0 storage: ${postRestartSlot0} (dec: ${BigInt(postRestartSlot0)})`);
    if (BigInt(postRestartSlot0) !== 42n) {
      throw new Error(`Storage slot 0 lost after restart: expected 42, got ${BigInt(postRestartSlot0)}`);
    }

    // 4. Verify eth_call value() returns 42
    const postRestartCallVal = await provider.send('eth_call', [{ to: contractAddress, data: valSelector }, 'latest']);
    console.log(`    Post-restart eth_call:       ${postRestartCallVal} (dec: ${BigInt(postRestartCallVal)})`);
    if (BigInt(postRestartCallVal) !== 42n) {
      throw new Error(`eth_call value() lost after restart: expected 42, got ${BigInt(postRestartCallVal)}`);
    }

    // 5. Verify transaction receipts remain available
    const postRestartDeployReceipt = await provider.getTransactionReceipt(deployTxHash);
    const postRestartCallReceipt = await provider.getTransactionReceipt(callTxHash);
    if (!postRestartDeployReceipt || postRestartDeployReceipt.status !== '0x1') {
      throw new Error('Deploy receipt unavailable or corrupted after restart');
    }
    if (!postRestartCallReceipt || postRestartCallReceipt.status !== '0x1') {
      throw new Error('Call receipt unavailable or corrupted after restart');
    }
    console.log('    Post-restart receipts verified intact.');

    // 6. Verify balances and nonces remain consistent
    const postRestartAliceBal = await provider.getBalance(aliceAddr);
    const postRestartAliceNonce = await provider.getTransactionCount(aliceAddr);
    const postRestartBobBal = await provider.getBalance(bobAddr);

    if (postRestartAliceBal !== aliceFinalBal || postRestartAliceNonce !== aliceFinalNonce || postRestartBobBal !== bobBal) {
      throw new Error('Account balances/nonces shifted after cold restart');
    }
    console.log('    Post-restart balances & nonces verified intact.');

    results.stage8 = {
      status: 'PASS',
      postRestartHeight: Number(postRestartHeight),
      contractAddress,
      storageSlot0: postRestartSlot0,
      callValue: Number(BigInt(postRestartCallVal)),
    };
    console.log('    ✓ Stage 8 Cold Restart & Persistence: PASS\n');

    // -------------------------------------------------------------
    // STAGE 9: Post-restart transaction processing
    // -------------------------------------------------------------
    console.log('>>> [STAGE 9] Submitting Post-Restart Transaction (setValue(100))...');
    const calldataSet100 = setSelector + pad32(100n);
    const { rawHex: postRawHex, txHash: expectedPostHash } = await signType2Tx({
      chainId,
      nonce: 3n,
      maxPriorityFeePerGas: 1000000000n,
      maxFeePerGas: 1000000000n,
      gasLimit: 100000n,
      to: contractAddress,
      value: 0n,
      data: calldataSet100,
      privateKeyHex: alicePrivKeyHex,
    });

    const postTxHash = await provider.sendRawTransaction(postRawHex);
    console.log(`    setValue(100) Tx Submitted:  ${postTxHash} (expected: ${expectedPostHash})`);

    // Wait for Block #5
    let h5 = 0n;
    for (let i = 0; i < 40; i++) {
      await sleep(150);
      h5 = await provider.getBlockNumber();
      if (h5 >= 5n) break;
    }
    if (h5 < 5n) throw new Error('Post-restart transaction block #5 was not committed');

    const postReceipt = await provider.getTransactionReceipt(postTxHash);
    console.log(`    Post-Restart Receipt Status: ${postReceipt.status}`);
    console.log(`    Post-Restart Receipt Height: ${postReceipt.blockNumber}`);

    if (postReceipt.status !== '0x1') {
      throw new Error(`Post-restart transaction receipt status failed: ${JSON.stringify(postReceipt)}`);
    }

    // Verify storage slot 0 updated to 100
    const finalCallVal = await provider.send('eth_call', [{ to: contractAddress, data: valSelector }, 'latest']);
    const finalSlot0 = await provider.send('eth_getStorageAt', [contractAddress, '0x0', 'latest']);
    const finalNonce = await provider.getTransactionCount(aliceAddr);

    console.log(`    Final eth_call value():      ${finalCallVal} (dec: ${BigInt(finalCallVal)})`);
    console.log(`    Final Slot 0 Storage:        ${finalSlot0} (dec: ${BigInt(finalSlot0)})`);
    console.log(`    Final Alice Nonce:           ${finalNonce} (expected 4)`);

    if (BigInt(finalCallVal) !== 100n || BigInt(finalSlot0) !== 100n || finalNonce !== 4n) {
      throw new Error(`Post-restart execution verification failed: callVal=${finalCallVal}, slot0=${finalSlot0}, nonce=${finalNonce}`);
    }

    results.stage9 = {
      status: 'PASS',
      postTxHash,
      blockNumber: Number(h5),
      gasUsed: postReceipt.gasUsed,
      finalValue: Number(BigInt(finalCallVal)),
      finalNonce: Number(finalNonce),
    };
    console.log('    ✓ Stage 9 Post-Restart Transaction Processing: PASS\n');

    console.log('================================================================');
    console.log('       ALL 9 SMOKE TEST STAGES COMPLETED WITH 100% PASS         ');
    console.log('================================================================');

    console.log('\nFINAL RESULTS SUMMARY:');
    console.log(JSON.stringify(results, null, 2));

  } finally {
    if (child) {
      console.log('\n[CLEANUP] Stopping test node daemon...');
      child.kill();
      await sleep(400);
      console.log('[CLEANUP] Test node daemon stopped.');
    }
  }
}

run().catch((err) => {
  console.error('\n[FATAL SMOKE TEST FAILURE]:', err);
  process.exit(1);
});
