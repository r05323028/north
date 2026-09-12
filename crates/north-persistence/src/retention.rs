//! Bounded retention for allowlisted ephemeral runtime telemetry.
//!
//! Only tables on the explicit ephemeral allowlist may be swept. The 0.1.0
//! allowlist contains exactly clarification_activities; every other class is
//! durable and unreachable from this module by construction, because the
//! persistence surface exposes a single named sweep with fixed SQL and no
//! generic table deletion.

use crate::{AuthStore, PersistenceError};
use std::{error::Error, fmt};

/// Default retention window for activity telemetry (7 days).
pub const DEFAULT_RETENTION_SECONDS: i64 = 7 * 24 * 60 * 60;
/// Default sweep cadence in seconds.
pub const DEFAULT_RETENTION_SWEEP_SECONDS: u64 = 60;
/// Default per-pass deletion bound.
pub const DEFAULT_RETENTION_BATCH_SIZE: i64 = 500;
/// Maximum per-pass deletion bound accepted by configuration.
pub const MAX_RETENTION_BATCH_SIZE: i64 = 1000;
/// Default maximum sweep passes in one drain cycle (10,000 rows at defaults).
pub const DEFAULT_RETENTION_MAX_PASSES: u32 = 20;
/// Maximum sweep passes accepted by configuration.
pub const MAX_RETENTION_MAX_PASSES: u32 = 1000;

/// Validated retention settings for ephemeral telemetry.
///
/// Fields are private and construction goes through RetentionConfig::new, so
/// an invalid retention configuration cannot exist and cannot run a sweep.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetentionConfig {
    retention_seconds: i64,
    sweep_interval_seconds: u64,
    batch_size: i64,
    max_passes_per_cycle: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetentionConfigError {
    InvalidRetentionSeconds,
    InvalidSweepInterval,
    InvalidBatchSize,
    InvalidMaxPasses,
}

impl fmt::Display for RetentionConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRetentionSeconds => {
                f.write_str("retention window must be at least one second")
            }
            Self::InvalidSweepInterval => {
                f.write_str("retention sweep interval must be at least one second")
            }
            Self::InvalidBatchSize => write!(
                f,
                "retention batch size must be between 1 and {MAX_RETENTION_BATCH_SIZE}"
            ),
            Self::InvalidMaxPasses => write!(
                f,
                "retention max passes per cycle must be between 1 and {MAX_RETENTION_MAX_PASSES}"
            ),
        }
    }
}

impl Error for RetentionConfigError {}

impl Default for RetentionConfig {
    fn default() -> Self {
        Self {
            retention_seconds: DEFAULT_RETENTION_SECONDS,
            sweep_interval_seconds: DEFAULT_RETENTION_SWEEP_SECONDS,
            batch_size: DEFAULT_RETENTION_BATCH_SIZE,
            max_passes_per_cycle: DEFAULT_RETENTION_MAX_PASSES,
        }
    }
}

impl RetentionConfig {
    /// Build validated retention settings; invalid values fail explicitly.
    pub fn new(
        retention_seconds: i64,
        sweep_interval_seconds: u64,
        batch_size: i64,
        max_passes_per_cycle: u32,
    ) -> Result<Self, RetentionConfigError> {
        if retention_seconds < 1 {
            return Err(RetentionConfigError::InvalidRetentionSeconds);
        }
        if sweep_interval_seconds < 1 {
            return Err(RetentionConfigError::InvalidSweepInterval);
        }
        if !(1..=MAX_RETENTION_BATCH_SIZE).contains(&batch_size) {
            return Err(RetentionConfigError::InvalidBatchSize);
        }
        if !(1..=MAX_RETENTION_MAX_PASSES).contains(&max_passes_per_cycle) {
            return Err(RetentionConfigError::InvalidMaxPasses);
        }
        Ok(Self {
            retention_seconds,
            sweep_interval_seconds,
            batch_size,
            max_passes_per_cycle,
        })
    }

    pub fn retention_seconds(&self) -> i64 {
        self.retention_seconds
    }

    pub fn sweep_interval_seconds(&self) -> u64 {
        self.sweep_interval_seconds
    }

    pub fn batch_size(&self) -> i64 {
        self.batch_size
    }

    pub fn max_passes_per_cycle(&self) -> u32 {
        self.max_passes_per_cycle
    }
}

/// Outcome of one bounded drain cycle: every individual sweep is batch-sized,
/// while a cycle may run several passes so a backlog can actually recover.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetentionDrain {
    /// Total rows deleted across the cycle.
    pub deleted: u64,
    /// Number of bounded sweep passes executed.
    pub passes: u32,
    /// True only when expired rows still remain after the configured pass
    /// budget was exhausted; the next cycle continues where this one stopped.
    /// A cycle that consumed the whole backlog with its final pass reports
    /// false even at exact capacity.
    pub drain_limit_reached: bool,
}

