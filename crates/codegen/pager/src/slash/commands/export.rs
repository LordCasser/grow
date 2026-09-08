//! `/export [filename]` -- export the current conversation transcript as Markdown.
//!
//! Omit the filename (or pass empty) to copy the full transcript to the clipboard.
//! With a filename, writes a UTF-8 .md file (supports ~ expansion, paths with spaces,
//! and parent directory creation).
//!
//! Pager-side only (local TUI execution). Follows the exact patterns from
//! `copy.rs`, `share.rs`, and the SlashCommand trait in `command.rs`.

use std::path::{Path, PathBuf};

use crate::app::actions::Action;
use crate::slash::command::{AppCtx, ArgItem, CommandExecCtx, CommandResult, SlashCommand};

/// Export the current conversation to a file or clipboard.
pub struct ExportCommand;

impl SlashCommand for ExportCommand {
    fn name(&self) -> &str {
        "export"
    }

    fn description(&self) -> &str {
        "Export the current conversation to a file or clipboard"
    }

    fn session_scoped(&self) -> bool {
        true
    }

    fn usage(&self) -> &str {
        "/export [filename]"
    }

    fn takes_args(&self) -> bool {
        true
    }

    fn args_required(&self) -> bool {
        false
    }

    fn arg_placeholder(&self) -> Option<&str> {
        Some("[filename]")
    }

    fn suggest_args(&self, ctx: &AppCtx, args_query: &str) -> Option<Vec<ArgItem>> {
        let items = list_path_completions(ctx.cwd, args_query);
        if items.is_empty() { None } else { Some(items) }
    }

    fn run(&self, ctx: &mut CommandExecCtx, args: &str) -> CommandResult {
        if ctx.session_id.is_none() {
            return CommandResult::Error("No active session to export".to_string());
        }

        let trimmed = args.trim();
        let file_path: Option<PathBuf> = if trimmed.is_empty() {
            None
        } else {
            Some(PathBuf::from(trimmed))
        };

        CommandResult::Action(Action::ExportConversation { file_path })
    }
}

/// List filesystem entries for path completion in the `/export` args dropdown.
///
/// Parses the typed query to extract a directory prefix, lists its contents,
/// and returns `ArgItem`s. Directories get a trailing `/` in `insert_text` so
/// the dropdown stays open for drill-down (same trick `/model` uses with
/// trailing space for effort chaining).
///
/// The `SlashController` handles nucleo fuzzy ranking on the returned items
/// automatically — we just provide the candidates.
///
/// Synchronous directory listing inspects at most 1000 iterator results,
/// including hidden entries and errors. This bounds enumeration work, not
/// filesystem latency; returned suggestions retain their separate 100-item cap.
fn list_path_completions(cwd: &Path, query: &str) -> Vec<ArgItem> {
    let trimmed = query.trim_start();
    if trimmed.is_empty() {
        return Vec::new();
    }

    let input_path = PathBuf::from(shellexpand::tilde(trimmed).as_ref());

    // Determine which directory to list and what prefix the user has typed.
    // If the input ends with `/`, list that directory's contents.
    // Otherwise, list the parent and let nucleo filter by the partial filename.
    let (dir_to_list, typed_prefix) = if trimmed.ends_with('/') {
        (input_path.clone(), trimmed.to_string())
    } else {
        let parent = input_path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(cwd);
        // Reconstruct the user's prefix up to the last `/` (preserving ~).
        let prefix = match trimmed.rfind('/') {
            Some(pos) => &trimmed[..=pos],
            None => "",
        };
        (parent.to_path_buf(), prefix.to_string())
    };

    // Resolve relative paths against cwd.
    let resolved = if dir_to_list.is_relative() {
        cwd.join(&dir_to_list)
    } else {
        dir_to_list
    };

    let entries = match std::fs::read_dir(&resolved) {
        Ok(rd) => rd,
        Err(_) => return Vec::new(),
    };

    collect_path_completions(entries, &typed_prefix)
}

