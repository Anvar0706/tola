//! Lightweight, deterministic Recursive Length Prefix (RLP) codec.
//!
//! Complies with Ethereum Yellow Paper Appendix B and EIP-2718 / EIP-1559 envelopes.
//! Zero external dependencies; fully auditable.

use crate::CodecError;

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum RlpItem<'a> {
    Bytes(&'a [u8]),
    List(Vec<RlpItem<'a>>),
}

/// Encode raw byte string into RLP.
pub fn encode_bytes(data: &[u8]) -> Vec<u8> {
    if data.len() == 1 && data[0] < 0x80 {
        vec![data[0]]
    } else if data.len() <= 55 {
        let mut out = Vec::with_capacity(1 + data.len());
        out.push(0x80 + data.len() as u8);
        out.extend_from_slice(data);
        out
    } else {
        let len_bytes = encode_length(data.len());
        let mut out = Vec::with_capacity(1 + len_bytes.len() + data.len());
        out.push(0xb7 + len_bytes.len() as u8);
        out.extend_from_slice(&len_bytes);
        out.extend_from_slice(data);
        out
    }
}

/// Encode list of encoded RLP elements into an RLP list.
pub fn encode_list(items: &[&[u8]]) -> Vec<u8> {
    let payload_len: usize = items.iter().map(|item| item.len()).sum();
    if payload_len <= 55 {
        let mut out = Vec::with_capacity(1 + payload_len);
        out.push(0xc0 + payload_len as u8);
        for item in items {
            out.extend_from_slice(item);
        }
        out
    } else {
        let len_bytes = encode_length(payload_len);
        let mut out = Vec::with_capacity(1 + len_bytes.len() + payload_len);
        out.push(0xf7 + len_bytes.len() as u8);
        out.extend_from_slice(&len_bytes);
        for item in items {
            out.extend_from_slice(item);
        }
        out
    }
}

fn encode_length(len: usize) -> Vec<u8> {
    let bytes = len.to_be_bytes();
    let start = bytes
        .iter()
        .position(|&b| b != 0)
        .unwrap_or(bytes.len() - 1);
    bytes[start..].to_vec()
}

/// Parse top-level RLP item from byte slice.
pub fn decode_rlp<'a>(input: &'a [u8]) -> Result<(RlpItem<'a>, &'a [u8]), CodecError> {
    if input.is_empty() {
        return Err(CodecError::UnexpectedEof);
    }

    let prefix = input[0];
    if prefix < 0x80 {
        // Single byte
        Ok((RlpItem::Bytes(&input[..1]), &input[1..]))
    } else if prefix <= 0xb7 {
        // Short string (0 - 55 bytes)
        let len = (prefix - 0x80) as usize;
        if input.len() < 1 + len {
            return Err(CodecError::UnexpectedEof);
        }
        let data = &input[1..1 + len];
        if len == 1 && data[0] < 0x80 {
            return Err(CodecError::InvalidEncoding(
                "Non-canonical single byte RLP string".into(),
            ));
        }
        Ok((RlpItem::Bytes(data), &input[1 + len..]))
    } else if prefix <= 0xbf {
        // Long string
        let len_of_len = (prefix - 0xb7) as usize;
        if input.len() < 1 + len_of_len {
            return Err(CodecError::UnexpectedEof);
        }
        let len = parse_be_usize(&input[1..1 + len_of_len])?;
        if len <= 55 {
            return Err(CodecError::InvalidEncoding(
                "Non-canonical long string length <= 55".into(),
            ));
        }
        let start = 1 + len_of_len;
        if input.len() < start + len {
            return Err(CodecError::UnexpectedEof);
        }
        Ok((
            RlpItem::Bytes(&input[start..start + len]),
            &input[start + len..],
        ))
    } else if prefix <= 0xf7 {
        // Short list (0 - 55 bytes payload)
        let payload_len = (prefix - 0xc0) as usize;
        if input.len() < 1 + payload_len {
            return Err(CodecError::UnexpectedEof);
        }
        let mut slice = &input[1..1 + payload_len];
        let mut list = Vec::new();
        while !slice.is_empty() {
            let (item, rest) = decode_rlp(slice)?;
            list.push(item);
            slice = rest;
        }
        Ok((RlpItem::List(list), &input[1 + payload_len..]))
    } else {
        // Long list
        let len_of_len = (prefix - 0xf7) as usize;
        if input.len() < 1 + len_of_len {
            return Err(CodecError::UnexpectedEof);
        }
        let payload_len = parse_be_usize(&input[1..1 + len_of_len])?;
        if payload_len <= 55 {
            return Err(CodecError::InvalidEncoding(
                "Non-canonical long list payload <= 55".into(),
            ));
        }
        let start = 1 + len_of_len;
        if input.len() < start + payload_len {
            return Err(CodecError::UnexpectedEof);
        }
        let mut slice = &input[start..start + payload_len];
        let mut list = Vec::new();
        while !slice.is_empty() {
            let (item, rest) = decode_rlp(slice)?;
            list.push(item);
            slice = rest;
        }
        Ok((RlpItem::List(list), &input[start + payload_len..]))
    }
}

