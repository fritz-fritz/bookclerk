//! Background scan / auto-liberate scheduler.

use std::sync::Arc;
use std::time::Duration;

use tracing::{info, warn};

use crate::api::AppState;

/// Spawn the periodic scan loop (interval from config; 0 disables).
pub fn spawn_scheduler(state: Arc<AppState>) {
    tokio::spawn(async move {
        loop {
            let interval_mins = {
                let cfg = state.config.read().await;
                cfg.library.scan_interval_minutes
            };
            if interval_mins == 0 {
                info!("scheduler disabled (scan_interval_minutes = 0)");
                // Sleep long and re-check in case config is reloaded later.
                tokio::time::sleep(Duration::from_secs(300)).await;
                continue;
            }

            let sleep_for = Duration::from_secs(interval_mins.saturating_mul(60));
            info!(?sleep_for, "scheduler sleeping until next scan");
            tokio::time::sleep(sleep_for).await;

            match state.library.list_accounts() {
                Ok(accounts) => {
                    let enabled: Vec<_> = accounts.into_iter().filter(|a| a.scan_enabled).collect();
                    info!(count = enabled.len(), "scheduled scan tick");
                    for acct in enabled {
                        // Real sync lands in library-sync todo.
                        info!(
                            account = %acct.account_id,
                            marketplace = %acct.marketplace,
                            "would scan account (audible-rs sync pending)"
                        );
                    }
                }
                Err(err) => warn!(error = %err, "failed to list accounts for scheduled scan"),
            }

            let auto = {
                let cfg = state.config.read().await;
                cfg.library.auto_liberate
            };
            if auto {
                info!("auto-liberate enabled — queue wiring pending");
            }
        }
    });
}
