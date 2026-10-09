//! Block-STM concurrency scheduler managing optimistic execution and validation tasks.
//!
//! Implements multi-version incarnation tracking, dynamic dependency re-validation,
//! and bounded-abort fallback detection conforming to ADR-0012.

use std::collections::BTreeSet;
use std::sync::{Condvar, Mutex};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Task {
    Execution(usize, usize),  // (tx_index, incarnation)
    Validation(usize, usize), // (tx_index, incarnation)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TxStatus {
    ReadyToExecute,
    Executing,
    Executed,
}

pub struct SchedulerState {
    pub num_txs: usize,
    pub statuses: Vec<TxStatus>,
    pub incarnations: Vec<usize>,
    pub execution_queue: BTreeSet<usize>,
    pub validation_queue: BTreeSet<usize>,
    pub aborts_count: usize,
    pub max_aborts: usize,
    pub active_workers: usize,
    pub done: bool,
    pub fallback_required: bool,
}

pub struct BlockStmScheduler {
    state: Mutex<SchedulerState>,
    cvar: Condvar,
}

impl BlockStmScheduler {
    pub fn new(num_txs: usize, max_aborts: usize) -> Self {
        let mut execution_queue = BTreeSet::new();
        for i in 0..num_txs {
            execution_queue.insert(i);
        }

        Self {
            state: Mutex::new(SchedulerState {
                num_txs,
                statuses: vec![TxStatus::ReadyToExecute; num_txs],
                incarnations: vec![0; num_txs],
                execution_queue,
                validation_queue: BTreeSet::new(),
                aborts_count: 0,
                max_aborts,
                active_workers: 0,
                done: num_txs == 0,
                fallback_required: false,
            }),
            cvar: Condvar::new(),
        }
    }

    /// Acquires the next task (Execution or Validation), blocking until work is available
    /// or scheduling completes.
    pub fn pop_task(&self) -> Option<Task> {
        let mut s = self.state.lock().unwrap();

        loop {
            if s.done || s.fallback_required {
                return None;
            }

            // Priority 1: Lowest index in execution queue
            if let Some(&idx) = s.execution_queue.iter().next() {
                s.execution_queue.remove(&idx);
                s.statuses[idx] = TxStatus::Executing;
                s.active_workers += 1;
                let inc = s.incarnations[idx];
                return Some(Task::Execution(idx, inc));
            }

            // Priority 2: Lowest index in validation queue
            if let Some(&idx) = s.validation_queue.iter().next() {
                s.validation_queue.remove(&idx);
                s.active_workers += 1;
                let inc = s.incarnations[idx];
                return Some(Task::Validation(idx, inc));
            }

            // No immediate work available: check if all transactions are executed & validated
            if s.active_workers == 0 {
                let all_executed = s.statuses.iter().all(|st| *st == TxStatus::Executed);
                if all_executed && s.execution_queue.is_empty() && s.validation_queue.is_empty() {
                    s.done = true;
                    self.cvar.notify_all();
                    return None;
                }
            }

            // Wait for active workers to produce tasks or signal completion
            s = self.cvar.wait(s).unwrap();
        }
    }

    /// Signals that transaction execution has completed at a specific incarnation.
    pub fn finish_execution(&self, tx_index: usize, incarnation: usize) -> bool {
        let mut s = self.state.lock().unwrap();
        s.active_workers = s.active_workers.saturating_sub(1);

        let accepted = s.incarnations[tx_index] == incarnation;
        if accepted {
            s.statuses[tx_index] = TxStatus::Executed;
            s.validation_queue.insert(tx_index);

            // Re-validate subsequent executed transactions whose read sets may be affected
            for j in (tx_index + 1)..s.num_txs {
                if s.statuses[j] == TxStatus::Executed {
                    s.validation_queue.insert(j);
                }
            }
        }

        self.cvar.notify_all();
        accepted
    }

