//! Native TOLA CLI Wallet & Recovery Module.
//!
//! Provides CLI subcommands for:
//! - Deterministic BIP-39 mnemonic generation (`wallet generate`)
//! - Terminal-secure recovery (`wallet recover`)
//! - Public address inspection (`wallet address`)
//! - Public key inspection (`wallet public-key`)
//! - Local transaction clear signing (`wallet sign`)

use std::io::{self, Read, Write};
use tola_codec::CanonicalSerialize;
use tola_crypto::{Mnemonic, MnemonicCount, CANONICAL_DERIVATION_PATH};
use tola_rpc::json::JsonValue;
use tola_sdk::TolaWallet;
use tola_types::{
    Address, Amount, BlockHeight, ChainId, Gas, Network, Nonce, Signature, Transaction,
};
use zeroize::Zeroize;

// ============================================================================
// Terminal Security & Secure Input
// ============================================================================

#[cfg(windows)]
mod terminal_guard {
    use std::os::windows::raw::HANDLE;
    const STD_INPUT_HANDLE: u32 = 0xFFFFFFF6; // ((DWORD)-10)
    const ENABLE_ECHO_INPUT: u32 = 0x0004;

    extern "system" {
        fn GetStdHandle(nStdHandle: u32) -> HANDLE;
        fn GetConsoleMode(hConsoleHandle: HANDLE, lpMode: *mut u32) -> i32;
        fn SetConsoleMode(hConsoleHandle: HANDLE, dwMode: u32) -> i32;
    }

    pub struct EchoGuard {
        handle: HANDLE,
        original_mode: u32,
        active: bool,
    }

    impl EchoGuard {
        pub fn disable() -> Self {
            unsafe {
                let handle = GetStdHandle(STD_INPUT_HANDLE);
                let mut mode = 0u32;
                if handle as isize != -1
                    && handle as isize != 0
                    && GetConsoleMode(handle, &mut mode) != 0
                {
                    let new_mode = mode & !ENABLE_ECHO_INPUT;
                    if SetConsoleMode(handle, new_mode) != 0 {
                        return Self {
                            handle,
                            original_mode: mode,
                            active: true,
                        };
                    }
                }
                Self {
                    handle,
                    original_mode: 0,
                    active: false,
                }
            }
        }
    }

    impl Drop for EchoGuard {
        fn drop(&mut self) {
            if self.active {
                unsafe {
                    let _ = SetConsoleMode(self.handle, self.original_mode);
                }
            }
        }
    }
}

#[cfg(unix)]
mod terminal_guard {
    pub struct EchoGuard {
        active: bool,
        orig: libc::termios,
    }

    impl EchoGuard {
        pub fn disable() -> Self {
            unsafe {
                let mut orig = std::mem::zeroed();
                if libc::isatty(libc::STDIN_FILENO) == 1
                    && libc::tcgetattr(libc::STDIN_FILENO, &mut orig) == 0
                {
                    let mut raw = orig;
                    raw.c_lflag &= !libc::ECHO;
                    if libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &raw) == 0 {
                        return Self { active: true, orig };
                    }
                }
                Self {
                    active: false,
                    orig,
                }
            }
        }
    }

    impl Drop for EchoGuard {
        fn drop(&mut self) {
            if self.active {
                unsafe {
                    let _ = libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &self.orig);
                }
            }
        }
    }
}

#[cfg(not(any(windows, unix)))]
mod terminal_guard {
    pub struct EchoGuard;
    impl EchoGuard {
        pub fn disable() -> Self {
            Self
        }
    }
}

/// Reads a line from stdin securely with terminal echo disabled if supported.
/// Memory is zeroized on failure or when no longer needed.
pub fn read_secure_line(prompt: &str) -> Result<String, String> {
    print!("{}", prompt);
    let _ = io::stdout().flush();

    let _guard = terminal_guard::EchoGuard::disable();
    let mut line = String::new();
    let res = io::stdin().read_line(&mut line);
    drop(_guard);

    // Print newline after invisible input for clean terminal display
    println!();

    match res {
        Ok(_) => {
            let trimmed = line.trim().to_string();
            line.zeroize();
            Ok(trimmed)
        }
        Err(e) => {
            line.zeroize();
            Err(format!("Failed to read terminal input: {}", e))
        }
    }
}

