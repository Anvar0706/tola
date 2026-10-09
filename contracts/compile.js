const fs = require('fs');
const path = require('path');
const solc = require('solc');

const SRC_DIR = path.join(__dirname, 'src');
const COMPILED_DIR = path.join(__dirname, 'compiled');

if (!fs.existsSync(COMPILED_DIR)) {
  fs.mkdirSync(COMPILED_DIR, { recursive: true });
}

function findImports(importPath) {
  if (importPath.startsWith('@openzeppelin/contracts/')) {
    const fullPath = path.join(__dirname, 'node_modules', importPath);
    if (fs.existsSync(fullPath)) {
      return { contents: fs.readFileSync(fullPath, 'utf8') };
    }
  }
  const localPath = path.join(SRC_DIR, importPath);
  if (fs.existsSync(localPath)) {
    return { contents: fs.readFileSync(localPath, 'utf8') };
  }
  return { error: 'File not found: ' + importPath };
}

const sources = {};
const solFiles = fs.readdirSync(SRC_DIR).filter(f => f.endsWith('.sol'));
for (const file of solFiles) {
  sources[file] = {
    content: fs.readFileSync(path.join(SRC_DIR, file), 'utf8')
  };
}

const input = {
  language: 'Solidity',
  sources: sources,
  settings: {
    evmVersion: 'shanghai',
    optimizer: {
      enabled: true,
      runs: 200
    },
    metadata: {
      bytecodeHash: 'none'
    },
    outputSelection: {
      '*': {
        '*': ['abi', 'evm.bytecode.object', 'evm.deployedBytecode.object', 'metadata']
      }
    }
  }
};

console.log('Compiling Solidity contracts with solc', solc.version(), 'targeting shanghai...');
const output = JSON.parse(solc.compile(JSON.stringify(input), { import: findImports }));

if (output.errors) {
  let hasError = false;
  for (const err of output.errors) {
    if (err.severity === 'error') {
      console.error(err.formattedMessage);
      hasError = true;
    } else {
      console.warn(err.formattedMessage);
    }
  }
  if (hasError) {
    process.exit(1);
  }
}

let count = 0;
for (const sourceFile in output.contracts) {
  for (const contractName in output.contracts[sourceFile]) {
    const contract = output.contracts[sourceFile][contractName];
    const bytecode = contract.evm.bytecode.object;
    const deployedBytecode = contract.evm.deployedBytecode.object;

    if (!bytecode || bytecode.length === 0) continue;

    const artifact = {
      contractName: contractName,
      sourceFile: sourceFile,
      compiler: '0.8.24',
      evmVersion: 'shanghai',
      optimizer: {
        enabled: true,
        runs: 200
      },
      abi: contract.abi,
      bytecode: bytecode.startsWith('0x') ? bytecode : '0x' + bytecode,
      deployedBytecode: deployedBytecode.startsWith('0x') ? deployedBytecode : '0x' + deployedBytecode
    };

    const outPath = path.join(COMPILED_DIR, `${contractName}.json`);
    fs.writeFileSync(outPath, JSON.stringify(artifact, null, 2), 'utf8');
    console.log(`Saved artifact: ${contractName}.json (${bytecode.length / 2} bytes initcode)`);
    count++;
  }
}

console.log(`Successfully compiled ${count} contract artifacts to ${COMPILED_DIR}`);
