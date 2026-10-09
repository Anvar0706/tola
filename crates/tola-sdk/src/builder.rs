//! Fluent transaction builder API for constructing and signing transactions.

use tola_types::*;

use crate::wallet::TolaWallet;

pub struct TransactionBuilder {
    chain_id: ChainId,
    from: Option<Address>,
    to: Option<Address>,
    value: Amount,
    nonce: Nonce,
    gas_limit: Gas,
    max_fee_per_gas: Amount,
    max_priority_fee_per_gas: Amount,
    valid_until_block: BlockHeight,
    data: Vec<u8>,
    memo: Option<String>,
}

impl TransactionBuilder {
    pub fn new(chain_id: ChainId) -> Self {
        Self {
            chain_id,
            from: None,
            to: None,
            value: Amount::ZERO,
            nonce: Nonce(0),
            gas_limit: Gas(21_000),
            max_fee_per_gas: Amount::from_u128(1_000_000_000), // 1 Gwei
            max_priority_fee_per_gas: Amount::from_u128(1_000_000_000), // 1 Gwei
            valid_until_block: BlockHeight(1_000_000),
            data: Vec::new(),
            memo: None,
        }
    }

    pub fn from(mut self, from: Address) -> Self {
        self.from = Some(from);
        self
    }

    pub fn to(mut self, to: Address) -> Self {
        self.to = Some(to);
        self
    }

    pub fn value(mut self, value: Amount) -> Self {
        self.value = value;
        self
    }

    pub fn nonce(mut self, nonce: Nonce) -> Self {
        self.nonce = nonce;
        self
    }

    pub fn gas_limit(mut self, gas_limit: Gas) -> Self {
        self.gas_limit = gas_limit;
        self
    }

    pub fn max_fee_per_gas(mut self, fee: Amount) -> Self {
        self.max_fee_per_gas = fee;
        self
    }

    pub fn max_priority_fee_per_gas(mut self, fee: Amount) -> Self {
        self.max_priority_fee_per_gas = fee;
        self
    }

    pub fn valid_until_block(mut self, height: BlockHeight) -> Self {
        self.valid_until_block = height;
        self
    }

    pub fn data(mut self, data: Vec<u8>) -> Self {
        self.data = data;
        self
    }

    pub fn memo(mut self, memo: impl Into<String>) -> Self {
        let m = memo.into();
        self.memo = if m.is_empty() { None } else { Some(m) };
        self
    }

    pub fn with_memo(mut self, memo: Option<String>) -> Self {
        self.memo = match memo {
            Some(m) if !m.is_empty() => Some(m),
            _ => None,
        };
        self
    }

    pub fn build_unsigned(self) -> Result<Transaction, String> {
        let from = self
            .from
            .ok_or_else(|| "Missing 'from' address".to_string())?;

        if let Some(ref m) = self.memo {
            if m.len() > MAX_MEMO_BYTES {
                return Err(format!(
                    "Memo length {} bytes exceeds maximum allowed {} bytes",
                    m.len(),
                    MAX_MEMO_BYTES
                ));
            }
        }

        Ok(Transaction {
            chain_id: self.chain_id,
            nonce: self.nonce,
            from,
            to: self.to,
            value: self.value,
            gas_limit: self.gas_limit,
            max_fee_per_gas: self.max_fee_per_gas,
            max_priority_fee_per_gas: self.max_priority_fee_per_gas,
            valid_until_block: self.valid_until_block,
            data: self.data,
            memo: self.memo,
            signature: Signature::Secp256k1 {
                r: [0u8; 32],
                s: [0u8; 32],
                v: 0,
            },
            tx_type: 0,
            access_list: vec![],
        })
    }

    pub fn build_and_sign(mut self, wallet: &TolaWallet) -> Result<Transaction, String> {
        self.from = Some(wallet.address());
        let mut tx = self.build_unsigned()?;
        wallet.sign_transaction(&mut tx)?;
        Ok(tx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transaction_builder_and_signing() {
        let wallet = TolaWallet::generate();
        let bob = Address([0xbb; 20]);

        let tx = TransactionBuilder::new(ChainId(102))
            .to(bob)
            .value(Amount(500_000_000))
            .nonce(Nonce(1))
            .memo("Invoice #1042")
            .build_and_sign(&wallet)
            .expect("Build and sign tx");

        assert_eq!(tx.from, wallet.address());
        assert_eq!(tx.to, Some(bob));
        assert_eq!(tx.value, Amount(500_000_000));
        assert_eq!(tx.nonce, Nonce(1));
        assert_eq!(tx.memo, Some("Invoice #1042".to_string()));
        match tx.signature {
            Signature::Secp256k1 { v, .. } => assert!(v == 0 || v == 1),
            _ => panic!("Expected Secp256k1 signature"),
        }
    }

    #[test]
    fn test_transaction_builder_memo_limits() {
        let wallet = TolaWallet::generate();
        let bob = Address([0xbb; 20]);

        // Empty string normalizes to None
        let tx = TransactionBuilder::new(ChainId(102))
            .to(bob)
            .memo("")
            .build_and_sign(&wallet)
            .unwrap();
        assert_eq!(tx.memo, None);

        // Max 256 bytes allowed
        let exact_256 = "x".repeat(256);
        let tx256 = TransactionBuilder::new(ChainId(102))
            .to(bob)
            .memo(&exact_256)
            .build_and_sign(&wallet)
            .unwrap();
        assert_eq!(tx256.memo.as_deref(), Some(exact_256.as_str()));

        // 257 bytes rejected
        let too_long = "x".repeat(257);
        let err = TransactionBuilder::new(ChainId(102))
            .to(bob)
            .memo(&too_long)
            .build_and_sign(&wallet);
        assert!(err.is_err());
    }
}
