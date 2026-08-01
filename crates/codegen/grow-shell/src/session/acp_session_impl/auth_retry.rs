use grow_sampling_types::SentCredential;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AuthRetryDecision {
    UnchargedResubmit {
        resubmit: u32,
    },
    Backoff {
        attempt: u32,
        delay: std::time::Duration,
    },
    Exhausted,
    RunawayGuard {
        resubmits: u32,
    },
}

/// Retry accounting for one uninterrupted BYOK rejection incident.
///
/// A request that carried no credential is not evidence that the user's key
/// was rejected. It receives a separately bounded lane. Sent and unknown
/// credentials consume the strict 1s/2s/4s rejection budget.
pub(super) struct AuthRetrySchedule {
    rejected: u32,
    uncharged: u32,
    started_awake: std::time::Instant,
    started_wall: std::time::SystemTime,
    suspend_resets: u32,
}

impl AuthRetrySchedule {
    pub(super) const MAX_RETRIES: u32 = 3;
    pub(super) const MAX_UNCHARGED_RESUBMITS: u32 = 50;
    const MAX_SUSPEND_RESETS: u32 = 8;
    const SUSPEND_DRIFT_MIN: std::time::Duration = std::time::Duration::from_secs(30);

    pub(super) fn new() -> Self {
        Self {
            rejected: 0,
            uncharged: 0,
            started_awake: std::time::Instant::now(),
            started_wall: std::time::SystemTime::now(),
            suspend_resets: 0,
        }
    }

    pub(super) fn on_recovered_401(&mut self, credential: SentCredential) -> AuthRetryDecision {
        self.reset_after_suspend();
        if credential.is_missing() {
            self.uncharged += 1;
            return if self.uncharged > Self::MAX_UNCHARGED_RESUBMITS {
                AuthRetryDecision::RunawayGuard {
                    resubmits: self.uncharged,
                }
            } else {
                AuthRetryDecision::UnchargedResubmit {
                    resubmit: self.uncharged,
                }
            };
        }

        if self.rejected >= Self::MAX_RETRIES {
            return AuthRetryDecision::Exhausted;
        }
        self.rejected += 1;
        AuthRetryDecision::Backoff {
            attempt: self.rejected,
            delay: std::time::Duration::from_secs(1 << (self.rejected - 1)),
        }
    }

    pub(super) fn reset(&mut self) {
        *self = Self::new();
    }

    fn reset_after_suspend(&mut self) {
        if self.suspend_resets >= Self::MAX_SUSPEND_RESETS {
            return;
        }
        let awake = self.started_awake.elapsed();
        let wall = self.started_wall.elapsed().unwrap_or_default();
        if wall.saturating_sub(awake) < Self::SUSPEND_DRIFT_MIN {
            return;
        }
        let resets = self.suspend_resets + 1;
        *self = Self::new();
        self.suspend_resets = resets;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejected_credentials_use_exact_bounded_schedule() {
        let mut schedule = AuthRetrySchedule::new();
        for (attempt, seconds) in [(1, 1), (2, 2), (3, 4)] {
            assert_eq!(
                schedule.on_recovered_401(SentCredential::Sent),
                AuthRetryDecision::Backoff {
                    attempt,
                    delay: std::time::Duration::from_secs(seconds),
                }
            );
        }
        assert_eq!(
            schedule.on_recovered_401(SentCredential::Unknown),
            AuthRetryDecision::Exhausted
        );
    }

    #[test]
    fn missing_credentials_do_not_charge_rejection_budget() {
        let mut schedule = AuthRetrySchedule::new();
        assert_eq!(
            schedule.on_recovered_401(SentCredential::Missing),
            AuthRetryDecision::UnchargedResubmit { resubmit: 1 }
        );
        assert!(matches!(
            schedule.on_recovered_401(SentCredential::Sent),
            AuthRetryDecision::Backoff { attempt: 1, .. }
        ));
    }
}
