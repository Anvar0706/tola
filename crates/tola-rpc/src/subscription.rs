//! Connection-scoped Ethereum JSON-RPC Subscription Manager (EIP-1474, eth_subscribe/eth_unsubscribe).

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, RwLock};

use tola_types::*;

use crate::dispatcher::{FilterCriteria, RpcDispatcher};
use crate::json::JsonValue;

pub const MAX_WEBSOCKET_CONNECTIONS: usize = 256;
pub const MAX_SUBSCRIPTIONS_PER_CONNECTION: usize = 32;
pub const MAX_TOTAL_SUBSCRIPTIONS: usize = 2048;
pub const MAX_QUEUE_MESSAGES: usize = 512;
pub const MAX_QUEUE_BYTES: usize = 2 * 1024 * 1024; // 2 MB
pub const MAX_MESSAGE_SIZE: usize = 65536; // 64 KB

#[derive(Clone)]
pub struct ConnectionSender {
    pub conn_id: u64,
    pub sender: SyncSender<String>,
    pub queued_messages: Arc<AtomicUsize>,
    pub queued_bytes: Arc<AtomicUsize>,
    pub is_stalled: Arc<std::sync::atomic::AtomicBool>,
}

impl ConnectionSender {
    /// Attempts to enqueue a message respecting the two-dimensional bounds (message count + byte count).
    /// Guarantees that a slow subscriber NEVER blocks the caller.
    pub fn try_send_notification(&self, message: String) -> Result<(), &'static str> {
        let msg_len = message.len();
        if msg_len > MAX_MESSAGE_SIZE {
            return Err("message exceeds MAX_MESSAGE_SIZE");
        }

        let cur_msgs = self.queued_messages.load(Ordering::SeqCst);
        let cur_bytes = self.queued_bytes.load(Ordering::SeqCst);

        if cur_msgs + 1 > MAX_QUEUE_MESSAGES || cur_bytes + msg_len > MAX_QUEUE_BYTES {
            self.is_stalled.store(true, Ordering::SeqCst);
            return Err("queue full");
        }

        self.queued_messages.fetch_add(1, Ordering::SeqCst);
        self.queued_bytes.fetch_add(msg_len, Ordering::SeqCst);

        match self.sender.try_send(message) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(_)) => {
                self.queued_messages.fetch_sub(1, Ordering::SeqCst);
                self.queued_bytes.fetch_sub(msg_len, Ordering::SeqCst);
                self.is_stalled.store(true, Ordering::SeqCst);
                Err("queue full")
            }
            Err(TrySendError::Disconnected(_)) => {
                self.queued_messages.fetch_sub(1, Ordering::SeqCst);
                self.queued_bytes.fetch_sub(msg_len, Ordering::SeqCst);
                Err("connection disconnected")
            }
        }
    }
}

pub struct SubscriptionManager {
    next_sub_id: AtomicU64,
    next_conn_id: AtomicU64,
    pub active_connections: AtomicUsize,

    // Subscriptions indexed by unique subscription ID
    new_heads: RwLock<HashMap<u64, (u64, ConnectionSender)>>,
    logs: RwLock<HashMap<u64, (u64, FilterCriteria, ConnectionSender)>>,
    pending_txs: RwLock<HashMap<u64, (u64, ConnectionSender)>>,

    // Connection ownership: conn_id -> set of sub_ids
    connection_subs: RwLock<HashMap<u64, HashSet<u64>>>,

    // Operational metrics
    pub events_sent_total: AtomicU64,
    pub events_dropped_total: AtomicU64,
    pub slow_clients_disconnected_total: AtomicU64,
}

impl Default for SubscriptionManager {
    fn default() -> Self {
        Self::new()
    }
}

impl SubscriptionManager {
    pub fn new() -> Self {
        Self {
            next_sub_id: AtomicU64::new(1),
            next_conn_id: AtomicU64::new(1),
            active_connections: AtomicUsize::new(0),
            new_heads: RwLock::new(HashMap::new()),
            logs: RwLock::new(HashMap::new()),
            pending_txs: RwLock::new(HashMap::new()),
            connection_subs: RwLock::new(HashMap::new()),
            events_sent_total: AtomicU64::new(0),
            events_dropped_total: AtomicU64::new(0),
            slow_clients_disconnected_total: AtomicU64::new(0),
        }
    }

