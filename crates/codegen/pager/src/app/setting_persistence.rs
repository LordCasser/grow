//! Admission and completion of ordinary settings writes, one in flight per key.
use std::collections::HashMap;

use super::actions::Effect;
use crate::settings::SettingValue;

#[derive(Default)]
pub(crate) struct SettingPersistence {
    dispatch_depth: usize,
    pending: HashMap<&'static str, PendingWrite>,
}

struct PendingWrite {
    rollback: SettingValue,
    // Preserve the first queued rollback (the in-flight value), while replacing
    // its desired value with the latest user selection.
    queued: Option<(SettingValue, SettingValue)>,
}

impl SettingPersistence {
    pub(crate) fn begin_dispatch(&mut self) {
        self.dispatch_depth += 1;
    }

    pub(crate) fn finish_dispatch(&mut self, effects: Vec<Effect>) -> Vec<Effect> {
        self.dispatch_depth -= 1;
        if self.dispatch_depth != 0 {
            return effects;
        }
        effects
            .into_iter()
            .filter_map(|effect| {
                let Effect::PersistSetting {
                    key,
                    value,
                    rollback_value,
                } = effect
                else {
                    return Some(effect);
                };
                if let Some(pending) = self.pending.get_mut(key) {
                    if let Some((queued_value, _)) = &mut pending.queued {
                        *queued_value = value;
                    } else {
                        pending.queued = Some((value, rollback_value));
                    }
                    None
                } else {
                    self.pending.insert(
                        key,
                        PendingWrite {
                            rollback: rollback_value.clone(),
                            queued: None,
                        },
                    );
                    Some(Effect::PersistSetting {
                        key,
                        value,
                        rollback_value,
                    })
                }
            })
            .collect()
    }

    /// Returns the next write, or the authoritative rollback on final failure.
    /// Untracked results (e.g. permission-mode writes) retain their caller's handling.
    pub(crate) fn complete(
        &mut self,
        key: &'static str,
        succeeded: bool,
    ) -> (Option<Effect>, Option<SettingValue>) {
        let Some(pending) = self.pending.remove(key) else {
            return (None, None);
        };
        match pending.queued {
            Some((value, rollback)) => (
                Some(Effect::PersistSetting {
                    key,
                    value,
                    rollback_value: if succeeded {
                        rollback
                    } else {
                        pending.rollback
                    },
                }),
                None,
            ),
            None => (None, (!succeeded).then_some(pending.rollback)),
        }
    }
}
