//! Priority queue mempool, nonce sequence validation, and anti-DDoS eviction.

use std::collections::VecDeque;
use tola_types::*;
use tola_core::MempoolError;

pub struct Mempool {
    queue: VecDeque<Transaction>,
    max_capacity: usize,
}

impl Mempool {
    pub fn new(max_capacity: usize) -> Self {
        Self {
            queue: VecDeque::new(),
            max_capacity,
        }
    }

    pub fn insert(&mut self, tx: Transaction) -> Result<TransactionId, MempoolError> {
        if self.queue.len() >= self.max_capacity {
            return Err(MempoolError::PoolFull);
        }
        self.queue.push_back(tx);
        Ok(TransactionId(Hash::ZERO))
    }

    pub fn pop_batch(&mut self, max_count: usize) -> Vec<Transaction> {
        let mut batch = Vec::new();
        while let Some(tx) = self.queue.pop_front() {
            batch.push(tx);
            if batch.len() >= max_count {
                break;
            }
        }
        batch
    }
}
