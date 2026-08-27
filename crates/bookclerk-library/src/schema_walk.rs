//! Plan walking for host schema upgrades and last-reversible CLI downgrades.

use crate::error::{LibraryError, Result};
use crate::migrations::{HostMigrationStep, SCHEMA_VERSION};

/// Result of walking a host migration plan from one version to another.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaWalk {
    /// Database version before the walk.
    pub from: i64,
    /// Version the caller asked to reach.
    pub requested_to: i64,
    /// Version the walk can actually reach (may be higher than `requested_to`
    /// when an irreversible step blocks a downgrade).
    pub stopped_at: i64,
    /// Remaining `up` steps in order (empty when rolling back).
    pub ups: Vec<HostMigrationStep>,
    /// `down` steps newest-first (empty when upgrading).
    pub downs: Vec<HostMigrationStep>,
    /// True when a missing `down` stopped a rollback short of `requested_to`.
    pub blocked: bool,
}

impl SchemaWalk {
    /// True when no DDL remains to apply.
    #[must_use]
    pub fn is_noop(&self) -> bool {
        self.ups.is_empty() && self.downs.is_empty()
    }
}

/// Plans ups or downs from `from` toward `to` using `plan`.
///
/// Downgrades walk newest-first and stop at the last reversible version when
/// the next step has `down: None`. Versions newer than the compiled plan
/// cannot be reversed by this binary.
///
/// # Errors
///
/// Returns [`LibraryError::Schema`] when `to` is below zero or `from` is ahead
/// of every compiled step (unknown newer schema).
pub fn plan_schema_walk(plan: &[HostMigrationStep], from: i64, to: i64) -> Result<SchemaWalk> {
    if to < 0 {
        return Err(LibraryError::Schema(
            "cannot migrate to a negative schema version".into(),
        ));
    }
    let max_plan = plan.iter().map(|s| s.version).max().unwrap_or(0);
    if from > max_plan {
        return Err(LibraryError::Schema(format!(
            "database schema version {from} is newer than this binary ({max_plan}); \
             run `bookclerk db downgrade` or restore a snapshot"
        )));
    }
    if to > max_plan {
        return Err(LibraryError::Schema(format!(
            "target schema version {to} is newer than this binary ({max_plan})"
        )));
    }

    if to >= from {
        let ups: Vec<HostMigrationStep> = plan
            .iter()
            .copied()
            .filter(|step| step.version > from && step.version <= to)
            .collect();
        let stopped_at = ups.last().map(|s| s.version).unwrap_or(from);
        return Ok(SchemaWalk {
            from,
            requested_to: to,
            stopped_at,
            ups,
            downs: Vec::new(),
            blocked: false,
        });
    }

    let mut downs = Vec::new();
    let mut current = from;
    let mut blocked = false;
    while current > to {
        let Some(step) = plan.iter().find(|s| s.version == current).copied() else {
            return Err(LibraryError::Schema(format!(
                "database schema version {current} is not in this binary's plan; restore a snapshot"
            )));
        };
        if step.down.is_none() {
            blocked = true;
            break;
        }
        downs.push(step);
        current -= 1;
    }
    Ok(SchemaWalk {
        from,
        requested_to: to,
        stopped_at: current,
        ups: Vec::new(),
        downs,
        blocked,
    })
}

/// Walks the compiled host plan from `from` to this binary's [`SCHEMA_VERSION`].
///
/// # Errors
///
/// Propagates [`plan_schema_walk`] failures.
pub fn plan_downgrade_to_binary(plan: &[HostMigrationStep], from: i64) -> Result<SchemaWalk> {
    plan_schema_walk(plan, from, SCHEMA_VERSION)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(version: i64, down: Option<&'static str>) -> HostMigrationStep {
        HostMigrationStep {
            version,
            canonical: "UP",
            down,
            introduced_in: "0.1.0",
        }
    }

    #[test]
    fn upgrade_selects_intervening_ups() {
        let plan = [step(1, None), step(2, Some("D2")), step(3, Some("D3"))];
        let walk = plan_schema_walk(&plan, 1, 3).unwrap();
        assert_eq!(
            walk.ups.iter().map(|s| s.version).collect::<Vec<_>>(),
            vec![2, 3]
        );
        assert!(walk.downs.is_empty());
        assert!(!walk.blocked);
        assert_eq!(walk.stopped_at, 3);
    }

    #[test]
    fn downgrade_walks_newest_first_until_irreversible() {
        let plan = [step(1, None), step(2, Some("D2")), step(3, Some("D3"))];
        let walk = plan_schema_walk(&plan, 3, 1).unwrap();
        assert_eq!(
            walk.downs.iter().map(|s| s.version).collect::<Vec<_>>(),
            vec![3, 2]
        );
        assert_eq!(walk.stopped_at, 1);
        assert!(!walk.blocked);
    }

    #[test]
    fn downgrade_stops_at_last_reversible_when_blocked() {
        let plan = [step(1, None), step(2, Some("D2")), step(3, Some("D3"))];
        let walk = plan_schema_walk(&plan, 3, 0).unwrap();
        assert_eq!(
            walk.downs.iter().map(|s| s.version).collect::<Vec<_>>(),
            vec![3, 2]
        );
        assert_eq!(walk.stopped_at, 1);
        assert!(walk.blocked);
        assert_eq!(walk.requested_to, 0);
    }

    #[test]
    fn unknown_newer_schema_fails_closed() {
        let plan = [step(1, None)];
        let err = plan_schema_walk(&plan, 2, 1).unwrap_err();
        assert!(err.to_string().contains("newer than this binary"), "{err}");
    }

    #[test]
    fn noop_when_already_at_target() {
        let plan = [step(1, None)];
        let walk = plan_schema_walk(&plan, 1, 1).unwrap();
        assert!(walk.is_noop());
        assert_eq!(walk.stopped_at, 1);
    }
}
