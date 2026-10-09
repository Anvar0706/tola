use std::io::Write;
use std::process::{Command, Stdio};

fn tola_cli_bin() -> &'static str {
    env!("CARGO_BIN_EXE_tola-cli")
}

#[test]
fn test_cli_wallet_generate_default_12_words() {
    let output = Command::new(tola_cli_bin())
        .args(["wallet", "generate"])
        .output()
        .expect("Failed to execute tola-cli");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(stdout.contains("TOLA Wallet Created"));
    assert!(stdout.contains("Network: Mainnet"));
    assert!(stdout.contains("Chain ID: 9999"));
    assert!(stdout.contains("Address:"));
    assert!(stdout.contains("tola1"));
    assert!(stdout.contains("Recovery Phrase:"));
    assert!(stdout.contains("IMPORTANT:"));
    assert!(stdout.contains("Store it offline."));

    // Ensure private key is NOT emitted
    assert!(!stdout.contains("Private Key"));
    assert!(!stdout.contains("private_key"));

    // Check 12 words in phrase
    let lines: Vec<&str> = stdout.lines().collect();
    let phrase_idx = lines
        .iter()
        .position(|l| l.contains("Recovery Phrase:"))
        .unwrap();
    let phrase_line = lines[phrase_idx + 1].trim();
    let words: Vec<&str> = phrase_line.split_whitespace().collect();
    assert_eq!(words.len(), 12);
}

#[test]
fn test_cli_wallet_generate_24_words() {
    let output = Command::new(tola_cli_bin())
        .args(["wallet", "generate", "--words", "24"])
        .output()
        .expect("Failed to execute tola-cli");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(stdout.contains("TOLA Wallet Created"));
    let lines: Vec<&str> = stdout.lines().collect();
    let phrase_idx = lines
        .iter()
        .position(|l| l.contains("Recovery Phrase:"))
        .unwrap();
    let phrase_line = lines[phrase_idx + 1].trim();
    let words: Vec<&str> = phrase_line.split_whitespace().collect();
    assert_eq!(words.len(), 24);
}

#[test]
fn test_cli_wallet_generate_invalid_words_fails() {
    let output = Command::new(tola_cli_bin())
        .args(["wallet", "generate", "--words", "15"])
        .output()
        .expect("Failed to execute tola-cli");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Allowed values are 12 or 24"));
}

#[test]
fn test_cli_wallet_generate_network_selection() {
    // Testnet
    let out_testnet = Command::new(tola_cli_bin())
        .args(["wallet", "generate", "--network", "testnet"])
        .output()
        .expect("Failed to execute tola-cli");

    assert!(out_testnet.status.success());
    let stdout_test = String::from_utf8_lossy(&out_testnet.stdout);
    assert!(stdout_test.contains("Network: Testnet"));
    assert!(stdout_test.contains("Chain ID: 102"));
    assert!(stdout_test.contains("ttola1"));

    // Devnet
    let out_devnet = Command::new(tola_cli_bin())
        .args(["wallet", "generate", "--network", "devnet"])
        .output()
        .expect("Failed to execute tola-cli");

    assert!(out_devnet.status.success());
    let stdout_dev = String::from_utf8_lossy(&out_devnet.stdout);
    assert!(stdout_dev.contains("Network: Devnet"));
    assert!(stdout_dev.contains("Chain ID: 1337"));
    assert!(stdout_dev.contains("dtola1"));
}

#[test]
fn test_cli_wallet_recover_rejects_mnemonic_flag() {
    let output = Command::new(tola_cli_bin())
        .args([
            "wallet",
            "recover",
            "--mnemonic",
            "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about",
        ])
        .output()
        .expect("Failed to execute tola-cli");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains(
        "For security reasons, the recovery phrase cannot be passed as a command-line argument"
    ));
}

