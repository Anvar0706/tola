//! JSON-RPC 2.0 interface and Ethereum compatibility method dispatch.

use tola_types::*;

pub struct JsonRpcRequest {
    pub method: String,
    pub id: u64,
}

pub struct JsonRpcResponse {
    pub id: u64,
    pub result: Option<String>,
    pub error: Option<String>,
}

pub fn handle_eth_block_number(current_height: BlockHeight) -> String {
    format!("0x{:x}", current_height.0)
}
