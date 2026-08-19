//! Canonical runtime-context snapshot rendered as a durable user-role message.
//!
//! Runtime facts have one owner. Agent definitions cannot replace this shape,
//! and skills, project instructions, MCP catalogs, and other dynamic domains
//! publish their own Timeline-backed messages instead of being copied here.

use chrono::NaiveDate;
use std::path::PathBuf;

/// Date format used by the runtime snapshot and rollover reminders.
pub const RUNTIME_DATE_FORMAT: &str = "%Y-%m-%d";

/// Per-repository character cap applied to VCS status at render time.
pub const VCS_STATUS_CHARACTER_LIMIT: usize = 10_000;

/// Trim, drop-if-empty, and cap a VCS status string on a UTF-8 boundary.
pub fn normalize_vcs_status(status: &str) -> Option<String> {
    let status = status.trim();
    if status.is_empty() {
        return None;
    }
    if status.len() <= VCS_STATUS_CHARACTER_LIMIT {
        return Some(status.to_string());
    }
    let mut end = VCS_STATUS_CHARACTER_LIMIT;
    while !status.is_char_boundary(end) {
        end -= 1;
    }
    let mut truncated = &status[..end];
    if let Some(nl) = truncated.rfind('\n')
        && nl > 0
    {
        truncated = &truncated[..nl];
    }
    Some(format!("{truncated}\n\n... (VCS status truncated)"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VcsSnapshotKind {
    Git,
    Jujutsu,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VcsSnapshot {
    pub kind: VcsSnapshotKind,
    pub status: String,
}

/// The complete runtime context visible to the model at session start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeContextSnapshot {
    pub workspace_path: PathBuf,
    pub os_version: String,
    pub shell: String,
    pub today_local: NaiveDate,
    pub vcs: Option<VcsSnapshot>,
}

impl RuntimeContextSnapshot {
    /// Render the one canonical model-facing representation.
    pub fn render(&self) -> String {
        let mut out = format!(
            "<runtime_context>\n\
             <user_info>\n\
             OS Version: {}\n\
             Shell: {}\n\
             Workspace Path: {}\n\
             Today's date: {}\n\
             Note: Prefer using relative paths over absolute paths as tool call args when possible.\n\
             </user_info>",
            self.os_version,
            self.shell,
            self.workspace_path.display(),
            self.today_local.format(RUNTIME_DATE_FORMAT),
        );
        if let Some(vcs) = &self.vcs
            && let Some(status) = normalize_vcs_status(&vcs.status)
        {
            let (tag, description) = match vcs.kind {
                VcsSnapshotKind::Git => (
                    "git_status",
                    "This is the git status at the start of the conversation. It is a snapshot in time and will not update during the conversation.",
                ),
                VcsSnapshotKind::Jujutsu => (
                    "jj_status",
                    "This is the Jujutsu (jj) status at the start of the conversation. Use jj commands instead of git; the working-copy commit has no staging area.",
                ),
            };
            out.push_str(&format!("\n\n<{tag}>\n{description}\n{status}\n</{tag}>"));
        }
        out.push_str("\n</runtime_context>");
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(vcs: Option<VcsSnapshot>) -> RuntimeContextSnapshot {
        RuntimeContextSnapshot {
            workspace_path: PathBuf::from("/workspace"),
            os_version: "darwin 25.0".into(),
            shell: "/bin/zsh".into(),
            today_local: NaiveDate::from_ymd_opt(2026, 8, 18).unwrap(),
            vcs,
        }
    }

    #[test]
    fn renders_the_canonical_runtime_snapshot() {
        let rendered = snapshot(None).render();
        assert!(rendered.starts_with("<runtime_context>\n<user_info>"));
        assert!(rendered.contains("OS Version: darwin 25.0"));
        assert!(rendered.contains("Workspace Path: /workspace"));
        assert!(rendered.contains("Today's date: 2026-08-18"));
        assert!(rendered.ends_with("</user_info>\n</runtime_context>"));
    }

    #[test]
    fn renders_git_and_jujutsu_as_distinct_snapshots() {
        let git = snapshot(Some(VcsSnapshot {
            kind: VcsSnapshotKind::Git,
            status: "## main\n M src/lib.rs".into(),
        }))
        .render();
        assert!(git.contains("<git_status>"));
        assert!(!git.contains("<jj_status>"));

        let jj = snapshot(Some(VcsSnapshot {
            kind: VcsSnapshotKind::Jujutsu,
            status: "Working copy changes:".into(),
        }))
        .render();
        assert!(jj.contains("<jj_status>"));
        assert!(!jj.contains("<git_status>"));
    }

    #[test]
    fn normalize_vcs_status_is_bounded_and_utf8_safe() {
        let mut status = String::from("## main\n");
        while status.len() <= VCS_STATUS_CHARACTER_LIMIT {
            status.push_str(" M 路径/file.rs\n");
        }
        let normalized = normalize_vcs_status(&status).unwrap();
        assert!(normalized.is_char_boundary(normalized.len()));
        assert!(normalized.ends_with("... (VCS status truncated)"));
    }
}
