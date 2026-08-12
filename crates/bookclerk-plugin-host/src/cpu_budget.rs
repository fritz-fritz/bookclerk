//! Cumulative plugin jail CPU budget (percent of one logical CPU).
//!
//! Each confined guest Spec carries a `cpu_rate_percent`. Concurrent plugin
//! jails share a process-wide pool: Σ(allocated rates) must stay ≤ the
//! cumulative ceiling (`[plugins.jail].cpu_rate_percent` when set, otherwise
//! [`crate::consent::host_cpu_rate_max`]). Reloads that replace the same
//! `plugin_id` count once (upsert).

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use bookclerk_config::Config;

use crate::consent::host_cpu_rate_max;

#[derive(Debug)]
struct Entry {
    generation: u64,
    rate: u32,
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
}

impl CpuBudgetLease {
    /// Reserve `rate` for `plugin_id` if the pool has room.
    ///
    /// An existing allocation for the same id is replaced (reload overlap).
    ///
    /// # Errors
    ///
    /// Returns a human-readable message when `sum(others) + rate` would exceed
    /// `cumulative_ceiling`.
    pub fn try_acquire(
        plugin_id: &str,
        rate: u32,
        cumulative_ceiling: u32,
    ) -> Result<Self, String> {
        let rate = rate.max(1);
        let ceiling = cumulative_ceiling.max(1);
        let mut guard = state()
            .lock()
            .map_err(|_| "plugin CPU budget lock poisoned".to_string())?;
        let mut sum_others: u64 = 0;
        for (id, entry) in &guard.live {
            if id != plugin_id {
                sum_others = sum_others.saturating_add(u64::from(entry.rate));
            }
        }
        let needed = u64::from(rate);
        if sum_others.saturating_add(needed) > u64::from(ceiling) {
            return Err(format!(
                "plugin CPU budget exceeded: need {rate}% of one CPU but \
                 {sum_others}% already allocated (ceiling {ceiling}%; \
                 lower grants, disable plugins, or raise [plugins.jail].cpu_rate_percent)"
            ));
        }
        let generation = NEXT_GEN.fetch_add(1, Ordering::Relaxed);
        guard
            .live
            .insert(plugin_id.to_string(), Entry { generation, rate });
        Ok(Self {
            plugin_id: plugin_id.to_string(),
            generation,
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
            .map(|g| g.live.values().map(|e| e.rate).sum())
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
    fn acquire_respects_cumulative_ceiling_and_releases_on_drop() {
        CpuBudgetLease::reset_for_tests();
        let a = CpuBudgetLease::try_acquire("a", 60, 100).expect("a");
        assert_eq!(CpuBudgetLease::allocated_sum_for_tests(), 60);
        let err = CpuBudgetLease::try_acquire("b", 50, 100).expect_err("over");
        assert!(err.contains("budget exceeded"), "{err}");
        let b = CpuBudgetLease::try_acquire("b", 40, 100).expect("b");
        assert_eq!(CpuBudgetLease::allocated_sum_for_tests(), 100);
        drop(a);
        assert_eq!(CpuBudgetLease::allocated_sum_for_tests(), 40);
        drop(b);
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
}