fn parse_be_usize(bytes: &[u8]) -> Result<usize, CodecError> {
    if bytes.is_empty() || (bytes.len() > 1 && bytes[0] == 0) {
        return Err(CodecError::InvalidEncoding(
            "Non-canonical leading zero in length".into(),
        ));
    }
    let mut val: usize = 0;
    for &b in bytes {
        val = val
            .checked_shl(8)
            .ok_or_else(|| CodecError::InvalidEncoding("Length overflow".into()))?
            | (b as usize);
    }
    Ok(val)
}

pub fn encode_u64(val: u64) -> Vec<u8> {
    if val == 0 {
        encode_bytes(&[])
    } else {
        let bytes = val.to_be_bytes();
        let start = bytes.iter().position(|&b| b != 0).unwrap_or(bytes.len());
        encode_bytes(&bytes[start..])
    }
}

pub fn encode_u128(val: u128) -> Vec<u8> {
    if val == 0 {
        encode_bytes(&[])
    } else {
        let bytes = val.to_be_bytes();
        let start = bytes.iter().position(|&b| b != 0).unwrap_or(bytes.len());
        encode_bytes(&bytes[start..])
    }
}

pub fn encode_addr(addr: Option<tola_types::Address>) -> Vec<u8> {
    match addr {
        Some(a) => encode_bytes(&a.0),
        None => encode_bytes(&[]),
    }
}

pub fn decode_u64(item: &RlpItem) -> Result<u64, CodecError> {
    match item {
        RlpItem::Bytes(b) => {
            if b.is_empty() {
                return Ok(0);
            }
            if b.len() > 8 {
                return Err(CodecError::InvalidEncoding("u64 overflow".into()));
            }
            let mut arr = [0u8; 8];
            arr[8 - b.len()..].copy_from_slice(b);
            Ok(u64::from_be_bytes(arr))
        }
        _ => Err(CodecError::InvalidEncoding("Expected bytes for u64".into())),
    }
}

pub fn decode_u128(item: &RlpItem) -> Result<u128, CodecError> {
    match item {
        RlpItem::Bytes(b) => {
            if b.is_empty() {
                return Ok(0);
            }
            if b.len() > 16 {
                return Err(CodecError::InvalidEncoding("u128 overflow".into()));
            }
            let mut arr = [0u8; 16];
            arr[16 - b.len()..].copy_from_slice(b);
            Ok(u128::from_be_bytes(arr))
        }
        _ => Err(CodecError::InvalidEncoding(
            "Expected bytes for u128".into(),
        )),
    }
}

pub fn decode_bytes32(item: &RlpItem) -> Result<[u8; 32], CodecError> {
    match item {
        RlpItem::Bytes(b) => {
            if b.len() > 32 {
                return Err(CodecError::InvalidEncoding("bytes32 overflow".into()));
            }
            let mut arr = [0u8; 32];
            arr[32 - b.len()..].copy_from_slice(b);
            Ok(arr)
        }
        _ => Err(CodecError::InvalidEncoding(
            "Expected bytes for bytes32".into(),
        )),
    }
}

pub fn decode_address(item: &RlpItem) -> Result<Option<tola_types::Address>, CodecError> {
    match item {
        RlpItem::Bytes(b) => {
            if b.is_empty() {
                Ok(None)
            } else if b.len() == 20 {
                let mut arr = [0u8; 20];
                arr.copy_from_slice(b);
                Ok(Some(tola_types::Address(arr)))
            } else {
                Err(CodecError::InvalidEncoding("Invalid address length".into()))
            }
        }
        _ => Err(CodecError::InvalidEncoding(
            "Expected bytes for address".into(),
        )),
    }
}

/// Encode an access list into RLP format.
pub fn encode_access_list(list: &[tola_types::AccessListItem]) -> Vec<u8> {
    let mut encoded_entries = Vec::with_capacity(list.len());
    for item in list {
        let addr_enc = encode_bytes(&item.address.0);
        let encoded_keys: Vec<Vec<u8>> = item
            .storage_keys
            .iter()
            .map(|k| encode_bytes(&k.0))
            .collect();
        let key_refs: Vec<&[u8]> = encoded_keys.iter().map(|k| k.as_slice()).collect();
        let keys_enc = encode_list(&key_refs);

        let tuple_enc = encode_list(&[&addr_enc, &keys_enc]);
        encoded_entries.push(tuple_enc);
    }
    let entry_refs: Vec<&[u8]> = encoded_entries.iter().map(|e| e.as_slice()).collect();
    encode_list(&entry_refs)
}