    /// Registers a new active WebSocket connection and provisions a bounded event channel.
    pub fn register_connection(
        &self,
    ) -> Result<(u64, Receiver<String>, ConnectionSender), &'static str> {
        let current_conns = self.active_connections.load(Ordering::SeqCst);
        if current_conns >= MAX_WEBSOCKET_CONNECTIONS {
            return Err("Maximum concurrent WebSocket connections reached");
        }

        self.active_connections.fetch_add(1, Ordering::SeqCst);
        let conn_id = self.next_conn_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = sync_channel(MAX_QUEUE_MESSAGES);

        let sender = ConnectionSender {
            conn_id,
            sender: tx,
            queued_messages: Arc::new(AtomicUsize::new(0)),
            queued_bytes: Arc::new(AtomicUsize::new(0)),
            is_stalled: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        };

        self.connection_subs
            .write()
            .unwrap()
            .insert(conn_id, HashSet::new());

        Ok((conn_id, rx, sender))
    }

    /// Unregisters a connection upon TCP disconnect or shutdown, removing all its active subscriptions.
    pub fn cleanup_connection(&self, conn_id: u64) {
        self.active_connections.fetch_sub(1, Ordering::SeqCst);
        let removed_subs = self
            .connection_subs
            .write()
            .unwrap()
            .remove(&conn_id)
            .unwrap_or_default();

        if !removed_subs.is_empty() {
            let mut nh = self.new_heads.write().unwrap();
            let mut lg = self.logs.write().unwrap();
            let mut pt = self.pending_txs.write().unwrap();

            for sub_id in removed_subs {
                nh.remove(&sub_id);
                lg.remove(&sub_id);
                pt.remove(&sub_id);
            }
        }
    }

    fn check_subscription_limits(&self, conn_id: u64) -> Result<(), &'static str> {
        let c_subs = self.connection_subs.read().unwrap();
        let subs_for_conn = c_subs.get(&conn_id).map(|s| s.len()).unwrap_or(0);
        if subs_for_conn >= MAX_SUBSCRIPTIONS_PER_CONNECTION {
            return Err("Maximum subscriptions per connection exceeded");
        }

        let total = self.new_heads.read().unwrap().len()
            + self.logs.read().unwrap().len()
            + self.pending_txs.read().unwrap().len();

        if total >= MAX_TOTAL_SUBSCRIPTIONS {
            return Err("Global subscription capacity reached");
        }

        Ok(())
    }

    /// Creates a `newHeads` subscription bound to `conn_id`.
    pub fn subscribe_new_heads(
        &self,
        conn_id: u64,
        sender: ConnectionSender,
    ) -> Result<String, &'static str> {
        self.check_subscription_limits(conn_id)?;
        let sub_id = self.next_sub_id.fetch_add(1, Ordering::SeqCst);

        self.new_heads
            .write()
            .unwrap()
            .insert(sub_id, (conn_id, sender));
        self.connection_subs
            .write()
            .unwrap()
            .entry(conn_id)
            .or_default()
            .insert(sub_id);

        Ok(format!("0x{:x}", sub_id))
    }

    /// Creates a `logs` subscription bound to `conn_id` with specific `FilterCriteria`.
    pub fn subscribe_logs(
        &self,
        conn_id: u64,
        criteria: FilterCriteria,
        sender: ConnectionSender,
    ) -> Result<String, &'static str> {
        self.check_subscription_limits(conn_id)?;
        let sub_id = self.next_sub_id.fetch_add(1, Ordering::SeqCst);

        self.logs
            .write()
            .unwrap()
            .insert(sub_id, (conn_id, criteria, sender));
        self.connection_subs
            .write()
            .unwrap()
            .entry(conn_id)
            .or_default()
            .insert(sub_id);

        Ok(format!("0x{:x}", sub_id))
    }

    /// Creates a `newPendingTransactions` subscription bound to `conn_id`.
    pub fn subscribe_pending_transactions(
        &self,
        conn_id: u64,
        sender: ConnectionSender,
    ) -> Result<String, &'static str> {
        self.check_subscription_limits(conn_id)?;
        let sub_id = self.next_sub_id.fetch_add(1, Ordering::SeqCst);

        self.pending_txs
            .write()
            .unwrap()
            .insert(sub_id, (conn_id, sender));
        self.connection_subs
            .write()
            .unwrap()
            .entry(conn_id)
            .or_default()
            .insert(sub_id);

        Ok(format!("0x{:x}", sub_id))
    }

    /// Unsubscribes a subscription ID. Returns `true` only if the subscription exists and belongs to `conn_id`.
    pub fn unsubscribe(&self, conn_id: u64, sub_id_str: &str) -> bool {
        let clean = sub_id_str
            .strip_prefix("0x")
            .or_else(|| sub_id_str.strip_prefix("0X"))
            .unwrap_or(sub_id_str);

        let sub_id = match u64::from_str_radix(clean, 16).or_else(|_| clean.parse::<u64>()) {
            Ok(id) => id,
            Err(_) => return false,
        };

        let mut c_guard = self.connection_subs.write().unwrap();
        if let Some(subs) = c_guard.get_mut(&conn_id) {
            if subs.remove(&sub_id) {
                self.new_heads.write().unwrap().remove(&sub_id);
                self.logs.write().unwrap().remove(&sub_id);
                self.pending_txs.write().unwrap().remove(&sub_id);
                return true;
            }
        }

        false
    }

    /// Emits `newHeads` notification to all active subscribers. Guaranteed strictly non-blocking.
    pub fn notify_new_head(&self, header_json: &JsonValue) {
        let subs = self.new_heads.read().unwrap();
        if subs.is_empty() {
            return;
        }

        for (sub_id, (_conn_id, sender)) in subs.iter() {
            let notification = JsonValue::Object(vec![
                ("jsonrpc".to_string(), JsonValue::String("2.0".into())),
                (
                    "method".to_string(),
                    JsonValue::String("eth_subscription".into()),
                ),
                (
                    "params".to_string(),
                    JsonValue::Object(vec![
                        (
                            "subscription".to_string(),
                            JsonValue::String(format!("0x{:x}", sub_id)),
                        ),
                        ("result".to_string(), header_json.clone()),
                    ]),
                ),
            ])
            .serialize();

            match sender.try_send_notification(notification) {
                Ok(()) => {
                    self.events_sent_total.fetch_add(1, Ordering::Relaxed);
                }
                Err(_) => {
                    self.events_dropped_total.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
    }

    /// Emits matched log events for a committed block to all active `logs` subscribers.
    pub fn notify_logs(&self, block: &Block, block_hash: Hash, receipts: &[Receipt]) {
        let subs = self.logs.read().unwrap();
        if subs.is_empty() {
            return;
        }

        let height = block.header.height.0;
        let mut block_log_index = 0u64;

        for (tx_idx, (tx, r)) in block.transactions.iter().zip(receipts.iter()).enumerate() {
            let tx_id = crate::dispatcher::compute_transaction_id(tx);
            let tx_hash_hex =
                RpcDispatcher::<tola_state::MemoryStateJournal>::get_transaction_hash_hex(
                    tx, &tx_id,
                );

            for log in &r.logs {
                let current_log_index = block_log_index;
                block_log_index += 1;

                for (sub_id, (_conn_id, criteria, sender)) in subs.iter() {
                    if RpcDispatcher::<tola_state::MemoryStateJournal>::log_matches_criteria(
                        log, criteria,
                    ) {
                        let log_json =
                            RpcDispatcher::<tola_state::MemoryStateJournal>::format_log_json(
                                log,
                                height,
                                block_hash,
                                &tx_hash_hex,
                                tx_idx as u64,
                                current_log_index,
                            );

                        let notification = JsonValue::Object(vec![
                            ("jsonrpc".to_string(), JsonValue::String("2.0".into())),
                            (
                                "method".to_string(),
                                JsonValue::String("eth_subscription".into()),
                            ),
                            (
                                "params".to_string(),
                                JsonValue::Object(vec![
                                    (
                                        "subscription".to_string(),
                                        JsonValue::String(format!("0x{:x}", sub_id)),
                                    ),
                                    ("result".to_string(), log_json),
                                ]),
                            ),
                        ])
                        .serialize();

                        match sender.try_send_notification(notification) {
                            Ok(()) => {
                                self.events_sent_total.fetch_add(1, Ordering::Relaxed);
                            }
                            Err(_) => {
                                self.events_dropped_total.fetch_add(1, Ordering::Relaxed);
                            }
                        }
                    }
                }
            }
        }
    }

    /// Emits `newPendingTransactions` notification to active subscribers.
    pub fn notify_pending_transaction(&self, tx_hash_hex: &str) {
        let subs = self.pending_txs.read().unwrap();
        if subs.is_empty() {
            return;
        }

        for (sub_id, (_conn_id, sender)) in subs.iter() {
            let notification = JsonValue::Object(vec![
                ("jsonrpc".to_string(), JsonValue::String("2.0".into())),
                (
                    "method".to_string(),
                    JsonValue::String("eth_subscription".into()),
                ),
                (
                    "params".to_string(),
                    JsonValue::Object(vec![
                        (
                            "subscription".to_string(),
                            JsonValue::String(format!("0x{:x}", sub_id)),
                        ),
                        (
                            "result".to_string(),
                            JsonValue::String(tx_hash_hex.to_string()),
                        ),
                    ]),
                ),
            ])
            .serialize();

            match sender.try_send_notification(notification) {
                Ok(()) => {
                    self.events_sent_total.fetch_add(1, Ordering::Relaxed);
                }
                Err(_) => {
                    self.events_dropped_total.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_subscription_lifecycle_and_ownership() {
        let sm = SubscriptionManager::new();
        let (conn1, _rx1, sender1) = sm.register_connection().unwrap();
        let (conn2, _rx2, _sender2) = sm.register_connection().unwrap();

        // 1. Subscribe newHeads on conn1
        let sub1 = sm.subscribe_new_heads(conn1, sender1).unwrap();
        assert!(sub1.starts_with("0x"));

        // 2. Conn2 cannot unsubscribe Conn1's subscription
        let unsub_cross = sm.unsubscribe(conn2, &sub1);
        assert_eq!(unsub_cross, false);

        // 3. Conn1 successfully unsubscribes
        let unsub_ok = sm.unsubscribe(conn1, &sub1);
        assert_eq!(unsub_ok, true);

        // 4. Repeated unsubscribe returns false
        assert_eq!(sm.unsubscribe(conn1, &sub1), false);
    }

    #[test]
    fn test_queue_overflow_backpressure_drop() {
        let sm = SubscriptionManager::new();
        let (conn1, rx1, sender1) = sm.register_connection().unwrap();

        let _sub1 = sm.subscribe_pending_transactions(conn1, sender1).unwrap();

        // Flood 600 messages (exceeding MAX_QUEUE_MESSAGES = 512)
        for i in 0..600 {
            sm.notify_pending_transaction(&format!("0x{:x}", i));
        }

        // Dropped total must be at least (600 - 512) = 88
        assert!(sm.events_dropped_total.load(Ordering::SeqCst) >= 88);
        assert_eq!(sm.events_sent_total.load(Ordering::SeqCst), 512);

        // Drain channel
        let mut count = 0;
        while let Ok(_) = rx1.try_recv() {
            count += 1;
        }
        assert_eq!(count, 512);
    }

    #[test]
    fn test_disconnect_cleans_all_subscriptions() {
        let sm = SubscriptionManager::new();
        let (conn1, _rx1, sender1) = sm.register_connection().unwrap();

        let _sub_nh = sm.subscribe_new_heads(conn1, sender1.clone()).unwrap();
        let _sub_pt = sm.subscribe_pending_transactions(conn1, sender1).unwrap();

        assert_eq!(sm.new_heads.read().unwrap().len(), 1);
        assert_eq!(sm.pending_txs.read().unwrap().len(), 1);

        // Cleanup connection
        sm.cleanup_connection(conn1);

        assert_eq!(sm.new_heads.read().unwrap().len(), 0);
        assert_eq!(sm.pending_txs.read().unwrap().len(), 0);
    }
}