/// Reads a standard confirmation prompt (echo enabled).
pub fn read_confirmation_prompt(prompt: &str) -> bool {
    print!("{}", prompt);
    let _ = io::stdout().flush();
    let mut line = String::new();
    if io::stdin().read_line(&mut line).is_err() {
        return false;
    }
    let trimmed = line.trim().to_lowercase();
    trimmed == "y" || trimmed == "yes"
}

// ============================================================================
// Network & Path Helpers
// ============================================================================

pub fn parse_network_and_chain(net_str: Option<&str>) -> Result<(Network, ChainId), String> {
    match net_str.unwrap_or("mainnet").to_lowercase().as_str() {
        "mainnet" | "main" | "9999" => Ok((Network::Mainnet, ChainId::MAINNET)),
        "testnet" | "test" | "102" => Ok((Network::Testnet, ChainId::TESTNET)),
        "devnet" | "dev" | "1337" => Ok((Network::Devnet, ChainId::DEVNET)),
        other => Err(format!(
            "Unknown network '{}'. Allowed networks: mainnet, testnet, devnet",
            other
        )),
    }
}

pub fn default_path_for_network(_network: &Network) -> &'static str {
    CANONICAL_DERIVATION_PATH
}

// ============================================================================
// CLI Subcommands
// ============================================================================

pub fn print_wallet_help() {
    println!(
        "TOLA Network CLI Wallet\n\n\
USAGE:\n\
    tola-cli wallet <SUBCOMMAND> [OPTIONS]\n\n\
SUBCOMMANDS:\n\
    generate       Generate a cryptographically secure BIP-39 mnemonic wallet\n\
    recover        Restore a wallet from an existing recovery phrase securely\n\
    address        Inspect the public address of a wallet\n\
    public-key     Inspect the public key of a wallet\n\
    sign           Sign a transaction locally with clear signing confirmation\n\n\
OPTIONS:\n\
    --words <12|24>           Word count for wallet generation (default: 12)\n\
    --network <NETWORK>       Target network: mainnet, testnet, devnet (default: mainnet)\n\
    --path <DERIVATION_PATH>  Custom BIP-44 derivation path\n\
    --passphrase              Prompt for optional BIP-39 passphrase\n\
    -h, --help                Print wallet command help\n"
    );
}

pub fn cmd_wallet(args: &[String], rpc_url: &str) {
    if args.is_empty() {
        print_wallet_help();
        return;
    }

    match args[0].as_str() {
        "-h" | "--help" => print_wallet_help(),
        "generate" => cmd_wallet_generate(&args[1..]),
        "recover" => cmd_wallet_recover(&args[1..]),
        "address" => cmd_wallet_address(&args[1..]),
        "public-key" => cmd_wallet_public_key(&args[1..]),
        "sign" => cmd_wallet_sign(&args[1..], rpc_url),
        other => {
            eprintln!(
                "Unknown wallet subcommand: '{}'. Run 'tola-cli wallet --help' for available subcommands.",
                other
            );
        }
    }
}

/// `tola-cli wallet generate`
pub fn cmd_wallet_generate(args: &[String]) {
    // 1. Word count validation
    let words_str = get_arg_val(args, "--words").unwrap_or_else(|| "12".to_string());
    let count = match words_str.as_str() {
        "12" => MnemonicCount::Words12,
        "24" => MnemonicCount::Words24,
        other => {
            eprintln!(
                "Error: Invalid word count '{}'. Allowed values are 12 or 24.",
                other
            );
            return;
        }
    };

    // 2. Network selection
    let (network, chain_id) =
        match parse_network_and_chain(get_arg_val(args, "--network").as_deref()) {
            Ok(res) => res,
            Err(e) => {
                eprintln!("Error: {}", e);
                return;
            }
        };

    let path = get_arg_val(args, "--path")
        .unwrap_or_else(|| default_path_for_network(&network).to_string());

    // 3. Optional passphrase
    let mut passphrase = String::new();
    if has_arg(args, "--passphrase") {
        match read_secure_line("BIP-39 passphrase (optional): ") {
            Ok(p) => passphrase = p,
            Err(e) => {
                eprintln!("Error: {}", e);
                return;
            }
        }
    }

    // 4. Cryptographic generation
    let mnemonic = match Mnemonic::generate(count) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("Error generating mnemonic: {:?}", e);
            return;
        }
    };

    let wallet = match TolaWallet::from_mnemonic_with_path(mnemonic.phrase(), &passphrase, &path) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("Error deriving wallet: {}", e);
            return;
        }
    };

    passphrase.zeroize();

    let net_name = match network {
        Network::Mainnet => "Mainnet",
        Network::Testnet => "Testnet",
        Network::Devnet => "Devnet",
        Network::Custom(_) => "Custom",
    };

    // 5. Display exact output format
    println!("TOLA Wallet Created");
    println!("Network: {}", net_name);
    println!("Chain ID: {}", chain_id.0);
    println!("Address:");
    println!("{}", wallet.address_bech32m(&network));
    println!("Recovery Phrase:");
    println!("{}", mnemonic.phrase());
    println!("IMPORTANT:");
    println!("Your recovery phrase controls this wallet.");
    println!("Store it offline.");
    println!("Never share it with anyone.");
}