/// Decode an access list from an RLP item.
pub fn decode_access_list(item: &RlpItem) -> Result<Vec<tola_types::AccessListItem>, CodecError> {
    let entries = match item {
        RlpItem::List(l) => l,
        _ => {
            return Err(CodecError::InvalidEncoding(
                "Expected RLP list for access list".into(),
            ))
        }
    };

    let mut access_list = Vec::with_capacity(entries.len());
    for entry in entries {
        let tuple = match entry {
            RlpItem::List(l) => l,
            _ => {
                return Err(CodecError::InvalidEncoding(
                    "Expected RLP list for access list item".into(),
                ))
            }
        };
        if tuple.len() != 2 {
            return Err(CodecError::InvalidEncoding(format!(
                "Expected 2 elements in access list entry, got {}",
                tuple.len()
            )));
        }
        let addr = match &tuple[0] {
            RlpItem::Bytes(b) if b.len() == 20 => {
                let mut arr = [0u8; 20];
                arr.copy_from_slice(b);
                tola_types::Address(arr)
            }
            _ => {
                return Err(CodecError::InvalidEncoding(
                    "Expected 20-byte address in access list".into(),
                ))
            }
        };
        let raw_keys = match &tuple[1] {
            RlpItem::List(l) => l,
            _ => {
                return Err(CodecError::InvalidEncoding(
                    "Expected RLP list for storage keys".into(),
                ))
            }
        };
        let mut storage_keys = Vec::with_capacity(raw_keys.len());
        for k in raw_keys {
            let key_bytes = decode_bytes32(k)?;
            storage_keys.push(tola_types::Hash(key_bytes));
        }
        access_list.push(tola_types::AccessListItem {
            address: addr,
            storage_keys,
        });
    }
    Ok(access_list)
}

/// Compute EIP-2930 Type-1 signing hash for a transaction.
pub fn compute_type1_signing_hash(tx: &tola_types::Transaction) -> tola_types::Hash {
    let chain_id_enc = encode_u64(tx.chain_id.0);
    let nonce_enc = encode_u64(tx.nonce.0);
    let gas_price_enc = encode_u128(tx.max_fee_per_gas.as_u128());
    let gas_enc = encode_u64(tx.gas_limit.as_u64());
    let to_enc = encode_addr(tx.to);
    let value_enc = encode_u128(tx.value.as_u128());
    let data_enc = encode_bytes(&tx.data);
    let access_list_enc = encode_access_list(&tx.access_list);

    let list_payload = encode_list(&[
        &chain_id_enc,
        &nonce_enc,
        &gas_price_enc,
        &gas_enc,
        &to_enc,
        &value_enc,
        &data_enc,
        &access_list_enc,
    ]);

    let mut envelope = Vec::with_capacity(1 + list_payload.len());
    envelope.push(0x01);
    envelope.extend_from_slice(&list_payload);

    tola_crypto::keccak256_hash(&envelope)
}

/// Compute EIP-1559 Type-2 signing hash for a transaction.
pub fn compute_type2_signing_hash(tx: &tola_types::Transaction) -> tola_types::Hash {
    let chain_id_enc = encode_u64(tx.chain_id.0);
    let nonce_enc = encode_u64(tx.nonce.0);
    let priority_enc = encode_u128(tx.max_priority_fee_per_gas.as_u128());
    let fee_enc = encode_u128(tx.max_fee_per_gas.as_u128());
    let gas_enc = encode_u64(tx.gas_limit.as_u64());
    let to_enc = encode_addr(tx.to);
    let value_enc = encode_u128(tx.value.as_u128());
    let data_enc = encode_bytes(&tx.data);
    let access_list_enc = encode_access_list(&tx.access_list);

    let list_payload = encode_list(&[
        &chain_id_enc,
        &nonce_enc,
        &priority_enc,
        &fee_enc,
        &gas_enc,
        &to_enc,
        &value_enc,
        &data_enc,
        &access_list_enc,
    ]);

    let mut envelope = Vec::with_capacity(1 + list_payload.len());
    envelope.push(0x02);
    envelope.extend_from_slice(&list_payload);

    tola_crypto::keccak256_hash(&envelope)
}

/// Compute Legacy EIP-155 signing hash for a transaction.
pub fn compute_legacy_signing_hash(tx: &tola_types::Transaction) -> tola_types::Hash {
    let nonce_enc = encode_u64(tx.nonce.0);
    let gas_price_enc = encode_u128(tx.max_fee_per_gas.as_u128());
    let gas_enc = encode_u64(tx.gas_limit.as_u64());
    let to_enc = encode_addr(tx.to);
    let value_enc = encode_u128(tx.value.as_u128());
    let data_enc = encode_bytes(&tx.data);
    let chain_id_enc = encode_u64(tx.chain_id.0);
    let zero_enc = encode_bytes(&[]);

    let list_payload = encode_list(&[
        &nonce_enc,
        &gas_price_enc,
        &gas_enc,
        &to_enc,
        &value_enc,
        &data_enc,
        &chain_id_enc,
        &zero_enc,
        &zero_enc,
    ]);

    tola_crypto::keccak256_hash(&list_payload)
}

