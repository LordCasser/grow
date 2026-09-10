//! Recovery belongs to one unaccepted model step, including session-owned
//! repairs. This shared counter does not own conversation or accounting state.

use std::sync::{Arc, Mutex};
use std::time::Duration;

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
    replay_safe: bool,
}

impl Default for State {
    fn default() -> Self {
        Self {
            attempts: 0,
            max_attempts: None,
            deadline: None,
            replay_safe: true,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct RecoveryBudget(Arc<Mutex<State>>);

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
        let deadline = tokio::time::Instant::now() + idle_timeout.saturating_mul(max_attempts);
        state.deadline = Some(state.deadline.map_or(deadline, |old| old.min(deadline)));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn repairs_share_count_and_cannot_extend_deadline() {
        let budget = RecoveryBudget::default();
        budget.configure(3, Duration::from_secs(10));
        let repaired = budget.clone();
        let original_deadline = budget.deadline();
        assert_eq!(budget.admit().unwrap(), 1);
        tokio::time::advance(Duration::from_secs(5)).await;
        repaired.configure(100, Duration::from_secs(100));
        assert_eq!(repaired.deadline(), original_deadline);
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
}