/// `tola-cli wallet recover`
pub fn cmd_wallet_recover(args: &[String]) {
    // Security check: Never accept recovery phrase as a normal command-line argument!
    if has_arg(args, "--mnemonic")
        || has_arg(args, "--recovery-phrase")
        || has_arg(args, "--phrase")
    {
        eprintln!(
            "Error: For security reasons, the recovery phrase cannot be passed as a command-line argument."
        );
        eprintln!("Run 'tola-cli wallet recover' to enter the phrase securely.");
        return;
    }

    // Network selection
    let (network, _) = match parse_network_and_chain(get_arg_val(args, "--network").as_deref()) {
        Ok(res) => res,
        Err(e) => {
            eprintln!("Error: {}", e);
            return;
        }
    };

    let path = get_arg_val(args, "--path")
        .unwrap_or_else(|| default_path_for_network(&network).to_string());

    // Secure interactive recovery phrase prompt
    let mut phrase_input = match read_secure_line("Recovery phrase: ") {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Error: {}", e);
            return;
        }
    };

    if phrase_input.is_empty() {
        eprintln!("Error: Recovery phrase cannot be empty.");
        return;
    }

    // Optional secure passphrase prompt
    let mut passphrase = match read_secure_line("BIP-39 passphrase (optional): ") {
        Ok(p) => p,
        Err(e) => {
            phrase_input.zeroize();
            eprintln!("Error: {}", e);
            return;
        }
    };

    let mnemonic = match Mnemonic::from_phrase(&phrase_input) {
        Ok(m) => m,
        Err(e) => {
            phrase_input.zeroize();
            passphrase.zeroize();
            eprintln!("Error: Invalid recovery phrase: {:?}", e);
            return;
        }
    };
    phrase_input.zeroize();

    let wallet = match TolaWallet::from_mnemonic_with_path(mnemonic.phrase(), &passphrase, &path) {
        Ok(w) => w,
        Err(e) => {
            passphrase.zeroize();
            eprintln!("Error restoring wallet: {}", e);
            return;
        }
    };
    passphrase.zeroize();

    let net_name = match network {
        Network::Mainnet => "Mainnet",
        Network::Testnet => "Testnet",
        Network::Devnet => "Devnet",
        Network::Custom(_) => "Custom",
    };

    println!("Wallet recovered successfully.");
    println!("Network: {}", net_name);
    println!("Address:");
    println!("{}", wallet.address_bech32m(&network));
}