/// Decode a raw signed Ethereum transaction (EIP-1559 Type-2 or Legacy EIP-155).
/// Returns canonical `(Transaction, eth_tx_hash)`.
pub fn decode_raw_eth_transaction(
    raw: &[u8],
) -> Result<(tola_types::Transaction, tola_types::Hash), CodecError> {
    if raw.is_empty() {
        return Err(CodecError::UnexpectedEof);
    }

    let eth_tx_hash = tola_crypto::keccak256_hash(raw);

    if raw[0] == 0x02 {
        // EIP-1559 Type-2 envelope
        let (item, rest) = decode_rlp(&raw[1..])?;
        if !rest.is_empty() {
            return Err(CodecError::TrailingBytes);
        }

        let items = match item {
            RlpItem::List(l) => l,
            _ => {
                return Err(CodecError::InvalidEncoding(
                    "Expected RLP list for Type-2 tx".into(),
                ))
            }
        };

        if items.len() < 12 {
            return Err(CodecError::InvalidEncoding(
                "Expected at least 12 items for signed Type-2 tx".into(),
            ));
        }

        let chain_id = decode_u64(&items[0])?;
        let nonce = decode_u64(&items[1])?;
        let max_priority_fee = decode_u128(&items[2])?;
        let max_fee = decode_u128(&items[3])?;
        let gas_limit = decode_u64(&items[4])?;
        let to = decode_address(&items[5])?;
        let value = decode_u128(&items[6])?;
        let data = match &items[7] {
            RlpItem::Bytes(b) => b.to_vec(),
            _ => {
                return Err(CodecError::InvalidEncoding(
                    "Expected bytes for tx data".into(),
                ))
            }
        };
        let access_list = decode_access_list(&items[8])?;

        let y_parity = decode_u64(&items[9])? as u8;
        let r = decode_bytes32(&items[10])?;
        let s = decode_bytes32(&items[11])?;

        // Reconstruct unsigned payload for signing hash
        let mut unsigned_tx = tola_types::Transaction {
            chain_id: tola_types::ChainId(chain_id),
            nonce: tola_types::Nonce(nonce),
            from: tola_types::Address([0u8; 20]),
            to,
            value: tola_types::Amount::from_u128(value),
            gas_limit: tola_types::Gas(gas_limit),
            max_fee_per_gas: tola_types::Amount::from_u128(max_fee),
            max_priority_fee_per_gas: tola_types::Amount::from_u128(max_priority_fee),
            valid_until_block: tola_types::BlockHeight(u64::MAX),
            data,
            memo: None,
            signature: tola_types::Signature::Secp256k1Eth {
                r,
                s,
                y_parity,
                chain_id,
            },
            tx_type: 2,
            access_list,
        };

        let signing_hash = compute_type2_signing_hash(&unsigned_tx);
        let mut sig_65 = [0u8; 65];
        sig_65[0..32].copy_from_slice(&r);
        sig_65[32..64].copy_from_slice(&s);
        sig_65[64] = y_parity;

        let recovered_from =
            tola_crypto::recover_secp256k1_address(signing_hash.as_bytes(), &sig_65).map_err(
                |e| CodecError::InvalidEncoding(format!("Signature recovery failed: {e:?}")),
            )?;

        unsigned_tx.from = recovered_from;
        Ok((unsigned_tx, eth_tx_hash))
    } else if raw[0] == 0x01 {
        // EIP-2930 Type-1 envelope
        let (item, rest) = decode_rlp(&raw[1..])?;
        if !rest.is_empty() {
            return Err(CodecError::TrailingBytes);
        }

        let items = match item {
            RlpItem::List(l) => l,
            _ => {
                return Err(CodecError::InvalidEncoding(
                    "Expected RLP list for Type-1 tx".into(),
                ))
            }
        };

        if items.len() < 11 {
            return Err(CodecError::InvalidEncoding(
                "Expected at least 11 items for signed Type-1 tx".into(),
            ));
        }

        let chain_id = decode_u64(&items[0])?;
        let nonce = decode_u64(&items[1])?;
        let gas_price = decode_u128(&items[2])?;
        let gas_limit = decode_u64(&items[3])?;
        let to = decode_address(&items[4])?;
        let value = decode_u128(&items[5])?;
        let data = match &items[6] {
            RlpItem::Bytes(b) => b.to_vec(),
            _ => {
                return Err(CodecError::InvalidEncoding(
                    "Expected bytes for tx data".into(),
                ))
            }
        };
        let access_list = decode_access_list(&items[7])?;
        let y_parity = decode_u64(&items[8])? as u8;
        let r = decode_bytes32(&items[9])?;
        let s = decode_bytes32(&items[10])?;

        let mut unsigned_tx = tola_types::Transaction {
            chain_id: tola_types::ChainId(chain_id),
            nonce: tola_types::Nonce(nonce),
            from: tola_types::Address([0u8; 20]),
            to,
            value: tola_types::Amount::from_u128(value),
            gas_limit: tola_types::Gas(gas_limit),
            max_fee_per_gas: tola_types::Amount::from_u128(gas_price),
            max_priority_fee_per_gas: tola_types::Amount::from_u128(gas_price),
            valid_until_block: tola_types::BlockHeight(u64::MAX),
            data,
            memo: None,
            signature: tola_types::Signature::Secp256k1Eth {
                r,
                s,
                y_parity,
                chain_id,
            },
            tx_type: 1,
            access_list,
        };

        let signing_hash = compute_type1_signing_hash(&unsigned_tx);
        let mut sig_65 = [0u8; 65];
        sig_65[0..32].copy_from_slice(&r);
        sig_65[32..64].copy_from_slice(&s);
        sig_65[64] = y_parity;

        let recovered_from =
            tola_crypto::recover_secp256k1_address(signing_hash.as_bytes(), &sig_65).map_err(
                |e| CodecError::InvalidEncoding(format!("Signature recovery failed: {e:?}")),
            )?;

        unsigned_tx.from = recovered_from;
        Ok((unsigned_tx, eth_tx_hash))
    } else if raw[0] >= 0xc0 {
        // Legacy EIP-155 envelope
        let (item, rest) = decode_rlp(raw)?;
        if !rest.is_empty() {
            return Err(CodecError::TrailingBytes);
        }

        let items = match item {
            RlpItem::List(l) => l,
            _ => {
                return Err(CodecError::InvalidEncoding(
                    "Expected RLP list for Legacy tx".into(),
                ))
            }
        };

        if items.len() < 9 {
            return Err(CodecError::InvalidEncoding(
                "Expected 9 items for Legacy tx".into(),
            ));
        }

        let nonce = decode_u64(&items[0])?;
        let gas_price = decode_u128(&items[1])?;
        let gas_limit = decode_u64(&items[2])?;
        let to = decode_address(&items[3])?;
        let value = decode_u128(&items[4])?;
        let data = match &items[5] {
            RlpItem::Bytes(b) => b.to_vec(),
            _ => {
                return Err(CodecError::InvalidEncoding(
                    "Expected bytes for tx data".into(),
                ))
            }
        };

        let v = decode_u64(&items[6])?;
        let r = decode_bytes32(&items[7])?;
        let s = decode_bytes32(&items[8])?;

        let (chain_id, y_parity) = if v >= 35 {
            let chain_id = (v - 35) / 2;
            let y_parity = ((v - 35) % 2) as u8;
            (chain_id, y_parity)
        } else if v == 27 || v == 28 {
            (0, (v - 27) as u8)
        } else {
            return Err(CodecError::InvalidEncoding(format!(
                "Invalid v parameter: {v}"
            )));
        };

        let mut unsigned_tx = tola_types::Transaction {
            chain_id: tola_types::ChainId(chain_id),
            nonce: tola_types::Nonce(nonce),
            from: tola_types::Address([0u8; 20]),
            to,
            value: tola_types::Amount::from_u128(value),
            gas_limit: tola_types::Gas(gas_limit),
            max_fee_per_gas: tola_types::Amount::from_u128(gas_price),
            max_priority_fee_per_gas: tola_types::Amount::from_u128(gas_price),
            valid_until_block: tola_types::BlockHeight(u64::MAX),
            data,
            memo: None,
            signature: tola_types::Signature::Secp256k1Eth {
                r,
                s,
                y_parity,
                chain_id,
            },
            tx_type: 0,
            access_list: vec![],
        };

        let signing_hash = compute_legacy_signing_hash(&unsigned_tx);
        let mut sig_65 = [0u8; 65];
        sig_65[0..32].copy_from_slice(&r);
        sig_65[32..64].copy_from_slice(&s);
        sig_65[64] = y_parity;

        let recovered_from =
            tola_crypto::recover_secp256k1_address(signing_hash.as_bytes(), &sig_65).map_err(
                |e| CodecError::InvalidEncoding(format!("Signature recovery failed: {e:?}")),
            )?;

        unsigned_tx.from = recovered_from;
        Ok((unsigned_tx, eth_tx_hash))
    } else {
        Err(CodecError::InvalidEncoding(format!(
            "Unsupported transaction prefix: 0x{:02x}",
            raw[0]
        )))
    }
}

