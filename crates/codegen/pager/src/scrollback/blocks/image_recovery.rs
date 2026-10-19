//! Mutable lifecycle row for recoverable image input.

use ratatui::style::Modifier;
use ratatui::text::{Line, Span};

use crate::appearance::AppearanceConfig;
use crate::scrollback::block::BlockContent;
use crate::scrollback::types::{AccentStyle, BlockContext, BlockOutput, DisplayMode};
use crate::theme::Theme;

#[derive(Debug, Clone)]
pub struct ImageRecoveryBlock {
    pub operation_id: String,
    pub prompt_id: Option<String>,
    pub message: String,
    pub running: bool,
    pub failed: bool,
}

impl BlockContent for ImageRecoveryBlock {
    fn output(&self, ctx: &BlockContext) -> BlockOutput {
        let theme = Theme::current();
        let label = if ctx.is_selected {
            theme.primary().add_modifier(Modifier::BOLD)
        } else {
            theme.muted().add_modifier(Modifier::BOLD)
        };
        let message = self.message.replace('\n', " ");
        let (verb, detail) = message.split_once(' ').unwrap_or((&message, ""));
        BlockOutput {
            lines: vec![
                Line::from(vec![
                    Span::styled(verb.to_owned(), label),
                    Span::styled(
                        if detail.is_empty() {
                            String::new()
                        } else {
                            format!(" {detail}")
                        },
                        theme.muted(),
                    ),
                ])
                .into(),
            ],
        }
    }

    fn accent(&self, _ctx: &BlockContext) -> Option<AccentStyle> {
        let theme = Theme::current();
        if self.running {
            Some(AccentStyle::animated(theme.accent_running))
        } else if self.failed {
            Some(AccentStyle::static_color(theme.accent_error))
        } else {
            Some(AccentStyle::static_color(theme.accent_success))
        }
    }

    fn has_vpad_for(&self, _appearance: &AppearanceConfig) -> bool {
        false
    }

    fn is_foldable(&self) -> bool {
        false
    }

    fn default_display_mode(&self) -> DisplayMode {
        DisplayMode::Collapsed
    }

    fn has_bullet(&self, _ctx: &BlockContext) -> bool {
        true
    }

    fn is_groupable(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scrollback::types::{BlockContext, DisplayMode};

    fn context() -> BlockContext {
        BlockContext {
            width: 80,
            mode: DisplayMode::Collapsed,
            is_running: false,
            raw: false,
            max_lines: None,
            appearance: Default::default(),
            is_selected: false,
            cwd: None,
        }
    }

    fn first_line(block: &ImageRecoveryBlock) -> String {
        block.output(&context()).lines[0]
            .content
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }

    #[test]
    fn operation_row_renders_english_result_and_running_accent_settles() {
        let mut block = ImageRecoveryBlock {
            operation_id: "image-op".into(),
            prompt_id: Some("prompt-1".into()),
            message: "Calling the configured vision model…".into(),
            running: true,
            failed: false,
        };
        assert_eq!(first_line(&block), "Calling the configured vision model…");
        assert!(block.accent(&context()).unwrap().animated);

        block.message = "Converted 1 image".into();
        block.running = false;
        assert_eq!(first_line(&block), "Converted 1 image");
        assert!(!block.accent(&context()).unwrap().animated);
    }
}
