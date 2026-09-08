//! `/copy` -- copy the last (or Nth) assistant message to the clipboard.
//!
//! Optional file path writes instead of (or when) the clipboard is unreachable:
//! - `/copy` — latest → clipboard (file fallback on failure)
//! - `/copy 2` — 2nd-latest → clipboard
//! - `/copy out.txt` — latest → file
//! - `/copy 2 out.txt` — 2nd-latest → file

use std::path::PathBuf;

use crate::app::actions::Action;
use crate::slash::command::{CommandExecCtx, CommandResult, SlashCommand};

/// Copy an assistant message to the clipboard (or an optional file).
pub struct CopyCommand;

impl SlashCommand for CopyCommand {
    fn name(&self) -> &str {
        "copy"
    }

    fn description(&self) -> &str {
        "Copy last response to clipboard or file (/copy [N] [file])"
    }

    fn session_scoped(&self) -> bool {
        true
    }

    fn usage(&self) -> &str {
        "/copy [N] [file]"
    }

    fn takes_args(&self) -> bool {
        true
    }

    fn arg_placeholder(&self) -> Option<&str> {
        Some("[N] [file]")
    }

    fn run(&self, _ctx: &mut CommandExecCtx, args: &str) -> CommandResult {
        match parse_copy_args(args) {
            Ok((n, file_path)) => {
                CommandResult::Action(Action::CopyAssistantMessage { n, file_path })
            }
            Err(msg) => CommandResult::Error(msg),
        }
    }
}

/// Parse `/copy` args into `(n, optional_file_path)`.
///
/// - empty → `(1, None)`
/// - `2` → `(2, None)`
/// - `out.txt` → `(1, Some(out.txt))`
/// - `2 out.txt` → `(2, Some(out.txt))` (rest of line is the path, spaces ok)
fn parse_copy_args(args: &str) -> Result<(usize, Option<PathBuf>), String> {
    let trimmed = args.trim();
    if trimmed.is_empty() {
        return Ok((1, None));
    }

    let mut parts = trimmed.splitn(2, char::is_whitespace);
    let first = parts.next().unwrap_or("");
    let rest = parts.next().map(str::trim).filter(|s| !s.is_empty());

    let digits = first.strip_prefix('+').or_else(|| first.strip_prefix('-')).unwrap_or(first);
    let is_integer = !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit());
    let usage = "Usage: /copy [N] [file] where N is 1 (latest), 2, 3, ... Use ./123 for a numeric filename.";
    match first.parse::<usize>() {
        Ok(0) => Err(usage.to_string()),
        Ok(n) => Ok((n, rest.map(PathBuf::from))),
        Err(_) if is_integer => Err(usage.to_string()),
        Err(_) => {
            // Non-numeric first token: treat the whole args string as a path.
            Ok((1, Some(PathBuf::from(trimmed))))
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn invalid_numeric_indices_do_not_become_file_paths() {
        let overflow = format!("{}0", usize::MAX);
        for token in ["0".to_string(), "-1".to_string(), "+0".to_string(),
            overflow.clone(), format!("+{overflow}"), format!("-{overflow}")] {
            for input in [token.clone(), format!("{token} output.txt")] {
                assert!(parse_copy_args(&input).is_err(), "{input:?} must not become a file path");
            }
        }
        for path in ["./123", "./-1", "123.txt", "folder/my reply.txt"] {
            assert_eq!(parse_copy_args(path).unwrap(), (1, Some(PathBuf::from(path))));
        }
        assert_eq!(parse_copy_args("+1").unwrap(), (1, None));
        assert_eq!(parse_copy_args("2 out.txt").unwrap(), (2, Some(PathBuf::from("out.txt"))));
    }

    use super::*;
    use crate::acp::model_state::ModelState;
    use crate::app::actions::Action;

    static DEFAULT_BUNDLE_STATE: crate::app::bundle::BundleState =
        crate::app::bundle::BundleState {
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
            pager_state: crate::settings::PagerLocalSnapshot::default(),
        }
    }

    #[test]
    fn no_args_copies_latest() {
        let models = ModelState::default();
        let mut ctx = make_ctx(&models);
        let cmd = CopyCommand;
        match cmd.run(&mut ctx, "") {
            CommandResult::Action(Action::CopyAssistantMessage { n, file_path }) => {
                assert_eq!(n, 1);
                assert!(file_path.is_none());
            }
            other => panic!("expected Action(CopyAssistantMessage), got {other:?}"),
        }
    }

    #[test]
    fn explicit_1_copies_latest() {
        let models = ModelState::default();
        let mut ctx = make_ctx(&models);
        let cmd = CopyCommand;
        match cmd.run(&mut ctx, "1") {
            CommandResult::Action(Action::CopyAssistantMessage { n, file_path }) => {
                assert_eq!(n, 1);
                assert!(file_path.is_none());
            }
            other => panic!("expected Action(CopyAssistantMessage), got {other:?}"),
        }
    }

    #[test]
    fn explicit_3_copies_third() {
        let models = ModelState::default();
        let mut ctx = make_ctx(&models);
        let cmd = CopyCommand;
        match cmd.run(&mut ctx, "3") {
            CommandResult::Action(Action::CopyAssistantMessage { n, file_path }) => {
                assert_eq!(n, 3);
                assert!(file_path.is_none());
            }
            other => panic!("expected Action(CopyAssistantMessage), got {other:?}"),
        }
    }

    #[test]
    fn zero_returns_error() {
        let models = ModelState::default();
        let mut ctx = make_ctx(&models);
        let cmd = CopyCommand;
        assert!(matches!(cmd.run(&mut ctx, "0"), CommandResult::Error(_)));
    }

    #[test]
    fn path_only_writes_latest_to_file() {
        let models = ModelState::default();
        let mut ctx = make_ctx(&models);
        let cmd = CopyCommand;
        match cmd.run(&mut ctx, "out.txt") {
            CommandResult::Action(Action::CopyAssistantMessage { n, file_path }) => {
                assert_eq!(n, 1);
                assert_eq!(file_path.as_deref(), Some(std::path::Path::new("out.txt")));
            }
            other => panic!("expected Action(CopyAssistantMessage), got {other:?}"),
        }
    }

    #[test]
    fn n_and_path_with_spaces() {
        assert_eq!(
            parse_copy_args("2 ~/exports/my note.txt").unwrap(),
            (2, Some(PathBuf::from("~/exports/my note.txt")))
        );
    }

    #[test]
    fn whitespace_only_copies_latest() {
        let models = ModelState::default();
        let mut ctx = make_ctx(&models);
        let cmd = CopyCommand;
        match cmd.run(&mut ctx, "   ") {
            CommandResult::Action(Action::CopyAssistantMessage { n, file_path }) => {
                assert_eq!(n, 1);
                assert!(file_path.is_none());
            }
            other => panic!("expected Action(CopyAssistantMessage), got {other:?}"),
        }
    }
}