/// Encode a signed transaction to its raw Ethereum wire bytes if it is an EVM transaction.
pub fn encode_raw_eth_transaction(tx: &tola_types::Transaction) -> Option<Vec<u8>> {
    match &tx.signature {
        tola_types::Signature::Secp256k1Eth {
            r,
            s,
            y_parity,
            chain_id,
        } => {
            if tx.tx_type == 0 {
                // Legacy EIP-155 raw envelope: rlp([nonce, gas_price, gas_limit, to, value, data, v, r, s])
                let v = if *chain_id > 0 {
                    *chain_id * 2 + 35 + (*y_parity as u64)
                } else {
                    27 + (*y_parity as u64)
                };
                let nonce_enc = encode_u64(tx.nonce.0);
                let gas_price_enc = encode_u128(tx.max_fee_per_gas.as_u128());
                let gas_enc = encode_u64(tx.gas_limit.as_u64());
                let to_enc = encode_addr(tx.to);
                let value_enc = encode_u128(tx.value.as_u128());
                let data_enc = encode_bytes(&tx.data);
                let v_enc = encode_u64(v);
                let r_enc = encode_bytes(r);
                let s_enc = encode_bytes(s);

                let list_payload = encode_list(&[
                    &nonce_enc,
                    &gas_price_enc,
                    &gas_enc,
                    &to_enc,
                    &value_enc,
                    &data_enc,
                    &v_enc,
                    &r_enc,
                    &s_enc,
                ]);
                Some(list_payload)
            } else if tx.tx_type == 1 {
                // EIP-2930 Type-1 envelope: 0x01 || rlp([chain_id, nonce, gas_price, gas_limit, to, value, data, access_list, y_parity, r, s])
                let r_enc = encode_bytes(r);
                let s_enc = encode_bytes(s);

                let chain_id_enc = encode_u64(*chain_id);
                let nonce_enc = encode_u64(tx.nonce.0);
                let gas_price_enc = encode_u128(tx.max_fee_per_gas.as_u128());
                let gas_enc = encode_u64(tx.gas_limit.as_u64());
                let to_enc = encode_addr(tx.to);
                let value_enc = encode_u128(tx.value.as_u128());
                let data_enc = encode_bytes(&tx.data);
                let access_list_enc = encode_access_list(&tx.access_list);
                let y_parity_enc = encode_u64(*y_parity as u64);

                let list_payload = encode_list(&[
                    &chain_id_enc,
                    &nonce_enc,
                    &gas_price_enc,
                    &gas_enc,
                    &to_enc,
                    &value_enc,
                    &data_enc,
                    &access_list_enc,
                    &y_parity_enc,
                    &r_enc,
                    &s_enc,
                ]);

                let mut out = Vec::with_capacity(1 + list_payload.len());
                out.push(0x01);
                out.extend_from_slice(&list_payload);
                Some(out)
            } else {
                // EIP-1559 Type-2 envelope: 0x02 || rlp([chain_id, nonce, priority, fee, gas, to, value, data, access_list, y_parity, r, s])
                let r_enc = encode_bytes(r);
                let s_enc = encode_bytes(s);

                let chain_id_enc = encode_u64(*chain_id);
                let nonce_enc = encode_u64(tx.nonce.0);
                let priority_enc = encode_u128(tx.max_priority_fee_per_gas.as_u128());
                let fee_enc = encode_u128(tx.max_fee_per_gas.as_u128());
                let gas_enc = encode_u64(tx.gas_limit.as_u64());
                let to_enc = encode_addr(tx.to);
                let value_enc = encode_u128(tx.value.as_u128());
                let data_enc = encode_bytes(&tx.data);
                let access_list_enc = encode_access_list(&tx.access_list);
                let y_parity_enc = encode_u64(*y_parity as u64);

                let list_payload = encode_list(&[
                    &chain_id_enc,
                    &nonce_enc,
                    &priority_enc,
                    &fee_enc,
                    &gas_enc,
                    &to_enc,
                    &value_enc,
                    &data_enc,
                    &access_list_enc,
                    &y_parity_enc,
                    &r_enc,
                    &s_enc,
                ]);

                let mut out = Vec::with_capacity(1 + list_payload.len());
                out.push(0x02);
                out.extend_from_slice(&list_payload);
                Some(out)
            }
        }
        _ => None,
    }
}

