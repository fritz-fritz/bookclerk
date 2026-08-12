//! Cumulative plugin jail CPU budget (percent of one logical CPU).
//!
//! Each confined guest Spec carries a requested `cpu_rate_percent`. Concurrent
//! plugin jails share a process-wide pool whose ceiling is
//! `[plugins.jail].cpu_rate_percent` when set, otherwise
//! [`crate::consent::host_cpu_rate_max`].
//!
//! # Oversubscription
//!
//! Operators may grant each plugin less than the global ceiling while the
//! **sum** of grants exceeds it (global > max(single) but global < Σ grants).
//! Admission is **fit-to-remaining**: a spawn receives
//! `min(requested, remaining_pool)` and the Spec CPU rate is rewritten to that
//! allocated value before the jail starts. Only a fully exhausted pool
//! (remaining &lt; 1%) refuses spawn. Reload of the same `plugin_id` replaces
//! its prior allocation (counts once).

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use bookclerk_config::Config;

use crate::consent::host_cpu_rate_max;

#[derive(Debug)]
struct Entry {
    generation: u64,
    /// Actually reserved in the pool (≤ requested Spec rate).
    allocated: u32,
}

#[derive(Debug, Default)]
struct CpuBudgetState {
    live: HashMap<String, Entry>,
}

fn state() -> &'static Mutex<CpuBudgetState> {
    static STATE: OnceLock<Mutex<CpuBudgetState>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(CpuBudgetState::default()))
}

static NEXT_GEN: AtomicU64 = AtomicU64::new(1);

/// Cumulative CPU pool ceiling for plugin jails (one-core percent units).
#[must_use]
pub fn cumulative_cpu_ceiling(config: &Config) -> u32 {
    let host_max = host_cpu_rate_max();
    match config.plugins.jail.cpu_rate_percent {
        Some(ceiling) => ceiling.clamp(1, host_max),
        None => host_max,
    }
}

/// RAII lease holding one plugin's allocation in the cumulative pool.
#[derive(Debug)]
pub struct CpuBudgetLease {
    plugin_id: String,
    generation: u64,
    requested: u32,
    allocated: u32,
}

impl CpuBudgetLease {
    /// Rate actually reserved (and that should be written into the jail Spec).
    #[must_use]
    pub fn allocated_rate(&self) -> u32 {
        self.allocated
    }

    /// Rate the Spec/grant asked for before fit-to-remaining.
    #[must_use]
    pub fn requested_rate(&self) -> u32 {
        self.requested
    }

    /// Whether the pool throttled this guest below its request.
    #[must_use]
    pub fn was_throttled(&self) -> bool {
        self.allocated < self.requested
    }

    /// Reserve CPU for `plugin_id`, fitting into whatever pool remains.
    ///
    /// Allocates `min(requested, ceiling - sum(others))`. An existing allocation
    /// for the same id is replaced (reload overlap).
    ///
    /// # Errors
    ///
    /// Returns a message only when the pool has no remaining capacity (`< 1%`).
    pub fn try_acquire(
        plugin_id: &str,
        requested: u32,
        cumulative_ceiling: u32,
    ) -> Result<Self, String> {
        let requested = requested.max(1);
        let ceiling = cumulative_ceiling.max(1);
        let mut guard = state()
            .lock()
            .map_err(|_| "plugin CPU budget lock poisoned".to_string())?;
        let mut sum_others: u64 = 0;
        for (id, entry) in &guard.live {
            if id != plugin_id {
                sum_others = sum_others.saturating_add(u64::from(entry.allocated));
            }
        }
        let remaining = u64::from(ceiling).saturating_sub(sum_others);
        if remaining == 0 {
            return Err(format!(
                "plugin CPU budget exhausted: {sum_others}% of one CPU already \
                 allocated (ceiling {ceiling}%; plugin `{plugin_id}` requested \
                 {requested}%). Stop or lower other plugins, or raise \
                 [plugins.jail].cpu_rate_percent"
            ));
        }
        let allocated = requested.min(remaining as u32).max(1);
        let generation = NEXT_GEN.fetch_add(1, Ordering::Relaxed);
        guard.live.insert(
            plugin_id.to_string(),
            Entry {
                generation,
                allocated,
            },
        );
        Ok(Self {
            plugin_id: plugin_id.to_string(),
            generation,
            requested,
            allocated,
        })
    }

    /// Test helper: clear all allocations.
    #[cfg(test)]
    pub fn reset_for_tests() {
        if let Ok(mut guard) = state().lock() {
            guard.live.clear();
        }
    }

    /// Test helper: sum of live allocations.
    #[cfg(test)]
    pub fn allocated_sum_for_tests() -> u32 {
        state()
            .lock()
            .map(|g| g.live.values().map(|e| e.allocated).sum())
            .unwrap_or(0)
    }
}

impl Drop for CpuBudgetLease {
    fn drop(&mut self) {
        if let Ok(mut guard) = state().lock() {
            if guard
                .live
                .get(&self.plugin_id)
                .is_some_and(|e| e.generation == self.generation)
            {
                guard.live.remove(&self.plugin_id);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acquire_fits_remaining_when_oversubscribed() {
        CpuBudgetLease::reset_for_tests();
        // Global 100%; A and B each want 60% — classic oversubscription.
        let a = CpuBudgetLease::try_acquire("a", 60, 100).expect("a");
        assert_eq!(a.allocated_rate(), 60);
        assert!(!a.was_throttled());
        let b = CpuBudgetLease::try_acquire("b", 60, 100).expect("b fits remainder");
        assert_eq!(b.allocated_rate(), 40);
        assert!(b.was_throttled());
        assert_eq!(b.requested_rate(), 60);
        assert_eq!(CpuBudgetLease::allocated_sum_for_tests(), 100);
        let err = CpuBudgetLease::try_acquire("c", 10, 100).expect_err("empty pool");
        assert!(err.contains("exhausted"), "{err}");
        drop(a);
        assert_eq!(CpuBudgetLease::allocated_sum_for_tests(), 40);
        let c = CpuBudgetLease::try_acquire("c", 50, 100).expect("after free");
        assert_eq!(c.allocated_rate(), 50);
        drop(b);
        drop(c);
        assert_eq!(CpuBudgetLease::allocated_sum_for_tests(), 0);
    }

    #[test]
    fn same_plugin_id_replaces_allocation() {
        CpuBudgetLease::reset_for_tests();
        let first = CpuBudgetLease::try_acquire("echo", 80, 100).expect("first");
        let second = CpuBudgetLease::try_acquire("echo", 50, 100).expect("replace");
        assert_eq!(CpuBudgetLease::allocated_sum_for_tests(), 50);
        drop(first); // stale generation must not clear the newer lease
        assert_eq!(CpuBudgetLease::allocated_sum_for_tests(), 50);
        drop(second);
        assert_eq!(CpuBudgetLease::allocated_sum_for_tests(), 0);
    }

    #[test]
    fn single_plugin_may_take_full_ceiling() {
        CpuBudgetLease::reset_for_tests();
        let a = CpuBudgetLease::try_acquire("solo", 200, 200).expect("solo");
        assert_eq!(a.allocated_rate(), 200);
        assert!(!a.was_throttled());
    }
}