#[test]
fn test_cli_wallet_recover_canonical_vector_piped_stdin() {
    let phrase = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about\n\n";

    let mut child = Command::new(tola_cli_bin())
        .args(["wallet", "recover", "--network", "mainnet"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Spawn tola-cli");

    {
        let stdin = child.stdin.as_mut().expect("Open stdin");
        stdin.write_all(phrase.as_bytes()).expect("Write phrase");
    }

    let output = child.wait_with_output().expect("Wait for child");
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Wallet recovered successfully."));
    assert!(stdout.contains("Network: Mainnet"));
    assert!(stdout.contains("Address:"));
    // Canonical Address for Vector 1
    assert!(stdout.contains("tola18dzkvnsetkav6qp4q344z9y3uzda8j5263ez6j"));

    // Ensure secrets are NOT printed back
    assert!(!stdout.contains("abandon"));
    assert!(!stdout.contains("5eb00bbddcf069084889a8ab9155568165f5c453ccb85e70811aaed6f6da5fc19a5ac40b389cd370d086206dec8aa6c43daea6690f20ad3d8d48b2d2ce9e38e4"));
}

#[test]
fn test_cli_wallet_address_piped_stdin() {
    let phrase = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about\n\n";

    let mut child = Command::new(tola_cli_bin())
        .args(["wallet", "address", "--network", "mainnet"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Spawn tola-cli");

    {
        let stdin = child.stdin.as_mut().expect("Open stdin");
        stdin.write_all(phrase.as_bytes()).expect("Write phrase");
    }

    let output = child.wait_with_output().expect("Wait for child");
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("tola18dzkvnsetkav6qp4q344z9y3uzda8j5263ez6j"));
    // Ensure no private key or mnemonic
    assert!(!stdout.contains("abandon"));
    assert!(!stdout.contains("Private Key"));
}

#[test]
fn test_cli_wallet_public_key_piped_stdin() {
    let phrase = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about\n\n";

    let mut child = Command::new(tola_cli_bin())
        .args(["wallet", "public-key", "--network", "mainnet"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Spawn tola-cli");

    {
        let stdin = child.stdin.as_mut().expect("Open stdin");
        stdin.write_all(phrase.as_bytes()).expect("Write phrase");
    }

    let output = child.wait_with_output().expect("Wait for child");
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    // Canonical compressed public key for Vector 1
    assert!(stdout.contains("0x03e95745eb5f028b4e23fb4c5af97fb49426d522c4228fe6dd922f2585e6b1bf96"));
    assert!(!stdout.contains("abandon"));
}

#[test]
fn test_cli_wallet_sign_clear_signing_abort_on_default() {
    // Provide phrase, empty passphrase, then empty input for confirmation (default NO)
    let input = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about\n\n\n";

    let mut child = Command::new(tola_cli_bin())
        .args([
            "wallet",
            "sign",
            "--to",
            "tola18dzkvnsetkav6qp4q344z9y3uzda8j5263ez6j",
            "--amount",
            "100",
            "--nonce",
            "42",
            "--network",
            "mainnet",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Spawn tola-cli");

    {
        let stdin = child.stdin.as_mut().expect("Open stdin");
        stdin.write_all(input.as_bytes()).expect("Write input");
    }

    let output = child.wait_with_output().expect("Wait for child");
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Verify clear signing details
    assert!(stdout.contains("TOLA TRANSACTION"));
    assert!(stdout.contains("Network:   Mainnet"));
    assert!(stdout.contains("Chain ID:  9999"));
    assert!(stdout.contains("Amount:    100 TOLA"));
    assert!(stdout.contains("Nonce:     42"));
    assert!(stdout.contains("Action:    Transfer"));
    assert!(stdout.contains("Sign this transaction? [y/N]:"));
    assert!(stdout.contains("Signing aborted."));
    assert!(!stdout.contains("[SUCCESS] Transaction signed"));
}

#[test]
fn test_cli_wallet_sign_clear_signing_confirm() {
    // Provide phrase, empty passphrase, then explicit confirmation 'y'
    let input = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about\n\ny\n";

    let mut child = Command::new(tola_cli_bin())
        .args([
            "wallet",
            "sign",
            "--to",
            "tola18dzkvnsetkav6qp4q344z9y3uzda8j5263ez6j",
            "--amount",
            "100",
            "--nonce",
            "42",
            "--network",
            "mainnet",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Spawn tola-cli");

    {
        let stdin = child.stdin.as_mut().expect("Open stdin");
        stdin.write_all(input.as_bytes()).expect("Write input");
    }

    let output = child.wait_with_output().expect("Wait for child");
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("[SUCCESS] Transaction signed locally!"));
    assert!(stdout.contains("Transaction Hash: 0x"));
    assert!(stdout.contains("Signed Transaction (Hex):"));
    assert!(stdout.contains("0x"));
}

#[test]
fn test_backward_compatibility_account_new() {
    let output = Command::new(tola_cli_bin())
        .args(["account", "new"])
        .output()
        .expect("Failed to execute tola-cli");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("NEW TOLA ACCOUNT GENERATED"));
    assert!(stdout.contains("Address (Hex):"));
    assert!(stdout.contains("Address (Testnet):"));
    assert!(stdout.contains("Address (Mainnet):"));
    assert!(stdout.contains("Public Key (Hex):"));
    assert!(stdout.contains("IMPORTANT: Store your private key securely!"));
}

#[test]
fn test_cli_wallet_sign_with_memo_confirm() {
    let input = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about\n\ny\n";

    let mut child = Command::new(tola_cli_bin())
        .args([
            "wallet",
            "sign",
            "--to",
            "tola18dzkvnsetkav6qp4q344z9y3uzda8j5263ez6j",
            "--amount",
            "50",
            "--nonce",
            "10",
            "--network",
            "mainnet",
            "--memo",
            "Invoice #1042",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Spawn tola-cli");

    {
        let stdin = child.stdin.as_mut().expect("Open stdin");
        stdin.write_all(input.as_bytes()).expect("Write input");
    }

    let output = child.wait_with_output().expect("Wait for child");
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Memo:      \"Invoice #1042\""));
    assert!(stdout.contains("[SUCCESS] Transaction signed locally!"));
    assert!(stdout.contains("Transaction Hash: 0x"));
}

#[test]
fn test_cli_wallet_sign_with_oversized_memo_fails() {
    let oversized_memo = "A".repeat(257);
    let input = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about\n\n";

    let mut child = Command::new(tola_cli_bin())
        .args([
            "wallet",
            "sign",
            "--to",
            "tola18dzkvnsetkav6qp4q344z9y3uzda8j5263ez6j",
            "--amount",
            "50",
            "--nonce",
            "10",
            "--network",
            "mainnet",
            "--memo",
            &oversized_memo,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Spawn tola-cli");

    {
        let stdin = child.stdin.as_mut().expect("Open stdin");
        stdin.write_all(input.as_bytes()).expect("Write input");
    }

    let output = child.wait_with_output().expect("Wait for child");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Memo exceeds maximum length of 256 UTF-8 bytes"));
}
