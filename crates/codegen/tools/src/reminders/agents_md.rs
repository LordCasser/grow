//! Project-rule reminders use the normal tool-result envelope, with deferred
//! acknowledgement at finalize_output's last synchronous step.

use std::path::PathBuf;

use crate::types::agents_md_tracker::{AgentsMdDiscovery, AgentsMdTracker, DISCOVERY_TIMEOUT};
use crate::types::output::{ListDirOutput, ReadFileOutput, SearchReplaceOutput, ToolOutput};
use crate::types::resources::{Cwd, DenyReadGlobs, DisplayCwd, SharedResources, Terminal};

pub(crate) async fn discover(
    resources: &SharedResources,
    output: &ToolOutput,
) -> Option<AgentsMdDiscovery> {
    let target = match output {
        ToolOutput::ReadFile(ReadFileOutput::FileContent(file)) => file.absolute_path.clone(),
        ToolOutput::ListDir(ListDirOutput::Content(dir)) => dir.absolute_root_path.clone(),
        ToolOutput::SearchReplace(SearchReplaceOutput::EditsApplied(edit)) => {
            edit.absolute_path.clone()
        }
        // This is the backend's launch cwd, never a path guessed from command text.
        ToolOutput::Bash(bash) => PathBuf::from(&bash.current_dir),
        _ => return None,
    };
    let (tracker, denies, terminal, display_paths) = {
        let res = resources.lock().await;
        (
            res.get::<AgentsMdTracker>()?.clone(),
            res.get::<DenyReadGlobs>()
                .map(|d| d.0.clone())
                .unwrap_or_default(),
            res.get::<Terminal>().map(|t| t.0.clone()),
            res.get::<Cwd>()
                .zip(res.get::<DisplayCwd>())
                .map(|(cwd, display)| (cwd.0.clone(), display.0.clone())),
        )
    };
    let mut targets = vec![target];
    if matches!(output, ToolOutput::Bash(_))
        && let Some(terminal) = terminal
    {
        // Persistent shells report their actual post-command cwd through a
        // structured backend capability. Remote/stateless backends return None.
        if let Ok(Some(cwd)) =
            tokio::time::timeout(DISCOVERY_TIMEOUT, terminal.get_shell_cwd()).await
            && !targets.contains(&cwd)
        {
            targets.push(cwd);
        }
    }
    Some(tracker.check_paths(targets, denies, display_paths).await)
}
