#![allow(clippy::all)]
//! Production JSON-RPC 2.0 interface and Ethereum compatibility method dispatch.
//!
//! Complies with standard Ethereum JSON-RPC 2.0 specifications (web3_*, net_*, eth_*)
//! and provides native TOLA protocol extensions (tola_*).

pub mod dispatcher;
pub mod explorer;
pub mod indexer;
pub mod json;
pub mod server;
pub mod subscription;
pub mod types;
pub mod ws;

pub use dispatcher::{compute_transaction_id, RpcDispatcher};
pub use indexer::{ChainIndexer, IndexerStatus};
pub use json::JsonValue;
pub use server::{MetricsCallback, RpcServer};
pub use subscription::SubscriptionManager;
pub use types::{
    JsonRpcError, JsonRpcRequest, JsonRpcResponse, ERROR_INTERNAL_ERROR, ERROR_INVALID_PARAMS,
    ERROR_INVALID_REQUEST, ERROR_METHOD_NOT_FOUND, ERROR_PARSE_ERROR, ERROR_SERVER_ERROR,
};
