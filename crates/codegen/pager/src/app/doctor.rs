//! Owned UI evidence and bounded execution for interactive diagnostics.
use crate::app::actions::DoctorPlanningOutcome;
use crate::notifications::protocol::NotificationProtocol;
use crate::notifications::{NotificationCondition, NotificationMethod};

#[derive(Debug)]
pub struct DoctorReportInput {
    pub workspace: std::path::PathBuf,
    pub fullscreen_active: bool,
    pub kitty_flags_pushed: bool,
    pub xtversion: Option<String>,
    pub notification_method: NotificationMethod,
    pub notification_protocol: NotificationProtocol,
    pub notification_condition: NotificationCondition,
}

impl DoctorReportInput {
    pub fn collect(
        &self,
        terminal: &crate::terminal::TerminalContext,
    ) -> crate::diagnostics::DiagnosticReport {
        crate::slash::commands::doctor::DoctorCommand::report_for_terminal(
            terminal,
            crate::diagnostics::probes::TuiProbeEvidence {
                fullscreen_active: self.fullscreen_active,
                kitty_flags_pushed: self.kitty_flags_pushed,
                xtversion: self.xtversion.as_deref(),
            },
            crate::diagnostics::TuiRuntimeRequest {
                workspace: &self.workspace,
                notification_method: self.notification_method,
                notification_protocol: self.notification_protocol,
                notification_condition: self.notification_condition,
            },
        )
    }
}

pub(crate) async fn execute(
    permit: tokio::sync::OwnedSemaphorePermit,
    work: impl FnOnce() -> Result<DoctorPlanningOutcome, String> + Send + 'static,
) -> Result<DoctorPlanningOutcome, String> {
    tokio::task::spawn_blocking(move || {
        // The actual blocking job owns the permit, including after waiter cancellation.
        let _permit = permit;
        work()
    })
    .await
    .map_err(|error| format!("Could not check diagnostics: {error}"))?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn doctor_worker_keeps_permit_until_blocking_work_exits() {
        let gate = std::sync::Arc::new(tokio::sync::Semaphore::new(1));
        let permit = gate.clone().try_acquire_owned().unwrap();
        let (started, ready) = tokio::sync::oneshot::channel();
        let (release, wait) = std::sync::mpsc::channel();
        let job = tokio::spawn(execute(permit, move || {
            started.send(()).unwrap();
            wait.recv().unwrap();
            Ok(DoctorPlanningOutcome::Report("done".into()))
        }));
        tokio::time::timeout(std::time::Duration::from_secs(5), ready)
            .await
            .unwrap()
            .unwrap();
        // The async runtime remains responsive while the actual collector waits.
        assert!(gate.clone().try_acquire_owned().is_err());
        job.abort();
        let _ = job.await;
        assert!(
            gate.clone().try_acquire_owned().is_err(),
            "cancelling waiter must not free a running worker slot"
        );
        release.send(()).unwrap();
        let _permit = tokio::time::timeout(std::time::Duration::from_secs(5), gate.acquire_owned())
            .await
            .unwrap()
            .unwrap();
    }

    #[tokio::test]
    async fn doctor_worker_panic_releases_slot_and_reports_failure() {
        let gate = std::sync::Arc::new(tokio::sync::Semaphore::new(1));
        let result = execute(gate.clone().try_acquire_owned().unwrap(), || {
            panic!("synthetic collector panic")
        })
        .await;
        assert!(result.unwrap_err().contains("synthetic collector panic"));
        assert_eq!(gate.available_permits(), 1);
    }
}