    /// Signals that validation of a transaction's read set has concluded.
    pub fn finish_validation(&self, tx_index: usize, incarnation: usize, valid: bool) -> bool {
        let mut s = self.state.lock().unwrap();
        s.active_workers = s.active_workers.saturating_sub(1);

        let mut accepted = false;
        if s.incarnations[tx_index] == incarnation {
            accepted = true;
            if !valid {
                s.aborts_count += 1;
                if s.aborts_count >= s.max_aborts {
                    s.fallback_required = true;
                    self.cvar.notify_all();
                    return true;
                }

                // Bump incarnation and re-schedule execution
                s.incarnations[tx_index] += 1;
                s.statuses[tx_index] = TxStatus::ReadyToExecute;
                s.execution_queue.insert(tx_index);

                // Subsequent executed transactions must be validated
                for j in (tx_index + 1)..s.num_txs {
                    if s.statuses[j] == TxStatus::Executed {
                        s.validation_queue.insert(j);
                    }
                }
            }
        }

        self.cvar.notify_all();
        accepted
    }

    /// Returns whether sequential fallback is required due to excessive aborts.
    pub fn is_fallback_required(&self) -> bool {
        self.state.lock().unwrap().fallback_required
    }

    /// Returns total aborts encountered during the execution wave.
    pub fn aborts_count(&self) -> usize {
        self.state.lock().unwrap().aborts_count
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scheduler_lifecycle_and_validation() {
        let scheduler = BlockStmScheduler::new(3, 10);

        // First task is Execution(0, 0)
        let task0 = scheduler.pop_task().unwrap();
        assert_eq!(task0, Task::Execution(0, 0));

        // Finish execution of 0 -> enqueues validation of 0
        scheduler.finish_execution(0, 0);

        // Next task can be Execution(1, 0) (higher priority execution over validation)
        let task1 = scheduler.pop_task().unwrap();
        assert_eq!(task1, Task::Execution(1, 0));

        scheduler.finish_execution(1, 0);

        // Next is Execution(2, 0)
        let task2 = scheduler.pop_task().unwrap();
        assert_eq!(task2, Task::Execution(2, 0));

        scheduler.finish_execution(2, 0);

        // Validate all 3
        let val0 = scheduler.pop_task().unwrap();
        assert_eq!(val0, Task::Validation(0, 0));
        scheduler.finish_validation(0, 0, true);

        let val1 = scheduler.pop_task().unwrap();
        assert_eq!(val1, Task::Validation(1, 0));
        scheduler.finish_validation(1, 0, true);

        let val2 = scheduler.pop_task().unwrap();
        assert_eq!(val2, Task::Validation(2, 0));
        scheduler.finish_validation(2, 0, true);

        // All done!
        assert!(scheduler.pop_task().is_none());
        assert!(!scheduler.is_fallback_required());
    }

    #[test]
    fn test_scheduler_abort_and_reexecution() {
        let scheduler = BlockStmScheduler::new(2, 10);

        // Execute 0 and 1
        let _ = scheduler.pop_task().unwrap(); // Exec(0, 0)
        scheduler.finish_execution(0, 0);

        let _ = scheduler.pop_task().unwrap(); // Exec(1, 0)
        scheduler.finish_execution(1, 0);

        // Validate 0 -> Pass
        let _ = scheduler.pop_task().unwrap();
        scheduler.finish_validation(0, 0, true);

        // Validate 1 -> Fail (abort!)
        let val1 = scheduler.pop_task().unwrap();
        assert_eq!(val1, Task::Validation(1, 0));
        scheduler.finish_validation(1, 0, false);

        // Should re-enqueue Execution(1, 1)
        let re_exec1 = scheduler.pop_task().unwrap();
        assert_eq!(re_exec1, Task::Execution(1, 1));
        scheduler.finish_execution(1, 1);

        // Validate 1 at inc 1 -> Pass
        let re_val1 = scheduler.pop_task().unwrap();
        assert_eq!(re_val1, Task::Validation(1, 1));
        scheduler.finish_validation(1, 1, true);

        // Finished!
        assert!(scheduler.pop_task().is_none());
        assert_eq!(scheduler.aborts_count(), 1);
    }
}