fn collect_path_completions(
    entries: impl Iterator<Item = std::io::Result<std::fs::DirEntry>>,
    typed_prefix: &str,
) -> Vec<ArgItem> {
    let mut items: Vec<ArgItem> = Vec::new();
    for entry in entries.take(1000).filter_map(Result::ok) {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        if name_str.starts_with('.') {
            continue;
        }

        // Follow symlinks so symlinked directories get trailing `/`.
        let is_dir = entry.path().is_dir();
        let suffix = if is_dir { "/" } else { "" };

        items.push(ArgItem {
            display: format!("{name_str}{suffix}"),
            match_text: format!("{typed_prefix}{name_str}"),
            insert_text: format!("{typed_prefix}{name_str}{suffix}"),
            description: if is_dir {
                "directory".to_string()
            } else {
                "file".to_string()
            },
        });

    }

    // Sort: directories first, then alphabetical. Truncate after sort.
    items.sort_by(|a, b| {
        let a_dir = a.display.ends_with('/');
        let b_dir = b.display.ends_with('/');
        b_dir.cmp(&a_dir).then_with(|| a.display.cmp(&b.display))
    });
    items.truncate(100);

    items
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp::model_state::ModelState;
    use crate::app::actions::Action;
    use crate::app::bundle::BundleState;
    use crate::settings::PagerLocalSnapshot;

    #[test]
    fn hidden_entries_and_errors_consume_enumeration_budget() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join(".hidden"), b"").unwrap();
        let mut consumed = 0;
        let entries = std::iter::from_fn(|| {
            if consumed == 1001 {
                return None;
            }
            consumed += 1;
            if consumed % 2 == 0 {
                Some(Err(std::io::Error::other("injected directory entry error")))
            } else {
                Some(std::fs::read_dir(directory.path()).unwrap().next().unwrap())
            }
        });
        assert!(collect_path_completions(entries, "prefix/").is_empty());
        assert_eq!(consumed, 1000);
    }

    #[test]
    fn visible_path_completions_preserve_prefix_sorting_and_output_cap() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        std::fs::create_dir(root.join("folder")).unwrap();
        std::fs::write(root.join("z.txt"), b"").unwrap();
        std::fs::write(root.join("a.txt"), b"").unwrap();
        std::fs::write(root.join(".hidden"), b"").unwrap();
        let items = list_path_completions(root, "./");
        assert_eq!(items.iter().map(|item| item.display.as_str()).collect::<Vec<_>>(), ["folder/", "a.txt", "z.txt"]);
        assert_eq!(items[0].insert_text, "./folder/");
        assert_eq!(items[1].insert_text, "./a.txt");
        assert_eq!(items[0].description, "directory");
        for index in 0..120 {
            std::fs::write(root.join(format!("item-{index:03}")), b"").unwrap();
        }
        assert_eq!(list_path_completions(root, "./").len(), 100);
        assert!(list_path_completions(root, "missing/").is_empty());
        assert!(list_path_completions(root, "").is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn symlink_directory_keeps_drill_down_suffix() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::create_dir(directory.path().join("target")).unwrap();
        std::os::unix::fs::symlink(directory.path().join("target"), directory.path().join("link")).unwrap();
        let items = list_path_completions(directory.path(), "./");
        assert!(items.iter().any(|item| item.insert_text == "./link/" && item.description == "directory"));
    }

    static DEFAULT_BUNDLE_STATE: BundleState = BundleState {
        has_cache: false,
        version: String::new(),
        agents: Vec::new(),
        skills: Vec::new(),
    };

    fn make_ctx(models: &ModelState) -> CommandExecCtx<'_> {
        CommandExecCtx {
            models,
            session_id: None,
            bundle_state: &DEFAULT_BUNDLE_STATE,
            screen_mode: crate::app::ScreenMode::Inline,
            pager_state: PagerLocalSnapshot::default(),
        }
    }

    #[test]
    fn no_session_errors() {
        let models = ModelState::default();
        let mut ctx = make_ctx(&models);
        let cmd = ExportCommand;
        match cmd.run(&mut ctx, "") {
            CommandResult::Error(msg) => assert!(msg.contains("No active session")),
            other => panic!("expected Error, got {other:?}"),
        }
    }

    #[test]
    fn dispatches_clipboard_when_no_path() {
        let models = ModelState::default();
        let sid = agent_client_protocol::schema::v1::SessionId::from("test-session".to_string());
        let mut ctx = CommandExecCtx {
            models: &models,
            session_id: Some(&sid),
            bundle_state: &DEFAULT_BUNDLE_STATE,
            screen_mode: crate::app::ScreenMode::Inline,
            pager_state: PagerLocalSnapshot::default(),
        };
        let cmd = ExportCommand;
        match cmd.run(&mut ctx, "   ") {
            CommandResult::Action(Action::ExportConversation { file_path }) => {
                assert!(file_path.is_none());
            }
            other => panic!("expected ExportConversation(None), got {other:?}"),
        }
    }

    #[test]
    fn dispatches_file_path_when_given() {
        let models = ModelState::default();
        let sid = agent_client_protocol::schema::v1::SessionId::from("s2".to_string());
        let mut ctx = CommandExecCtx {
            models: &models,
            session_id: Some(&sid),
            bundle_state: &DEFAULT_BUNDLE_STATE,
            screen_mode: crate::app::ScreenMode::Inline,
            pager_state: PagerLocalSnapshot::default(),
        };
        let cmd = ExportCommand;
        match cmd.run(&mut ctx, "~/exports/my convo with spaces.md") {
            CommandResult::Action(Action::ExportConversation { file_path }) => {
                let p = file_path.expect("some path");
                assert!(p.to_string_lossy().contains("my convo with spaces.md"));
            }
            other => panic!("expected ExportConversation(Some), got {other:?}"),
        }
    }
}