/// Compute Ethereum transaction hash (keccak256 of raw serialized tx) for an EVM transaction.
pub fn compute_eth_tx_hash(tx: &tola_types::Transaction) -> Option<tola_types::Hash> {
    encode_raw_eth_transaction(tx).map(|raw| tola_crypto::keccak256_hash(&raw))
}

/// Compute Ethereum contract deployment address via CREATE (keccak256(rlp([sender, nonce]))[12..32]).
pub fn compute_contract_address(sender: &tola_types::Address, nonce: u64) -> tola_types::Address {
    let rlp_sender = encode_addr(Some(*sender));
    let rlp_nonce = encode_u64(nonce);
    let rlp_list = encode_list(&[&rlp_sender, &rlp_nonce]);
    let hash = tola_crypto::keccak256_hash(&rlp_list);
    let mut addr_bytes = [0u8; 20];
    addr_bytes.copy_from_slice(&hash.as_bytes()[12..32]);
    tola_types::Address(addr_bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rlp_primitives() {
        // "dog" -> 0x83 'd' 'o' 'g'
        let enc = encode_bytes(b"dog");
        assert_eq!(enc, vec![0x83, b'd', b'o', b'g']);

        let (decoded, rest) = decode_rlp(&enc).unwrap();
        assert!(rest.is_empty());
        assert_eq!(decoded, RlpItem::Bytes(b"dog"));
    }

    #[test]
    fn test_rlp_list() {
        // ["cat", "dog"]
        let cat = encode_bytes(b"cat");
        let dog = encode_bytes(b"dog");
        let list_enc = encode_list(&[&cat, &dog]);

        let (decoded, rest) = decode_rlp(&list_enc).unwrap();
        assert!(rest.is_empty());
        match decoded {
            RlpItem::List(items) => {
                assert_eq!(items.len(), 2);
                assert_eq!(items[0], RlpItem::Bytes(b"cat"));
                assert_eq!(items[1], RlpItem::Bytes(b"dog"));
            }
            _ => panic!("Expected list"),
        }
    }

    #[test]
    fn test_type2_eip1559_roundtrip_recovery() {
        use tola_crypto::Secp256k1Keypair;

        let key = Secp256k1Keypair::generate();
        let expected_addr = key.address();
        let chain_id = 9999u64;

        let tx = tola_types::Transaction {
            chain_id: tola_types::ChainId(chain_id),
            nonce: tola_types::Nonce(5),
            from: expected_addr,
            to: Some(tola_types::Address([0x11; 20])),
            value: tola_types::Amount::from_u128(500_000),
            gas_limit: tola_types::Gas(100_000),
            max_fee_per_gas: tola_types::Amount::from_u128(20),
            max_priority_fee_per_gas: tola_types::Amount::from_u128(2),
            valid_until_block: tola_types::BlockHeight(u64::MAX),
            data: vec![0xca, 0xfe, 0xba, 0xbe],
            memo: None,
            signature: tola_types::Signature::Secp256k1Eth {
                r: [0u8; 32],
                s: [0u8; 32],
                y_parity: 0,
                chain_id,
            },
            tx_type: 2,
            access_list: vec![],
        };

        let signing_hash = compute_type2_signing_hash(&tx);
        let sig_bytes = key.sign_recoverable(signing_hash.as_bytes()).unwrap();

        let mut r = [0u8; 32];
        let mut s = [0u8; 32];
        r.copy_from_slice(&sig_bytes[0..32]);
        s.copy_from_slice(&sig_bytes[32..64]);
        let y_parity = sig_bytes[64];

        // Construct raw Type-2 envelope: 0x02 || rlp([...])
        let chain_id_enc = encode_u64(chain_id);
        let nonce_enc = encode_u64(5);
        let priority_enc = encode_u128(2);
        let fee_enc = encode_u128(20);
        let gas_enc = encode_u64(100_000);
        let to_enc = encode_addr(tx.to);
        let value_enc = encode_u128(500_000);
        let data_enc = encode_bytes(&tx.data);
        let access_list_enc = encode_list(&[]);
        let y_parity_enc = encode_u64(y_parity as u64);
        let r_enc = encode_bytes(&r);
        let s_enc = encode_bytes(&s);

        let list_payload = encode_list(&[
            &chain_id_enc,
            &nonce_enc,
            &priority_enc,
            &fee_enc,
            &gas_enc,
            &to_enc,
            &value_enc,
            &data_enc,
            &access_list_enc,
            &y_parity_enc,
            &r_enc,
            &s_enc,
        ]);

        let mut raw = vec![0x02];
        raw.extend_from_slice(&list_payload);

        let (decoded_tx, eth_hash) =
            decode_raw_eth_transaction(&raw).expect("Decode Type-2 tx succeeds");
        assert_eq!(decoded_tx.from, expected_addr);
        assert_eq!(decoded_tx.chain_id.0, chain_id);
        assert_eq!(decoded_tx.nonce.0, 5);
        assert_eq!(decoded_tx.value.as_u128(), 500_000);
        assert_eq!(decoded_tx.data, vec![0xca, 0xfe, 0xba, 0xbe]);
        assert_eq!(decoded_tx.tx_type, 2);
        assert_ne!(eth_hash, tola_types::Hash::ZERO);
    }

    #[test]
    fn test_legacy_eip155_roundtrip_recovery() {
        use tola_crypto::Secp256k1Keypair;

        let key = Secp256k1Keypair::generate();
        let expected_addr = key.address();
        let chain_id = 9999u64;

        let tx = tola_types::Transaction {
            chain_id: tola_types::ChainId(chain_id),
            nonce: tola_types::Nonce(12),
            from: expected_addr,
            to: Some(tola_types::Address([0x22; 20])),
            value: tola_types::Amount::from_u128(1_000_000),
            gas_limit: tola_types::Gas(21_000),
            max_fee_per_gas: tola_types::Amount::from_u128(15),
            max_priority_fee_per_gas: tola_types::Amount::from_u128(15),
            valid_until_block: tola_types::BlockHeight(u64::MAX),
            data: vec![],
            memo: None,
            signature: tola_types::Signature::Secp256k1Eth {
                r: [0u8; 32],
                s: [0u8; 32],
                y_parity: 0,
                chain_id,
            },
            tx_type: 0,
            access_list: vec![],
        };

        let signing_hash = compute_legacy_signing_hash(&tx);
        let sig_bytes = key.sign_recoverable(signing_hash.as_bytes()).unwrap();

        let mut r = [0u8; 32];
        let mut s = [0u8; 32];
        r.copy_from_slice(&sig_bytes[0..32]);
        s.copy_from_slice(&sig_bytes[32..64]);
        let y_parity = sig_bytes[64];

        // EIP-155: v = chain_id * 2 + 35 + y_parity
        let v = chain_id * 2 + 35 + (y_parity as u64);

        let nonce_enc = encode_u64(12);
        let gas_price_enc = encode_u128(15);
        let gas_enc = encode_u64(21_000);
        let to_enc = encode_addr(tx.to);
        let value_enc = encode_u128(1_000_000);
        let data_enc = encode_bytes(&[]);
        let v_enc = encode_u64(v);
        let r_enc = encode_bytes(&r);
        let s_enc = encode_bytes(&s);

        let raw = encode_list(&[
            &nonce_enc,
            &gas_price_enc,
            &gas_enc,
            &to_enc,
            &value_enc,
            &data_enc,
            &v_enc,
            &r_enc,
            &s_enc,
        ]);

        let (decoded_tx, eth_hash) =
            decode_raw_eth_transaction(&raw).expect("Decode Legacy EIP-155 tx succeeds");
        assert_eq!(decoded_tx.from, expected_addr);
        assert_eq!(decoded_tx.chain_id.0, chain_id);
        assert_eq!(decoded_tx.nonce.0, 12);
        assert_eq!(decoded_tx.value.as_u128(), 1_000_000);
        assert_eq!(decoded_tx.tx_type, 0);
        assert_ne!(eth_hash, tola_types::Hash::ZERO);
    }

    #[test]
    fn test_access_list_codec_roundtrip() {
        let al = vec![
            tola_types::AccessListItem {
                address: tola_types::Address([0xaa; 20]),
                storage_keys: vec![tola_types::Hash([0x11; 32]), tola_types::Hash([0x22; 32])],
            },
            tola_types::AccessListItem {
                address: tola_types::Address([0xbb; 20]),
                storage_keys: vec![],
            },
        ];

        let encoded = encode_access_list(&al);
        let (decoded_item, rest) = decode_rlp(&encoded).expect("RLP decode succeeds");
        assert!(rest.is_empty());
        let decoded_al = decode_access_list(&decoded_item).expect("decode_access_list succeeds");
        assert_eq!(al, decoded_al);
    }

    #[test]
    fn test_type1_eip2930_roundtrip_recovery() {
        use tola_crypto::Secp256k1Keypair;

        let key = Secp256k1Keypair::generate();
        let expected_addr = key.address();
        let chain_id = 9999u64;

        let al = vec![tola_types::AccessListItem {
            address: tola_types::Address([0xcc; 20]),
            storage_keys: vec![tola_types::Hash([0x33; 32])],
        }];

        let mut tx = tola_types::Transaction {
            chain_id: tola_types::ChainId(chain_id),
            nonce: tola_types::Nonce(7),
            from: expected_addr,
            to: Some(tola_types::Address([0x44; 20])),
            value: tola_types::Amount::from_u128(250_000),
            gas_limit: tola_types::Gas(75_000),
            max_fee_per_gas: tola_types::Amount::from_u128(25),
            max_priority_fee_per_gas: tola_types::Amount::from_u128(25),
            valid_until_block: tola_types::BlockHeight(u64::MAX),
            data: vec![0x12, 0x34],
            memo: None,
            signature: tola_types::Signature::Secp256k1Eth {
                r: [0u8; 32],
                s: [0u8; 32],
                y_parity: 0,
                chain_id,
            },
            tx_type: 1,
            access_list: al.clone(),
        };

        let signing_hash = compute_type1_signing_hash(&tx);
        let sig_bytes = key.sign_recoverable(signing_hash.as_bytes()).unwrap();

        let mut r = [0u8; 32];
        let mut s = [0u8; 32];
        r.copy_from_slice(&sig_bytes[0..32]);
        s.copy_from_slice(&sig_bytes[32..64]);
        let y_parity = sig_bytes[64];

        tx.signature = tola_types::Signature::Secp256k1Eth {
            r,
            s,
            y_parity,
            chain_id,
        };

        let raw = encode_raw_eth_transaction(&tx).expect("encode Type-1 succeeds");
        let (decoded_tx, eth_hash) =
            decode_raw_eth_transaction(&raw).expect("Decode Type-1 tx succeeds");
        assert_eq!(decoded_tx.from, expected_addr);
        assert_eq!(decoded_tx.chain_id.0, chain_id);
        assert_eq!(decoded_tx.nonce.0, 7);
        assert_eq!(decoded_tx.value.as_u128(), 250_000);
        assert_eq!(decoded_tx.tx_type, 1);
        assert_eq!(decoded_tx.access_list, al);
        assert_ne!(eth_hash, tola_types::Hash::ZERO);
    }

    #[test]
    fn test_type2_with_access_list_roundtrip() {
        use tola_crypto::Secp256k1Keypair;

        let key = Secp256k1Keypair::generate();
        let expected_addr = key.address();
        let chain_id = 9999u64;

        let al = vec![tola_types::AccessListItem {
            address: tola_types::Address([0xee; 20]),
            storage_keys: vec![tola_types::Hash([0x55; 32])],
        }];

        let mut tx = tola_types::Transaction {
            chain_id: tola_types::ChainId(chain_id),
            nonce: tola_types::Nonce(10),
            from: expected_addr,
            to: Some(tola_types::Address([0x66; 20])),
            value: tola_types::Amount::from_u128(100_000),
            gas_limit: tola_types::Gas(80_000),
            max_fee_per_gas: tola_types::Amount::from_u128(30),
            max_priority_fee_per_gas: tola_types::Amount::from_u128(3),
            valid_until_block: tola_types::BlockHeight(u64::MAX),
            data: vec![0xaa, 0xbb],
            memo: None,
            signature: tola_types::Signature::Secp256k1Eth {
                r: [0u8; 32],
                s: [0u8; 32],
                y_parity: 0,
                chain_id,
            },
            tx_type: 2,
            access_list: al.clone(),
        };

        let signing_hash = compute_type2_signing_hash(&tx);
        let sig_bytes = key.sign_recoverable(signing_hash.as_bytes()).unwrap();

        let mut r = [0u8; 32];
        let mut s = [0u8; 32];
        r.copy_from_slice(&sig_bytes[0..32]);
        s.copy_from_slice(&sig_bytes[32..64]);
        let y_parity = sig_bytes[64];

        tx.signature = tola_types::Signature::Secp256k1Eth {
            r,
            s,
            y_parity,
            chain_id,
        };

        let raw = encode_raw_eth_transaction(&tx).expect("encode Type-2 with AL succeeds");
        let (decoded_tx, eth_hash) =
            decode_raw_eth_transaction(&raw).expect("Decode Type-2 tx succeeds");
        assert_eq!(decoded_tx.from, expected_addr);
        assert_eq!(decoded_tx.tx_type, 2);
        assert_eq!(decoded_tx.access_list, al);
        assert_ne!(eth_hash, tola_types::Hash::ZERO);
    }
}