/// `tola-cli wallet address`
pub fn cmd_wallet_address(args: &[String]) {
    if has_arg(args, "--mnemonic") {
        eprintln!("Error: Recovery phrase cannot be passed as a command-line argument.");
        return;
    }

    let (network, _) = match parse_network_and_chain(get_arg_val(args, "--network").as_deref()) {
        Ok(res) => res,
        Err(e) => {
            eprintln!("Error: {}", e);
            return;
        }
    };

    let path = get_arg_val(args, "--path")
        .unwrap_or_else(|| default_path_for_network(&network).to_string());

    let wallet = if let Some(mut pk_hex) = get_arg_val(args, "--private-key") {
        let clean = pk_hex
            .strip_prefix("0x")
            .or_else(|| pk_hex.strip_prefix("0X"))
            .unwrap_or(&pk_hex);
        let res = TolaWallet::from_hex_private_key(clean);
        pk_hex.zeroize();
        match res {
            Ok(w) => w,
            Err(e) => {
                eprintln!("Error loading private key: {}", e);
                return;
            }
        }
    } else {
        let mut phrase = match read_secure_line("Recovery phrase: ") {
            Ok(p) => p,
            Err(e) => {
                eprintln!("Error: {}", e);
                return;
            }
        };
        let mut passphrase = read_secure_line("BIP-39 passphrase (optional): ").unwrap_or_default();

        let m = match Mnemonic::from_phrase(&phrase) {
            Ok(m) => m,
            Err(e) => {
                phrase.zeroize();
                passphrase.zeroize();
                eprintln!("Error: Invalid recovery phrase: {:?}", e);
                return;
            }
        };
        phrase.zeroize();
        let res = TolaWallet::from_mnemonic_with_path(m.phrase(), &passphrase, &path);
        passphrase.zeroize();
        match res {
            Ok(w) => w,
            Err(e) => {
                eprintln!("Error deriving wallet: {}", e);
                return;
            }
        }
    };

    println!("{}", wallet.address_bech32m(&network));
}

/// `tola-cli wallet public-key`
pub fn cmd_wallet_public_key(args: &[String]) {
    if has_arg(args, "--mnemonic") {
        eprintln!("Error: Recovery phrase cannot be passed as a command-line argument.");
        return;
    }

    let (network, _) = match parse_network_and_chain(get_arg_val(args, "--network").as_deref()) {
        Ok(res) => res,
        Err(e) => {
            eprintln!("Error: {}", e);
            return;
        }
    };

    let path = get_arg_val(args, "--path")
        .unwrap_or_else(|| default_path_for_network(&network).to_string());

    let wallet = if let Some(mut pk_hex) = get_arg_val(args, "--private-key") {
        let clean = pk_hex
            .strip_prefix("0x")
            .or_else(|| pk_hex.strip_prefix("0X"))
            .unwrap_or(&pk_hex);
        let res = TolaWallet::from_hex_private_key(clean);
        pk_hex.zeroize();
        match res {
            Ok(w) => w,
            Err(e) => {
                eprintln!("Error loading private key: {}", e);
                return;
            }
        }
    } else {
        let mut phrase = match read_secure_line("Recovery phrase: ") {
            Ok(p) => p,
            Err(e) => {
                eprintln!("Error: {}", e);
                return;
            }
        };
        let mut passphrase = read_secure_line("BIP-39 passphrase (optional): ").unwrap_or_default();

        let m = match Mnemonic::from_phrase(&phrase) {
            Ok(m) => m,
            Err(e) => {
                phrase.zeroize();
                passphrase.zeroize();
                eprintln!("Error: Invalid recovery phrase: {:?}", e);
                return;
            }
        };
        phrase.zeroize();
        let res = TolaWallet::from_mnemonic_with_path(m.phrase(), &passphrase, &path);
        passphrase.zeroize();
        match res {
            Ok(w) => w,
            Err(e) => {
                eprintln!("Error deriving wallet: {}", e);
                return;
            }
        }
    };

    let pk_bytes = wallet.public_key_bytes();
    let mut s = String::with_capacity(66);
    s.push_str("0x");
    for b in pk_bytes {
        s.push_str(&format!("{b:02x}"));
    }
    println!("{}", s);
}

