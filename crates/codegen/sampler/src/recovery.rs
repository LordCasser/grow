//! Recovery belongs to one unaccepted model step, including session-owned
//! repairs. This shared counter does not own conversation or accounting state.

use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use sampling_types::SamplingError;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OutputDelivery {
    /// Unknown and append-only consumers cannot discard published frames.
    #[default]
    Irreversible,
    Buffered,
    Retractable,
}

impl OutputDelivery {
    pub(crate) fn permits_resampling(self, published: bool) -> bool {
        !published || self != Self::Irreversible
    }
}

#[derive(Debug)]
struct State {
    attempts: u32,
    max_attempts: Option<u32>,
    deadline: Option<tokio::time::Instant>,
    deadline_ms: Option<u64>,
    replay_safe: bool,
}

impl Default for State {
    fn default() -> Self {
        Self {
            attempts: 0,
            max_attempts: None,
            deadline: None,
            deadline_ms: None,
            replay_safe: true,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct RecoveryBudget(Arc<Mutex<State>>);

/// Process-independent recovery state for one logical sampling attempt.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct RecoveryBudgetSnapshot {
    pub attempts: u32,
    pub max_attempts: Option<u32>,
    /// Absolute UNIX timestamp in milliseconds.
    pub deadline_ms: Option<u64>,
    pub replay_safe: bool,
}

impl RecoveryBudget {
    /// Initialize once; changing route/config during recovery cannot enlarge
    /// the allowance or move its absolute deadline forward.
    pub(crate) fn configure(&self, max_attempts: u32, idle_timeout: Duration) {
        let mut state = self.0.lock().expect("recovery budget poisoned");
        let max_attempts = max_attempts.max(1);
        state.max_attempts = Some(
            state
                .max_attempts
                .map_or(max_attempts, |old| old.min(max_attempts)),
        );
        let timeout = idle_timeout.saturating_mul(max_attempts);
        let deadline = tokio::time::Instant::now() + timeout;
        state.deadline = Some(state.deadline.map_or(deadline, |old| old.min(deadline)));
        let wall_deadline = unix_time_millis().saturating_add(duration_millis(timeout));
        state.deadline_ms = Some(
            state
                .deadline_ms
                .map_or(wall_deadline, |old| old.min(wall_deadline)),
        );
    }

    /// Capture the exact allowance and absolute deadline for process recovery.
    pub fn snapshot(&self) -> RecoveryBudgetSnapshot {
        let state = self.0.lock().expect("recovery budget poisoned");
        RecoveryBudgetSnapshot {
            attempts: state.attempts,
            max_attempts: state.max_attempts,
            deadline_ms: state.deadline_ms,
            replay_safe: state.replay_safe,
        }
    }

    /// Reconstruct monotonic deadline state from its persisted wall-clock bound.
    pub fn restore(snapshot: RecoveryBudgetSnapshot) -> Result<Self, String> {
        let deadline = match (snapshot.max_attempts, snapshot.deadline_ms) {
            (None, None) => None,
            (Some(_), Some(deadline_ms)) => {
                let remaining_ms = deadline_ms.saturating_sub(unix_time_millis());
                let remaining = Duration::from_millis(remaining_ms);
                Some(
                    tokio::time::Instant::now()
                        .checked_add(remaining)
                        .ok_or_else(|| {
                            "recovery deadline is outside the monotonic clock range".to_owned()
                        })?,
                )
            }
            _ => return Err("recovery snapshot has mismatched attempt and deadline bounds".into()),
        };
        Ok(Self(Arc::new(Mutex::new(State {
            attempts: snapshot.attempts,
            max_attempts: snapshot.max_attempts,
            deadline,
            deadline_ms: snapshot.deadline_ms,
            replay_safe: snapshot.replay_safe,
        }))))
    }

    pub(crate) fn admit(&self) -> Result<u32, SamplingError> {
        let mut state = self.0.lock().expect("recovery budget poisoned");
        if state
            .deadline
            .is_some_and(|deadline| tokio::time::Instant::now() >= deadline)
        {
            return Err(SamplingError::Lifecycle(
                "logical sampling deadline exceeded".into(),
            ));
        }
        if state.attempts > 0 && !state.replay_safe {
            return Err(SamplingError::Lifecycle(
                "recovery prohibited after irreversible output or provider-side effects".into(),
            ));
        }
        if state.attempts >= state.max_attempts.unwrap_or(1) {
            return Err(SamplingError::Lifecycle(format!(
                "logical sampling exhausted after {} admitted provider attempts",
                state.attempts
            )));
        }
        state.attempts += 1;
        Ok(state.attempts)
    }

    pub fn can_recover(&self) -> bool {
        let state = self.0.lock().expect("recovery budget poisoned");
        state.replay_safe
            && state.attempts < state.max_attempts.unwrap_or(1)
            && !state
                .deadline
                .is_some_and(|deadline| tokio::time::Instant::now() >= deadline)
    }

    pub(crate) fn prohibit_replay(&self) {
        self.0.lock().expect("recovery budget poisoned").replay_safe = false;
    }

    /// Session-owned repairs use the same absolute deadline. Dropping this
    /// future cancels the wait; it never extends or spends provider admission.
    pub async fn wait_before_retry(&self, delay: Duration) -> Result<(), SamplingError> {
        if !self.can_recover() {
            return Err(SamplingError::Lifecycle(
                "logical sampling recovery is closed".into(),
            ));
        }
        tokio::time::sleep_until((tokio::time::Instant::now() + delay).min(self.deadline())).await;
        if !self.can_recover() {
            return Err(SamplingError::Lifecycle(
                "logical sampling recovery deadline reached".into(),
            ));
        }
        Ok(())
    }

    pub fn attempts(&self) -> u32 {
        self.0.lock().expect("recovery budget poisoned").attempts
    }

    pub(crate) fn remaining(&self) -> u32 {
        let state = self.0.lock().expect("recovery budget poisoned");
        if state
            .deadline
            .is_some_and(|deadline| tokio::time::Instant::now() >= deadline)
        {
            return 0;
        }
        state
            .max_attempts
            .unwrap_or(1)
            .saturating_sub(state.attempts)
    }

    pub(crate) fn deadline(&self) -> tokio::time::Instant {
        self.0
            .lock()
            .expect("recovery budget poisoned")
            .deadline
            .expect("configured recovery budget")
    }
}

fn unix_time_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

fn duration_millis(duration: Duration) -> u64 {
    duration.as_millis().try_into().unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn repairs_share_count_and_cannot_extend_deadline() {
        let budget = RecoveryBudget::default();
        budget.configure(3, Duration::from_secs(10));
        let repaired = budget.clone();
        let original_deadline = budget.deadline();
        let original_deadline_ms = budget.snapshot().deadline_ms;
        assert_eq!(budget.admit().unwrap(), 1);
        tokio::time::advance(Duration::from_secs(5)).await;
        repaired.configure(100, Duration::from_secs(100));
        assert_eq!(repaired.deadline(), original_deadline);
        assert_eq!(repaired.snapshot().deadline_ms, original_deadline_ms);
        assert_eq!(repaired.admit().unwrap(), 2);
        tokio::time::advance(Duration::from_secs(26)).await;
        assert!(budget.admit().is_err());
        assert_eq!(budget.attempts(), 2);
    }

    #[tokio::test]
    async fn disabled_recovery_still_allows_only_the_initial_attempt() {
        let budget = RecoveryBudget::default();
        budget.configure(0, Duration::from_secs(10));
        assert_eq!(budget.admit().unwrap(), 1);
        assert!(budget.admit().is_err());
    }

    #[test]
    fn all_published_frames_obey_delivery_capability() {
        assert!(OutputDelivery::Irreversible.permits_resampling(false));
        assert!(!OutputDelivery::Irreversible.permits_resampling(true));
        assert!(OutputDelivery::Buffered.permits_resampling(true));
        assert!(OutputDelivery::Retractable.permits_resampling(true));
    }

    #[test]
    fn irreversible_output_sticks_until_recovery_is_rejected() {
        let budget = RecoveryBudget::default();
        budget.configure(3, Duration::from_secs(10));
        assert!(budget.admit().is_ok());
        assert!(budget.can_recover());

        budget.prohibit_replay();
        assert!(!budget.can_recover());
        assert!(matches!(
            budget.admit(),
            Err(SamplingError::Lifecycle(message))
                if message.contains("irreversible output")
        ));
    }

    #[test]
    fn snapshot_restore_preserves_attempt_count_and_deadline() {
        let budget = RecoveryBudget::default();
        budget.configure(3, Duration::from_secs(30));
        assert_eq!(budget.admit().unwrap(), 1);

        let snapshot = budget.snapshot();
        let restored = RecoveryBudget::restore(snapshot).unwrap();
        assert_eq!(restored.snapshot(), snapshot);
        assert_eq!(restored.attempts(), 1);
        assert_eq!(restored.admit().unwrap(), 2);
    }

    #[test]
    fn restoring_an_expired_snapshot_keeps_its_deadline_expired() {
        let snapshot = RecoveryBudgetSnapshot {
            attempts: 1,
            max_attempts: Some(3),
            deadline_ms: Some(0),
            replay_safe: true,
        };
        let restored = RecoveryBudget::restore(snapshot).unwrap();

        assert!(!restored.can_recover());
        assert!(matches!(
            restored.admit(),
            Err(SamplingError::Lifecycle(message)) if message.contains("deadline")
        ));
        assert_eq!(restored.snapshot(), snapshot);
    }

    #[test]
    fn restoring_irreversible_budget_does_not_reenable_replay() {
        let budget = RecoveryBudget::default();
        budget.configure(3, Duration::from_secs(30));
        assert_eq!(budget.admit().unwrap(), 1);
        budget.prohibit_replay();

        let snapshot = budget.snapshot();
        let restored = RecoveryBudget::restore(snapshot).unwrap();
        assert_eq!(restored.snapshot(), snapshot);
        assert!(!restored.can_recover());
        assert!(matches!(
            restored.admit(),
            Err(SamplingError::Lifecycle(message)) if message.contains("irreversible output")
        ));
    }

    #[test]
    fn unconfigured_budget_can_be_snapshotted_and_restored() {
        let budget = RecoveryBudget::default();
        let snapshot = budget.snapshot();
        assert_eq!(
            snapshot,
            RecoveryBudgetSnapshot {
                attempts: 0,
                max_attempts: None,
                deadline_ms: None,
                replay_safe: true,
            }
        );

        let restored = RecoveryBudget::restore(snapshot).unwrap();
        assert_eq!(restored.snapshot(), snapshot);
        assert_eq!(restored.admit().unwrap(), 1);
        assert!(restored.admit().is_err());
    }
}