impl AuthStore {
    /// Delete at most the configured batch of expired activity telemetry.
    ///
    /// Ordering is deterministic, locks are row-scoped with FOR UPDATE SKIP
    /// LOCKED so concurrent server instances delete disjoint rows, and the
    /// statement is bounded by the configured batch size. This is the only
    /// deletion primitive in the persistence surface.
    pub async fn purge_expired_clarification_activities(&self) -> Result<u64, PersistenceError> {
        let deleted = sqlx::query(
            "WITH expired AS (
                 SELECT id
                 FROM clarification_activities
                 WHERE expires_at <= CURRENT_TIMESTAMP
                 ORDER BY expires_at ASC, id ASC
                 LIMIT $1
                 FOR UPDATE SKIP LOCKED
             )
             DELETE FROM clarification_activities AS activities
             USING expired
             WHERE activities.id = expired.id",
        )
        .bind(self.retention.batch_size())
        .execute(&self.pool)
        .await?;
        Ok(deleted.rows_affected())
    }

    /// Bounded probe: whether any expired activity rows remain.
    ///
    /// Uses an indexed LIMIT 1 existence scan, so the worker path never runs an
    /// exact unbounded COUNT(*) over a large expired backlog. Exact backlog
    /// size is deliberately an observability concern, not retention progress.
    pub async fn expired_activity_remains(&self) -> Result<bool, PersistenceError> {
        let row: Option<i32> = sqlx::query_scalar(
            "SELECT 1
             FROM clarification_activities
             WHERE expires_at <= CURRENT_TIMESTAMP
             LIMIT 1",
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.is_some())
    }

    /// Drain expired activity telemetry for one scheduler cycle.
    ///
    /// Each pass is an independent bounded statement (no large transaction).
    /// The cycle stops when a pass deleted fewer rows than the batch bound, when
    /// no expired rows remain, or when the configured pass bound is reached with
    /// rows still eligible. Both the pass count and the post-pass backlog probe
    /// are bounded, so maintenance work per cycle is strictly bounded regardless
    /// of backlog size.
    pub async fn drain_expired_clarification_activities(
        &self,
    ) -> Result<RetentionDrain, PersistenceError> {
        let batch_size = u64::try_from(self.retention.batch_size()).unwrap_or(u64::MAX);
        let mut deleted = 0_u64;
        let mut passes = 0_u32;
        loop {
            let swept = self.purge_expired_clarification_activities().await?;
            deleted = deleted.saturating_add(swept);
            passes = passes.saturating_add(1);
            if swept < batch_size {
                return Ok(RetentionDrain {
                    deleted,
                    passes,
                    drain_limit_reached: false,
                });
            }
            if !self.expired_activity_remains().await? {
                return Ok(RetentionDrain {
                    deleted,
                    passes,
                    drain_limit_reached: false,
                });
            }
            if passes >= self.retention.max_passes_per_cycle() {
                return Ok(RetentionDrain {
                    deleted,
                    passes,
                    drain_limit_reached: true,
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retention_configuration_rejects_invalid_values() {
        assert_eq!(
            RetentionConfig::new(0, 60, 500, 20),
            Err(RetentionConfigError::InvalidRetentionSeconds)
        );
        assert_eq!(
            RetentionConfig::new(604_800, 0, 500, 20),
            Err(RetentionConfigError::InvalidSweepInterval)
        );
        assert_eq!(
            RetentionConfig::new(604_800, 60, 0, 20),
            Err(RetentionConfigError::InvalidBatchSize)
        );
        assert_eq!(
            RetentionConfig::new(604_800, 60, MAX_RETENTION_BATCH_SIZE + 1, 20),
            Err(RetentionConfigError::InvalidBatchSize)
        );
        assert_eq!(
            RetentionConfig::new(604_800, 60, 500, 0),
            Err(RetentionConfigError::InvalidMaxPasses)
        );
        assert_eq!(
            RetentionConfig::new(604_800, 60, 500, MAX_RETENTION_MAX_PASSES + 1),
            Err(RetentionConfigError::InvalidMaxPasses)
        );
        let valid =
            RetentionConfig::new(604_800, 60, MAX_RETENTION_BATCH_SIZE, 5).expect("valid bounds");
        assert_eq!(valid.retention_seconds(), 604_800);
        assert_eq!(valid.sweep_interval_seconds(), 60);
        assert_eq!(valid.batch_size(), MAX_RETENTION_BATCH_SIZE);
        assert_eq!(valid.max_passes_per_cycle(), 5);
        assert_eq!(
            RetentionConfig::default(),
            RetentionConfig::new(
                DEFAULT_RETENTION_SECONDS,
                DEFAULT_RETENTION_SWEEP_SECONDS,
                DEFAULT_RETENTION_BATCH_SIZE,
                DEFAULT_RETENTION_MAX_PASSES
            )
            .expect("defaults are valid")
        );
    }
}