/// `tola-cli wallet sign`
pub fn cmd_wallet_sign(args: &[String], rpc_url: &str) {
    if has_arg(args, "--mnemonic") {
        eprintln!("Error: Recovery phrase cannot be passed as a command-line argument.");
        return;
    }

    // 1. Destination address
    let to_str = match get_arg_val(args, "--to") {
        Some(s) => s,
        None => {
            eprintln!("Usage: tola-cli wallet sign --to <ADDR> --amount <TOLA> [OPTIONS]");
            return;
        }
    };

    let to_addr = if to_str.starts_with("tola1")
        || to_str.starts_with("ttola1")
        || to_str.starts_with("dtola1")
    {
        match Address::from_bech32m(&to_str) {
            Ok((_, a)) => a,
            Err(e) => {
                eprintln!("Invalid recipient bech32m address: {:?}", e);
                return;
            }
        }
    } else {
        match Address::from_hex(&to_str) {
            Ok(a) => a,
            Err(e) => {
                eprintln!("Invalid recipient address: {:?}", e);
                return;
            }
        }
    };

    // 2. Amount in TOLA
    let amount_tola = match get_arg_val(args, "--amount").and_then(|s| s.parse::<f64>().ok()) {
        Some(a) if a > 0.0 => a,
        _ => {
            eprintln!("Missing or invalid --amount (must be positive number of TOLA)");
            return;
        }
    };
    let amount_base = (amount_tola * (Amount::ONE_TOLA as f64)) as u128;

    // 3. Network & Chain ID
    let (network, chain_id) =
        match parse_network_and_chain(get_arg_val(args, "--network").as_deref()) {
            Ok(res) => res,
            Err(e) => {
                eprintln!("Error: {}", e);
                return;
            }
        };

    let path = get_arg_val(args, "--path")
        .unwrap_or_else(|| default_path_for_network(&network).to_string());

    // 4. Wallet key resolution (local only)
    let wallet = if let Some(mut pk_hex) = get_arg_val(args, "--private-key") {
        let clean = pk_hex
            .strip_prefix("0x")
            .or_else(|| pk_hex.strip_prefix("0X"))
            .unwrap_or(&pk_hex);
        let res = TolaWallet::from_hex_private_key(clean);
        pk_hex.zeroize();
        match res {
            Ok(w) => w,
            Err(e) => {
                eprintln!("Error loading private key: {}", e);
                return;
            }
        }
    } else {
        let mut phrase = match read_secure_line("Recovery phrase: ") {
            Ok(p) => p,
            Err(e) => {
                eprintln!("Error: {}", e);
                return;
            }
        };
        let mut passphrase = read_secure_line("BIP-39 passphrase (optional): ").unwrap_or_default();

        let m = match Mnemonic::from_phrase(&phrase) {
            Ok(m) => m,
            Err(e) => {
                phrase.zeroize();
                passphrase.zeroize();
                eprintln!("Error: Invalid recovery phrase: {:?}", e);
                return;
            }
        };
        phrase.zeroize();
        let res = TolaWallet::from_mnemonic_with_path(m.phrase(), &passphrase, &path);
        passphrase.zeroize();
        match res {
            Ok(w) => w,
            Err(e) => {
                eprintln!("Error deriving wallet: {}", e);
                return;
            }
        }
    };

    let from_addr = wallet.address();

    // 5. Nonce resolution (from flag or RPC)
    let nonce_val =
        if let Some(n) = get_arg_val(args, "--nonce").and_then(|s| s.parse::<u64>().ok()) {
            n
        } else {
            // Query RPC if available
            let nonce_resp = rpc_call_direct(
                rpc_url,
                "eth_getTransactionCount",
                vec![
                    JsonValue::String(from_addr.to_hex()),
                    JsonValue::String("latest".into()),
                ],
            );
            match nonce_resp {
                Ok(v) => parse_hex_u64_str(v.as_str().unwrap_or("0x0")),
                Err(_) => 0,
            }
        };

    // 6. Max Fee
    let max_fee_tola = get_arg_val(args, "--max-fee")
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(0.01);
    let max_fee_per_gas = (max_fee_tola * (Amount::ONE_TOLA as f64) / 21_000.0).max(1.0) as u128;

    let action_str = get_arg_val(args, "--action").unwrap_or_else(|| "Transfer".to_string());

    let memo_opt = get_arg_val(args, "--memo").and_then(|s| {
        let trimmed = s.trim().to_string();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    });

    if let Some(ref m) = memo_opt {
        if m.len() > tola_types::MAX_MEMO_BYTES {
            eprintln!(
                "[ERROR] Memo exceeds maximum length of 256 UTF-8 bytes (got {} bytes)",
                m.len()
            );
            return;
        }
    }

    let net_name = match network {
        Network::Mainnet => "Mainnet",
        Network::Testnet => "Testnet",
        Network::Devnet => "Devnet",
        Network::Custom(_) => "Custom",
    };

    // 7. Clear Signing Display
    println!("=======================================================");
    println!("                   TOLA TRANSACTION                   ");
    println!("=======================================================");
    println!("Network:   {}", net_name);
    println!("Chain ID:  {}", chain_id.0);
    println!("From:      {}", wallet.address_bech32m(&network));
    println!("To:        {}", to_addr.to_bech32m(&network));
    println!("Amount:    {} TOLA", amount_tola);
    println!("Max Fee:   {} TOLA", max_fee_tola);
    println!("Nonce:     {}", nonce_val);
    println!("Action:    {}", action_str);
    if let Some(ref m) = memo_opt {
        println!("Memo:      \"{}\"", m);
    }
    println!("=======================================================");

    // 8. Explicit confirmation (Default NO)
    if !read_confirmation_prompt("Sign this transaction? [y/N]: ") {
        println!("Signing aborted.");
        return;
    }

    // 9. Assemble & sign locally
    let mut tx = Transaction {
        chain_id,
        nonce: Nonce(nonce_val),
        from: from_addr,
        to: Some(to_addr),
        value: Amount(amount_base),
        gas_limit: Gas(21_000),
        max_fee_per_gas: Amount(max_fee_per_gas),
        max_priority_fee_per_gas: Amount(max_fee_per_gas),
        valid_until_block: BlockHeight(1_000_000),
        data: vec![],
        memo: memo_opt,
        signature: Signature::Secp256k1 {
            r: [0u8; 32],
            s: [0u8; 32],
            v: 0,
        },
        tx_type: 0,
        access_list: vec![],
    };

    if let Err(e) = wallet.sign_transaction(&mut tx) {
        eprintln!("[ERROR] Signing failed: {}", e);
        return;
    }

    let wire_bytes = tx.to_canonical_bytes();
    let tx_hash = tola_crypto::blake3_hash(&wire_bytes);
    let mut raw_hex = String::with_capacity(wire_bytes.len() * 2 + 2);
    raw_hex.push_str("0x");
    for b in &wire_bytes {
        raw_hex.push_str(&format!("{b:02x}"));
    }

    println!("[SUCCESS] Transaction signed locally!");
    println!("Transaction Hash: {}", tx_hash.to_hex());
    println!("Signed Transaction (Hex):");
    println!("{}", raw_hex);

    // 10. Optional broadcast
    if has_arg(args, "--broadcast") {
        println!("Broadcasting transaction to {}...", rpc_url);
        match rpc_call_direct(
            rpc_url,
            "eth_sendRawTransaction",
            vec![JsonValue::String(raw_hex)],
        ) {
            Ok(res) => {
                println!("[SUCCESS] Broadcast accepted by node!");
                println!("RPC Transaction Hash: {}", res.as_str().unwrap_or(""));
            }
            Err(e) => {
                eprintln!("[ERROR] Broadcast failed: {}", e);
            }
        }
    }
}

// ============================================================================
// Internal Helpers
// ============================================================================

fn get_arg_val(args: &[String], key: &str) -> Option<String> {
    for i in 0..args.len() {
        if args[i] == key && i + 1 < args.len() {
            return Some(args[i + 1].clone());
        }
    }
    None
}

fn has_arg(args: &[String], key: &str) -> bool {
    args.iter().any(|a| a == key)
}

fn parse_hex_u64_str(s: &str) -> u64 {
    let clean = s
        .strip_prefix("0x")
        .or_else(|| s.strip_prefix("0X"))
        .unwrap_or(s);
    u64::from_str_radix(clean, 16).unwrap_or(0)
}

fn rpc_call_direct(
    rpc_url: &str,
    method: &str,
    params: Vec<JsonValue>,
) -> Result<JsonValue, String> {
    use std::net::TcpStream;
    use std::time::Duration;

    let clean = rpc_url.strip_prefix("http://").unwrap_or(rpc_url);
    let mut stream = TcpStream::connect(clean)
        .map_err(|e| format!("Failed to connect to RPC node at {}: {}", clean, e))?;

    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));

    let req_obj = JsonValue::Object(vec![
        ("jsonrpc".into(), JsonValue::String("2.0".into())),
        ("method".into(), JsonValue::String(method.into())),
        ("params".into(), JsonValue::Array(params)),
        ("id".into(), JsonValue::Integer(1)),
    ]);
    let req_body = req_obj.serialize();

    let http_req = format!(
        "POST / HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        clean,
        req_body.len(),
        req_body
    );

    stream
        .write_all(http_req.as_bytes())
        .map_err(|e| format!("Failed to write to RPC stream: {}", e))?;
    stream.flush().map_err(|e| format!("Flush error: {}", e))?;

    let mut response_bytes = Vec::new();
    let _ = stream.read_to_end(&mut response_bytes);

    let resp_str = String::from_utf8_lossy(&response_bytes);
    let body = if let Some(pos) = resp_str.find("\r\n\r\n") {
        &resp_str[pos + 4..]
    } else if let Some(pos) = resp_str.find("\n\n") {
        &resp_str[pos + 2..]
    } else {
        return Err("Malformed HTTP response".to_string());
    };

    let parsed = JsonValue::parse(body).map_err(|e| format!("JSON parse error: {}", e))?;
    if let Some(err) = parsed.get("error") {
        return Err(format!("RPC Error: {}", err.serialize()));
    }

    parsed
        .get("result")
        .cloned()
        .ok_or_else(|| "Missing result in response".to_string())
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_canonical_test_vector_1_recovery() {
        let phrase = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
        let mnemonic = Mnemonic::from_phrase(phrase).expect("Valid mnemonic");
        let wallet =
            TolaWallet::from_mnemonic(mnemonic.phrase(), "", &Network::Mainnet).expect("Derive");

        assert_eq!(
            wallet.address().to_hex(),
            "0x3b45664e195dbacd0035046b511491e09bd3ca8a"
        );
        assert_eq!(
            wallet.address_bech32m(&Network::Mainnet),
            "tola18dzkvnsetkav6qp4q344z9y3uzda8j5263ez6j"
        );
    }

    #[test]
    fn test_network_hrp_mapping() {
        let (net_main, chain_main) = parse_network_and_chain(Some("mainnet")).unwrap();
        assert_eq!(net_main.hrp(), "tola");
        assert_eq!(chain_main, ChainId(9999));

        let (net_test, chain_test) = parse_network_and_chain(Some("testnet")).unwrap();
        assert_eq!(net_test.hrp(), "ttola");
        assert_eq!(chain_test, ChainId(102));

        let (net_dev, chain_dev) = parse_network_and_chain(Some("devnet")).unwrap();
        assert_eq!(net_dev.hrp(), "dtola");
        assert_eq!(chain_dev, ChainId(1337));
    }

    #[test]
    fn test_wallet_generation_and_determinism() {
        let phrase = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
        let m1 = Mnemonic::from_phrase(phrase).unwrap();
        let w1 = TolaWallet::from_mnemonic(m1.phrase(), "", &Network::Mainnet).unwrap();

        let m2 = Mnemonic::from_phrase(phrase).unwrap();
        let w2 = TolaWallet::from_mnemonic(m2.phrase(), "", &Network::Mainnet).unwrap();

        assert_eq!(w1.address(), w2.address());
        assert_eq!(w1.public_key_bytes(), w2.public_key_bytes());
    }

    #[test]
    fn test_invalid_mnemonic_rejections() {
        // Invalid word
        let invalid_word = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon foobarxyz";
        assert!(Mnemonic::from_phrase(invalid_word).is_err());

        // Invalid checksum
        let invalid_checksum = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon";
        assert!(Mnemonic::from_phrase(invalid_checksum).is_err());

        // Wrong word count
        let wrong_count = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon";
        assert!(Mnemonic::from_phrase(wrong_count).is_err());
    }

    #[test]
    fn test_local_transaction_signing_and_verification() {
        let phrase = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
        let mnemonic = Mnemonic::from_phrase(phrase).unwrap();
        let wallet = TolaWallet::from_mnemonic(mnemonic.phrase(), "", &Network::Mainnet).unwrap();
        let addr = wallet.address();

        let recipient = Address([0x77; 20]);
        let mut tx = Transaction {
            chain_id: ChainId::MAINNET,
            nonce: Nonce(10),
            from: addr,
            to: Some(recipient),
            value: Amount(100_000_000_000_000_000_000), // 100 TOLA
            gas_limit: Gas(21_000),
            max_fee_per_gas: Amount(1_000_000_000),
            max_priority_fee_per_gas: Amount(1_000_000_000),
            valid_until_block: BlockHeight(500),
            data: vec![],
            memo: None,
            signature: Signature::Secp256k1 {
                r: [0u8; 32],
                s: [0u8; 32],
                v: 0,
            },
            tx_type: 0,
            access_list: vec![],
        };

        wallet.sign_transaction(&mut tx).expect("Sign locally");

        // Verify signature recovers originating address
        let mut unsigned_bytes = Vec::new();
        tola_codec::serialize_unsigned_transaction(&tx, &mut unsigned_bytes);
        let payload_hash = tola_crypto::blake3_hash(&unsigned_bytes);
        let digest = tola_crypto::domain_separated_hash(
            tola_crypto::DOMAIN_TRANSACTION_PAYLOAD,
            tx.chain_id.0,
            0,
            &payload_hash,
        );

        match tx.signature {
            Signature::Secp256k1 { r, s, v } => {
                let mut sig_65 = [0u8; 65];
                sig_65[0..32].copy_from_slice(&r);
                sig_65[32..64].copy_from_slice(&s);
                sig_65[64] = v;
                let recovered = tola_crypto::recover_secp256k1_address(digest.as_bytes(), &sig_65)
                    .expect("Recover address");
                assert_eq!(recovered, addr);
                assert_eq!(recovered, tx.from);
            }
            _ => panic!("Expected Secp256k1 signature"),
        }
    }

    #[test]
    fn test_generation_12_and_24_words() {
        let (wallet12, m12) = TolaWallet::generate_with_mnemonic(MnemonicCount::Words12).unwrap();
        assert_eq!(m12.word_count(), 12);
        let restored12 = TolaWallet::from_mnemonic(m12.phrase(), "", &Network::Mainnet).unwrap();
        assert_eq!(wallet12.address(), restored12.address());

        let (wallet24, m24) = TolaWallet::generate_with_mnemonic(MnemonicCount::Words24).unwrap();
        assert_eq!(m24.word_count(), 24);
        let restored24 = TolaWallet::from_mnemonic(m24.phrase(), "", &Network::Mainnet).unwrap();
        assert_eq!(wallet24.address(), restored24.address());
    }

    #[test]
    fn test_canonical_test_vector_2_24_words() {
        let phrase24 = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon art";
        let m24 = Mnemonic::from_phrase(phrase24).expect("Valid 24 words");
        assert_eq!(m24.word_count(), 24);

        let w_mainnet = TolaWallet::from_mnemonic(m24.phrase(), "", &Network::Mainnet).unwrap();
        assert_eq!(
            w_mainnet.address().to_hex(),
            "0x37061e043e1b89972ed615c46ff3e5090514c909"
        );
        assert_eq!(
            w_mainnet.address_bech32m(&Network::Mainnet),
            "tola1xurpupp7rwyewtkkzhzxlul9pyz3fjgf4wludn"
        );

        // Model A: Testnet uses identical keypair and raw address
        let w_testnet = TolaWallet::from_mnemonic(m24.phrase(), "", &Network::Testnet).unwrap();
        assert_eq!(
            w_testnet.address().to_hex(),
            "0x37061e043e1b89972ed615c46ff3e5090514c909"
        );
        assert_eq!(
            w_testnet.address_bech32m(&Network::Mainnet),
            "tola1xurpupp7rwyewtkkzhzxlul9pyz3fjgf4wludn"
        );

        // Explicit legacy path verification
        let w_legacy = TolaWallet::from_mnemonic_legacy_testnet(m24.phrase(), "").unwrap();
        assert_eq!(
            w_legacy.address().to_hex(),
            "0x1073f501701b4a14dd4bc6a1f1746fca1fb8de64"
        );
    }

    #[test]
    fn test_secrets_redaction() {
        let phrase = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
        let mnemonic = Mnemonic::from_phrase(phrase).unwrap();
        let debug_str = format!("{:?}", mnemonic);
        assert_eq!(debug_str, "Mnemonic([REDACTED])");
        assert!(!debug_str.contains("abandon"));

        let seed = mnemonic.to_seed("");
        let debug_seed = format!("{:?}", seed);
        assert_eq!(debug_seed, "Seed([REDACTED])");
    }
}
