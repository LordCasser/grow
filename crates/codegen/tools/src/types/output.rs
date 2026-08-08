use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use strip_ansi_escapes::strip_str;
use tool_types::SubagentCompletedOutput;
/// `(added, removed)` line counts for the `edit.lines` diagnostics counter.
pub fn line_diff(old: &str, new: &str) -> (i64, i64) {
    let mut added = 0i64;
    let mut removed = 0i64;
    for change in similar::TextDiff::from_lines(old, new).iter_all_changes() {
        match change.tag() {
            similar::ChangeTag::Insert => added += 1,
            similar::ChangeTag::Delete => removed += 1,
            similar::ChangeTag::Equal => {}
        }
    }
    (added, removed)
}
/// Wrapper for [`ToolOutput::Text`] so it can round-trip through
/// `#[serde(tag = "type")]` (internally-tagged enums require struct/map
/// payloads, not bare primitives).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextOutput {
    pub text: String,
    /// Background-task id for auto-wake suppression when set (see
    /// [`crate::reminders::task_completion::consumed_completion_ids`]);
    /// omitted from prompts and from JSON when `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub consumed_completion_task_id: Option<String>,
}
impl From<String> for TextOutput {
    fn from(text: String) -> Self {
        Self {
            text,
            consumed_completion_task_id: None,
        }
    }
}
impl From<&str> for TextOutput {
    fn from(text: &str) -> Self {
        Self {
            text: text.to_owned(),
            consumed_completion_task_id: None,
        }
    }
}
/// Wrapper for [`ToolOutput::Dynamic`] so it can round-trip through
/// `#[serde(tag = "type")]` (internally-tagged enums require struct/map
/// payloads, not bare primitives).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DynamicOutput {
    pub value: serde_json::Value,
}
impl From<serde_json::Value> for DynamicOutput {
    fn from(value: serde_json::Value) -> Self {
        Self { value }
    }
}
use crate::implementations::grow_build::todo::{TodoItem, TodoState};
use crate::implementations::skills::skill::SkillOutput;
use crate::util::truncate::{DEFAULT_SOFT_WRAP_WIDTH, soft_wrap_lines};
/// Result of running a tool through the ToolRunner pipeline.
///
/// This is the **single return type** from `ToolRunner::run()`. It carries:
/// 1. Clean `output` — never mutated by layers; for JSON serialization, protocol translation.
/// 2. `prompt_text` — rendered with system reminders appended; for model prompt.
#[derive(Debug, Serialize, Deserialize)]
pub struct ToolRunResult {
    /// Clean tool output — never mutated by layers.
    /// Consumers use this for: JSON serialization, protocol translation, hunk tracking.
    pub output: ToolOutput,
    /// Prompt-ready text — layers can append system reminders, etc.
    /// Consumers use this for: model prompt (ConversationItem::tool_result).
    pub prompt_text: String,
    /// When a meta-tool dispatches to a different underlying tool (for example
    /// `use_tool` → `linear__save_issue`), this carries the effective tool name.
    /// `None` means the requested tool and executed tool are the same.
    pub effective_tool_name: Option<String>,
}
impl ToolRunResult {
    /// Like [`TypedToolOutput::from_value`], but reattaches `chat_completion_output` from `output`.
    pub fn into_typed_tool_output(
        self,
        tool_id: tool_protocol::ToolId,
    ) -> tool_runtime::TypedToolOutput {
        typed_tool_output_preserving_cco(tool_id, &self, &self.output)
    }
}
/// Like [`TypedToolOutput::from_value`], but reattaches `chat_completion_output` from `source`.
pub(crate) fn typed_tool_output_preserving_cco(
    tool_id: tool_protocol::ToolId,
    payload: &impl Serialize,
    source: &impl tool_runtime::ToolOutput,
) -> tool_runtime::TypedToolOutput {
    let cco = source.chat_completion_output();
    let value = serde_json::to_value(payload).unwrap_or(serde_json::Value::Null);
    tool_runtime::TypedToolOutput::from_value(tool_id, value).with_chat_completion_output(cco)
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ListDirContent {
    /// Formatted directory listing string
    pub content: String,
    /// Root directory path (absolute) for this listing
    pub absolute_root_path: PathBuf,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub enum ListDirOutput {
    Content(ListDirContent),
    /// Target path does not exist
    NotFound(String),
    /// Target path exists but is a file, not a directory
    IsAFile(String),
    /// Target path exists but is not a directory
    NotADirectory(String),
    /// Permission denied accessing the directory
    PermissionDenied(String),
    /// Generic / unclassified error
    Error(String),
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct GrepLineMatch {
    pub line_number: usize,
    pub content: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct GrepFileMatch {
    pub path: String,
    pub matches: Vec<GrepLineMatch>,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct GrepSearchOutput {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub exit_code: i32,
    pub match_count: usize,
    #[serde(default)]
    pub file_matches: Vec<GrepFileMatch>,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct FileContent {
    /// content here is the model friendly output which will always be present since even
    /// on failures we want to present the model with some information
    pub content: String,
    /// Concise version of content (arrow separator, no padding) for models
    /// that use the concise output format
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_concise: Option<String>,
    pub absolute_path: PathBuf,
    pub offset: Option<usize>,
    /// The line limit used for this read. `None` means no limit was applied.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
    /// Contains the raw output from the tool invocation without any formatting
    pub raw_output: String,
    /// Total number of lines in the file. Used by system reminders to detect
    /// offset-past-end vs genuinely-empty files.
    #[serde(default)]
    pub total_lines: usize,
    /// Base64 images captured before per-line truncation. The session
    /// layer turns these into multimodal `ContentPart::Image` follow-ups
    /// (same pipeline as MCP image extraction); pre-truncation capture
    /// prevents `truncate_line` from cutting a long single-line URI
    /// mid-payload. Hidden from the model's JSON schema.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[schemars(skip)]
    pub extracted_images: Vec<crate::util::base64_images::ExtractedImage>,
}
/// Image content returned when reading an image file.
///
/// This is a local type so it can derive `schemars::JsonSchema` v0.8,
/// which the `Tool` trait requires for its `Output` associated type.
/// Conversion to the protocol-level image type happens at the
/// protocol boundary in `shell`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ImageContent {
    /// Base64-encoded image data
    pub data: String,
    /// MIME type of the image (e.g., "image/png", "image/jpeg")
    pub mime_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub annotations: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<serde_json::Value>,
}
/// A single rendered PDF page.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PdfPageImage {
    /// Base64-encoded JPEG data
    pub data: String,
    /// MIME type (always "image/jpeg")
    pub mime_type: String,
    /// 1-based page number
    pub page_number: usize,
}
/// Multiple rendered PDF page images.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PdfPageImages {
    /// Rendered page images, one per requested page
    pub pages: Vec<PdfPageImage>,
    /// Total pages in the PDF document
    pub total_pages: usize,
    /// File size in bytes
    pub file_size: usize,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub enum ReadFileOutput {
    FileContent(FileContent),
    /// Target file does not exist
    FileNotFound(String),
    /// Target path is a directory, not a file
    IsADirectory(String),
    /// Permission denied reading the file
    PermissionDenied(String),
    /// File content exceeds maximum token limit
    FileTooLarge(String),
    /// Generic / unclassified read error
    FileReadError(String),
    ImageContent(ImageContent),
    ImageSizeError(String),
    PdfPageImages(PdfPageImages),
}
/// Represents successful edits applied by SearchReplace
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SearchReplaceEditsApplied {
    pub old_string: String,
    pub new_string: String,
    pub tool_output_for_prompt: String,
    /// Concise version of tool_output_for_prompt (shorter, no snippet) for
    /// models that use the concise output format
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_output_for_prompt_concise: Option<String>,
    pub absolute_path: PathBuf,
    pub edits: SearchReplaceEditContextInformation,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub patch: Option<String>,
    /// `true` when the match used Unicode confusable normalization
    /// (exact byte match failed, but normalized match succeeded).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub unicode_normalized: bool,
}
/// Contains the edit details present as a struct
#[derive(Debug, Default, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SearchReplaceEditContextInformation {
    pub details: Vec<SearchReplaceEditDetail>,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SearchReplaceEditDetail {
    /// The exact old string that was matched in the file
    pub old_string: String,
    /// 1-based line number where the match begins in the original file
    pub old_line: usize,
    /// The replacement string that was written
    pub new_string: String,
    /// 1-based line number where the replacement begins in the updated file
    pub new_line: usize,
    /// The context before the match
    pub context_before: String,
    /// The context after the match
    pub context_after: String,
    /// Leading text on the first line before the matched `old_string` begins.
    ///
    /// When the match starts mid-line (e.g., after indentation), this captures
    /// the prefix so the diff renderer can display proper alignment. Empty when
    /// the match starts at the beginning of a line or when unknown.
    #[serde(default)]
    pub line_prefix: String,
}
/// Payload for `SearchReplaceOutput::NoMatchesFound`.
///
/// Separate struct so consumers (reminders, outcome trackers) can extract
/// the file path without needing to know the call-site context.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct NoMatchesFoundError {
    /// Human-readable error message shown to the model.
    pub message: String,
    /// Canonical absolute path of the file that was searched.
    pub file_path: std::path::PathBuf,
    /// Full file text from the same read the edit used when reporting no match.
    ///
    /// In-process only: never serialized on the wire (avoids leaking fresher or
    /// broader file content than the read/edit path already loaded). Used for
    /// `StrReplace` fuzzy hints without a second `read_file`.
    #[serde(default, skip_serializing)]
    #[schemars(skip)]
    pub file_snapshot_at_edit: Option<String>,
}
/// Output type for the SearchReplace tool
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub enum SearchReplaceOutput {
    FileAlreadyExists(String),
    EditsApplied(SearchReplaceEditsApplied),
    MultipleMatchesFound(String),
    /// The `old_string` was not found in the file.
    /// Carries the canonical absolute path so reminders and outcome trackers
    /// can do per-file accounting without needing extra context.
    NoMatchesFound(NoMatchesFoundError),
    InvalidInput(String),
    /// Target file does not exist
    FileNotFound(String),
    /// A path component exceeds the OS filename length limit (ENAMETOOLONG)
    FilenameTooLong(String),
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct BashOutput {
    pub output: Vec<u8>,
    /// ANSI-stripped and soft-wrapped output for model prompt.
    /// Pre-baked at construction time so `to_prompt_format` is a simple read.
    #[serde(default)]
    pub output_for_prompt: String,
    pub exit_code: i32,
    pub command: String,
    pub truncated: bool,
    pub signal: Option<String>,
    pub timed_out: bool,
    /// describes the intent of this bash command
    pub description: Option<String>,
    /// the current working directory after the command completes
    pub current_dir: String,
    /// Path to the output file where full output is stored.
    /// Use read_file tool to retrieve full output when truncated.
    pub output_file: String,
    /// Total bytes of output (before truncation).
    pub total_bytes: usize,
    /// Incremental output delta (new bytes since last notification).
    /// When present, consumers should append to their accumulated buffer
    /// instead of replacing with `output`. When `Some(vec![])`, consumers
    /// should clear their accumulated buffer (reset signal).
    /// When `None`, the consumer should use `output` as the full buffer.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub output_delta: Option<Vec<u8>>,
    /// Set by the grow_build `run_terminal_cmd` implementation when the
    /// command was detected as a bare `echo "<msg>"` (or close variant:
    /// echo -n, echo -e, simple printf for literal output, etc.).
    ///
    /// Used for:
    /// - Diagnostic / statistics on this pattern for the grow_build backend.
    /// - Potential doom-loop / stagnation signals (repeated trivial echoes
    ///   are a common "no progress" signal).
    /// - Model hints (see BareEchoHintState in the bash tool).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub was_bare_echo: bool,
}
impl BashOutput {
    /// Compute `output_for_prompt` from raw output string.
    /// Strips ANSI escapes and soft-wraps long lines.
    pub fn make_output_for_prompt(raw: &str) -> String {
        let stripped = strip_str(raw);
        soft_wrap_lines(&stripped, DEFAULT_SOFT_WRAP_WIDTH)
    }
}
/// Output when a background task is started (matches the vendor-compat XML format)
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct BackgroundTaskStarted {
    /// Unique task ID (UUID) for querying later
    pub task_id: String,
    /// Type of background task (e.g., "bash")
    pub task_type: String,
    /// Path to the output file on disk
    pub output_file: String,
    /// Current status (always "running" when returned)
    pub status: String,
    /// The command that was started
    pub command: String,
    /// Human-readable summary
    pub summary: String,
    /// Pre-resolved hint text telling the model how to retrieve output.
    /// Built by the tool's run() using resolved tool/param names.
    #[serde(default)]
    pub retrieval_hint: String,
    /// Optional pre-formatted prompt body. When set, `to_prompt_format`
    /// uses this string verbatim instead of the default
    /// `<task-id>...</task-id>` XML envelope. Used by namespace-specific
    /// adapters that need to emit a different model-visible shape
    /// without disturbing the structured fields above (which other
    /// consumers still parse).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pre_formatted: Option<String>,
    /// PID of the spawned shell process, when available. Surfaced by
    /// adapters in their background-start template; left as `None` when the
    /// underlying backend cannot report a PID (e.g. ACP/remote terminals).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
}
#[derive(Debug, Clone)]
pub struct WebFetchSourceArtifact {
    /// Session artifact containing the complete converted response.
    pub path: PathBuf,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct WebFetchOutputLocation {
    /// Absolute path to the complete rendered output.
    pub file_path: String,
    /// Exact file size in bytes.
    pub size_bytes: usize,
    /// Number of lines in the file.
    pub line_count: usize,
}
/// Successful web fetch result with page content.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct WebFetchContent {
    /// The final URL (may differ from input after redirects).
    pub url: String,
    /// Page content converted to markdown (or raw text for non-HTML).
    pub content: String,
    /// Content type: "markdown" for converted HTML, or the original MIME type.
    pub content_type: String,
    /// HTTP status code.
    pub status_code: u16,
    /// Size of the content in bytes (before truncation).
    pub bytes: usize,
    /// Internal path to the complete converted body when Grow persisted overflow.
    #[serde(skip)]
    #[schemars(skip)]
    pub source_artifact: Option<WebFetchSourceArtifact>,
    /// Structurally selected inline fallback when the complete body is unavailable.
    #[serde(skip)]
    #[schemars(skip)]
    pub inline_fallback: Option<String>,
    /// Vendor-compat file location for large rendered output.
    #[serde(
        rename = "outputLocation",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub output_location: Option<WebFetchOutputLocation>,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub enum WebFetchOutput {
    /// Successful fetch with content.
    Content(WebFetchContent),
    /// Domain is not in the allowed domains list.
    DomainNotAllowed(String),
    /// Server redirected to a different host.
    CrossHostRedirect {
        original_host: String,
        redirect_url: String,
    },
    /// Pre-formatted error message (returned without the `Tool \`X\` failed:`
    /// wrapper that `ToolError` propagation would add).
    Error {
        url: Option<String>,
        message: String,
    },
}
impl WebFetchOutput {
    pub fn to_prompt_format(&self) -> String {
        match self {
            Self::Content(c) => c.content.clone(),
            Self::DomainNotAllowed(domain) => {
                format!(
                    "Error: domain {} is not in the allowed domains list",
                    domain
                )
            }
            Self::CrossHostRedirect {
                original_host,
                redirect_url,
            } => {
                format!(
                    "Error: cross-host redirect from {} to {}. Make a new web_fetch call with the redirect URL if needed.",
                    original_host, redirect_url
                )
            }
            Self::Error {
                url: Some(url),
                message,
            } => {
                format!("Error fetching URL {url}: {message}")
            }
            Self::Error { url: None, message } => format!("Error: {message}"),
        }
    }
}
use tool_types::KillTaskOutput;
use tool_types::TaskOutputOutput;
/// Output schema for the bash tool.
///
/// The bash tool can either complete synchronously (`Bash`) or be started
/// in the background (`BackgroundTaskStarted`). This enum exists to
/// provide a precise JSON Schema via the `Tool::Output` associated type.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type")]
pub enum BashToolOutput {
    Bash(BashOutput),
    BackgroundTaskStarted(BackgroundTaskStarted),
}
impl tool_runtime::ToolOutput for BashToolOutput {
    fn chat_completion_output(&self) -> Option<tool_runtime::ToolChatCompletionResponse> {
        match self {
            Self::Bash(bash) => tool_runtime::ToolOutput::chat_completion_output(bash),
            Self::BackgroundTaskStarted(_) => None,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchToolOutput {
    pub result_count: usize,
    pub content: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, derive_more::From)]
#[serde(tag = "type")]
pub enum ToolOutput {
    Bash(BashOutput),
    BackgroundTaskStarted(BackgroundTaskStarted),
    GrepSearch(GrepSearchOutput),
    ReadFile(ReadFileOutput),
    ListDir(ListDirOutput),
    SearchReplace(SearchReplaceOutput),
    Todo(TodoWriteOutput),
    WebFetch(WebFetchOutput),
    MCP(MCPOutput),
    TaskOutput(TaskOutputOutput),
    KillTask(KillTaskOutput),
    Skill(SkillOutput),
    SearchTool(SearchToolOutput),
    SubagentCompleted(SubagentCompletedOutput),
    PlanControl(PlanControlOutput),
    AskUserQuestion(AskUserQuestionOutput),
    Monitor(crate::implementations::grow_build::monitor::types::MonitorOutput),
    SchedulerCreate(crate::implementations::grow_build::scheduler::create::SchedulerCreateOutput),
    SchedulerDelete(crate::implementations::grow_build::scheduler::delete::SchedulerDeleteOutput),
    SchedulerList(crate::implementations::grow_build::scheduler::list::SchedulerListOutput),
    UpdateGoal(crate::implementations::grow_build::update_goal::UpdateGoalOutput),
    GetGoal(crate::implementations::grow_build::update_goal::GoalView),
    Workflow(crate::implementations::grow_build::workflow::WorkflowToolOutput),
    /// Dynamic output for runtime-registered tools (MCP, test tools, etc.)
    Dynamic(DynamicOutput),
    /// Generic text output for tools that produce simple formatted text
    /// (e.g., memory_search, memory_get). The string is the pre-formatted
    /// prompt text — no additional rendering is needed.
    Text(TextOutput),
}
impl ToolOutput {
    /// Whether this output is a logical tool failure, for `tool.execution`'s
    /// `success`/`outcome`. Conservative: only known error variants count, so we
    /// never report a *false failure*.
    pub fn is_error(&self) -> bool {
        match self {
            ToolOutput::MCP(m) => m.is_error,
            ToolOutput::Bash(b) => b.exit_code != 0,
            ToolOutput::SearchReplace(SearchReplaceOutput::EditsApplied(_)) => false,
            ToolOutput::SearchReplace(_) => true,
            ToolOutput::ListDir(ListDirOutput::Content(_)) => false,
            ToolOutput::ListDir(_) => true,
            ToolOutput::ReadFile(
                ReadFileOutput::FileContent(_)
                | ReadFileOutput::ImageContent(_)
                | ReadFileOutput::PdfPageImages(_),
            ) => false,
            ToolOutput::ReadFile(_) => true,
            ToolOutput::TaskOutput(TaskOutputOutput::TaskNotFound(_)) => true,
            ToolOutput::KillTask(KillTaskOutput::TaskNotFound(_)) => true,
            ToolOutput::Skill(s) => !s.success,
            ToolOutput::WebFetch(WebFetchOutput::Content(_)) => false,
            ToolOutput::WebFetch(_) => true,
            ToolOutput::Todo(
                TodoWriteOutput::DuplicateId(_) | TodoWriteOutput::InvalidArgument(_),
            ) => true,
            ToolOutput::GrepSearch(g) => g.exit_code > 1,
            _ => false,
        }
    }
    /// Render tool output for inclusion in the model prompt with specified format.
    pub fn to_prompt_format(&self) -> String {
        match self {
            ToolOutput::ReadFile(read_file_output) => match read_file_output {
                ReadFileOutput::FileContent(file_content) if file_content.content.is_empty() => {
                    if file_content.total_lines == 0 {
                        "File is empty.".to_string()
                    } else if file_content
                        .offset
                        .is_some_and(|offset| offset > file_content.total_lines)
                    {
                        format!(
                            "(no lines returned: the requested window is past the end of the \
                             file; the file has {} lines)",
                            file_content.total_lines
                        )
                    } else {
                        "(no lines returned)".to_string()
                    }
                }
                ReadFileOutput::FileContent(file_content) => file_content.content.clone(),
                ReadFileOutput::ImageContent(image_content) => {
                    format!(
                        "[Image content of type: {} is included inline in this tool result]",
                        image_content.mime_type,
                    )
                }
                ReadFileOutput::FileNotFound(error_msg)
                | ReadFileOutput::IsADirectory(error_msg)
                | ReadFileOutput::PermissionDenied(error_msg)
                | ReadFileOutput::FileTooLarge(error_msg)
                | ReadFileOutput::FileReadError(error_msg)
                | ReadFileOutput::ImageSizeError(error_msg) => error_msg.to_owned(),
                ReadFileOutput::PdfPageImages(pdf) => {
                    let page_list: Vec<String> = pdf
                        .pages
                        .iter()
                        .map(|p| p.page_number.to_string())
                        .collect();
                    format!(
                        "[Read PDF: {} pages rendered (pages {}). Total document: {} pages, {:.1} KB]",
                        pdf.pages.len(),
                        page_list.join(", "),
                        pdf.total_pages,
                        pdf.file_size as f64 / 1024.0,
                    )
                }
            },
            ToolOutput::ListDir(list_dir_output) => match list_dir_output {
                ListDirOutput::Content(content) => content.content.clone(),
                ListDirOutput::NotFound(error_msg)
                | ListDirOutput::IsAFile(error_msg)
                | ListDirOutput::NotADirectory(error_msg)
                | ListDirOutput::PermissionDenied(error_msg)
                | ListDirOutput::Error(error_msg) => error_msg.to_owned(),
            },
            ToolOutput::SearchReplace(search_replace_output) => match search_replace_output {
                SearchReplaceOutput::EditsApplied(edits_applied) => {
                    edits_applied.tool_output_for_prompt.to_owned()
                }
                SearchReplaceOutput::NoMatchesFound(e) => e.message.clone(),
                SearchReplaceOutput::FileAlreadyExists(error_string)
                | SearchReplaceOutput::MultipleMatchesFound(error_string)
                | SearchReplaceOutput::InvalidInput(error_string)
                | SearchReplaceOutput::FileNotFound(error_string)
                | SearchReplaceOutput::FilenameTooLong(error_string) => error_string.to_owned(),
            },
            ToolOutput::Bash(bash_output) => bash_output.output_for_prompt.clone(),
            ToolOutput::GrepSearch(grep_search_output) => {
                String::from_utf8_lossy(&grep_search_output.stdout).into_owned()
            }
            ToolOutput::Todo(todo_output) => match todo_output {
                TodoWriteOutput::TodosUpdated(success) => success.summary_for_prompt.to_owned(),
                TodoWriteOutput::DuplicateId(msg) => msg.to_owned(),
                TodoWriteOutput::InvalidArgument(msg) => msg.to_owned(),
            },
            ToolOutput::WebFetch(o) => o.to_prompt_format(),
            ToolOutput::MCP(mcp_output) => match &mcp_output.output {
                MCPOutputDetails::Error(error) => {
                    format!("Failed to call {}: {}", &mcp_output.tool_name, error)
                }
                MCPOutputDetails::OkayOutput(output) => output.to_owned(),
            },
            ToolOutput::BackgroundTaskStarted(bg) => {
                if let Some(body) = bg.pre_formatted.as_deref() {
                    body.to_string()
                } else {
                    format!(
                        "<task-id>{}</task-id>\n\
                         <task-type>{}</task-type>\n\
                         <output-file>{}</output-file>\n\
                         <status>{}</status>\n\
                         <summary>{}</summary>\n\
                         {}",
                        bg.task_id,
                        bg.task_type,
                        bg.output_file,
                        bg.status,
                        bg.summary,
                        bg.retrieval_hint
                    )
                }
            }
            ToolOutput::TaskOutput(task_output) => match task_output {
                TaskOutputOutput::Result(r) => {
                    let mut lines = vec![
                        format!("=== Task {} ===", r.task_id),
                        format!("Command: {}", r.command),
                        format!("Status: {}", r.status),
                        format!("Duration: {:.2}s", r.duration_secs),
                    ];
                    if let Some(code) = r.exit_code {
                        lines.push(format!("Exit Code: {}", code));
                    }
                    if !r.output_file.is_empty() {
                        lines.push(format!("Output File: {}", r.output_file));
                    }
                    lines.push(String::new());
                    lines.push("=== Output ===".to_string());
                    if r.output.is_empty() {
                        if r.status == "running" {
                            lines.push("(no output yet)".to_string());
                        } else {
                            lines.push("(no output)".to_string());
                        }
                    } else {
                        lines.push(r.output.clone());
                    }
                    if r.truncated {
                        lines.push(r.truncation_hint.clone());
                    }
                    lines.join("\n")
                }
                TaskOutputOutput::TaskNotFound(msg) => msg.to_owned(),
                TaskOutputOutput::MultiResult(mr) => {
                    let mut lines = vec![format!("=== Multi-wait ({}) ===", mr.mode)];
                    for r in &mr.results {
                        lines.push(format!(
                            "--- Task {} [{}] ---\nCommand: {}\nDuration: {:.2}s",
                            r.task_id, r.status, r.command, r.duration_secs,
                        ));
                        if let Some(code) = r.exit_code {
                            lines.push(format!("Exit Code: {code}"));
                        }
                        if !r.output.is_empty() {
                            lines.push(r.output.clone());
                        }
                    }
                    lines.push(format!("\n{}", mr.summary));
                    lines.join("\n")
                }
            },
            ToolOutput::KillTask(kill_output) => match kill_output {
                KillTaskOutput::Result(r) => format!("{}: {}", r.outcome, r.message),
                KillTaskOutput::TaskNotFound(msg) => msg.to_owned(),
            },
            ToolOutput::Skill(skill_output) => skill_output
                .skill_message
                .clone()
                .unwrap_or_else(|| skill_output.tool_result.clone()),
            ToolOutput::SearchTool(out) => out.content.clone(),
            ToolOutput::SubagentCompleted(sub) => {
                let mut text = sub.output.clone();
                if let Some(ref wt) = sub.worktree_path {
                    text.push_str(&format!("\n\n<worktree_path>{wt}</worktree_path>"));
                }
                text.push_str("\n\n");
                text.push_str(&sub.resume_footer());
                text
            }
            ToolOutput::PlanControl(output) => output.message.clone(),
            ToolOutput::AskUserQuestion(
                AskUserQuestionOutput::QuestionsSent { message, .. }
                | AskUserQuestionOutput::UserAnswered { message },
            ) => message.clone(),
            ToolOutput::Monitor(o) => {
                if o.persistent {
                    format!(
                        "Monitor started (task {}, persistent -- runs until kill_task or session end).\n\
                         You will be notified on each event. Keep working -- do not poll or sleep.\n\
                         Events may arrive while you are waiting for the user -- an event is not their reply.",
                        o.task_id
                    )
                } else {
                    format!(
                        "Monitor started (task {}, timeout {}ms).\n\
                         You will be notified on each event. Keep working -- do not poll or sleep.\n\
                         Events may arrive while you are waiting for the user -- an event is not their reply.",
                        o.task_id, o.timeout_ms
                    )
                }
            }
            ToolOutput::SchedulerCreate(o) => {
                let verb = if o.updated { "updated" } else { "created" };
                format!(
                    "Scheduled task {} (ID: {}, {}).",
                    verb, o.id, o.human_schedule
                )
            }
            ToolOutput::SchedulerDelete(o) => o.message.clone(),
            ToolOutput::SchedulerList(o) => {
                if o.tasks.is_empty() {
                    "No scheduled tasks.".into()
                } else {
                    serde_json::to_string_pretty(&o.tasks).unwrap_or_default()
                }
            }
            ToolOutput::UpdateGoal(o) => o.summary.clone(),
            ToolOutput::GetGoal(o) => serde_json::to_string_pretty(o).unwrap_or_default(),
            ToolOutput::Workflow(o) => o.message.clone(),
            ToolOutput::Dynamic(v) => serde_json::to_string_pretty(&v.value).unwrap_or_default(),
            ToolOutput::Text(text) => text.text.clone(),
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct TodoWriteSuccess {
    pub summary_for_prompt: String,
    pub todos: Vec<TodoItem>,
    /// Full state snapshot for consumer persistence/restoration.
    #[schemars(skip)]
    pub state: TodoState,
}
/// Output from the TodoWrite tool.
///
/// Follows the error-as-output-variant pattern (like `ReadFileOutput`,
/// `SearchReplaceOutput`) so consumers (Python side, ACP layer) can
/// distinguish tool-logic errors from infrastructure errors.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub enum TodoWriteOutput {
    /// Successfully updated todo state.
    TodosUpdated(TodoWriteSuccess),
    /// Duplicate todo ID found in the input.
    DuplicateId(String),
    /// Argument validation failed (model-facing message is returned verbatim).
    /// Used so missing-field errors surface as the terse `Invalid argument: …`
    /// line, instead of the framework's wrapper around a `ToolError`.
    InvalidArgument(String),
}
/// Output from the `AskUserQuestion` tool.
///
/// This is a thin signal — the tool sends the questions to the client via
/// a notification and returns a confirmation. The actual answers come back
/// from the client as the tool result (handled by the orchestration layer).
///
/// Because the answers are provided by the client asynchronously (the user
/// interacts with a UI), the tool output here just confirms the questions
/// were dispatched. The orchestration layer is responsible for blocking
/// until the user responds and injecting the answers into the conversation.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub enum AskUserQuestionOutput {
    /// Questions were successfully dispatched to the client for user input.
    /// Used during migration fallback when `UserQuestionSender` is not yet
    /// injected by the shell.
    QuestionsSent {
        /// Confirmation message for the model.
        message: String,
        /// Number of questions sent.
        question_count: usize,
    },
    /// The user has responded (or cancelled). The `message` is the
    /// fully-formatted tool result string produced by the format module.
    ///
    /// All four user paths (accepted, chat about this, skip interview,
    /// cancel) return this variant with `ToolCall` status `Completed`.
    UserAnswered {
        /// Pre-formatted tool result string for the model.
        message: String,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PlanControlOutput {
    pub message: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MCPOutputDetails {
    OkayOutput(String),
    Error(String),
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MCPOutput {
    tool_name: String,
    server_name: String,
    output: MCPOutputDetails,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub reconnect_attempted: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub auth_retry_attempted: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub is_timeout: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub is_error: bool,
}
impl MCPOutput {
    pub fn okay_output(tool_name: String, server_name: String, output: String) -> Self {
        Self {
            tool_name,
            server_name,
            output: MCPOutputDetails::OkayOutput(output),
            reconnect_attempted: false,
            auth_retry_attempted: false,
            is_timeout: false,
            is_error: false,
        }
    }
    pub fn errored(tool_name: String, server_name: String, error: String) -> Self {
        Self {
            tool_name,
            server_name,
            output: MCPOutputDetails::Error(error),
            reconnect_attempted: false,
            auth_retry_attempted: false,
            is_timeout: false,
            is_error: true,
        }
    }
    pub fn output(&self) -> &MCPOutputDetails {
        &self.output
    }
    pub fn output_mut(&mut self) -> &mut MCPOutputDetails {
        &mut self.output
    }
}
impl tool_runtime::ToolOutput for ToolOutput {
    fn chat_completion_output(&self) -> Option<tool_runtime::ToolChatCompletionResponse> {
        match self {
            Self::Bash(bash) => tool_runtime::ToolOutput::chat_completion_output(bash),
            _ => None,
        }
    }
}
impl tool_runtime::ToolOutput for BashOutput {
    fn chat_completion_output(&self) -> Option<tool_runtime::ToolChatCompletionResponse> {
        let mut stdout = String::from_utf8_lossy(&self.output).into_owned();
        let mut extra = serde_json::Map::new();
        if self.truncated {
            let shown = crate::util::truncate::format_bytes(self.output.len());
            let total = crate::util::truncate::format_bytes(self.total_bytes);
            stdout.push_str(&format!(
                "\n[truncated: showing first/last {shown} of {total} - full output at: {}]",
                self.output_file
            ));
            extra.insert("truncated".into(), serde_json::Value::Bool(true));
            extra.insert(
                "total_bytes".into(),
                serde_json::Value::from(self.total_bytes as u64),
            );
            if !self.output_file.is_empty() {
                extra.insert(
                    "output_file".into(),
                    serde_json::Value::String(self.output_file.clone()),
                );
            }
        }
        Some(tool_runtime::ToolChatCompletionResponse {
            result: Some(tool_runtime::ToolChatCompletion {
                sender: "assistant".into(),
                message_tag: Some("raw_function_result".into()),
                code_execution_result: Some(tool_runtime::ToolCodeExecutionResult {
                    stdout,
                    stderr: String::new(),
                    exit_code: self.exit_code,
                    command_timed_out: self.timed_out,
                }),
                extra,
                ..Default::default()
            }),
            ..Default::default()
        })
    }
}
impl tool_runtime::ToolOutput for GrepSearchOutput {}
impl tool_runtime::ToolOutput for ReadFileOutput {}
impl tool_runtime::ToolOutput for ListDirOutput {}
impl tool_runtime::ToolOutput for SearchReplaceOutput {}
impl tool_runtime::ToolOutput for TodoWriteOutput {}
impl tool_runtime::ToolOutput for WebFetchOutput {}
impl tool_runtime::ToolOutput for SkillOutput {}
impl tool_runtime::ToolOutput for SearchToolOutput {}
impl tool_runtime::ToolOutput for PlanControlOutput {}
impl tool_runtime::ToolOutput for AskUserQuestionOutput {}
impl tool_runtime::ToolOutput for MCPOutput {}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::implementations::grow_build::todo::{TodoPriority, TodoStatus};
    use serde_json::json;
    use tool_types::KillTaskResult;
    use tool_types::TaskOutputResult;
    /// Serialize a ToolOutput to JSON value
    fn to_json(output: ToolOutput) -> serde_json::Value {
        serde_json::to_value(&output).unwrap()
    }
    fn empty_file_content(offset: Option<usize>, total_lines: usize) -> FileContent {
        FileContent {
            content: String::new(),
            content_concise: None,
            absolute_path: PathBuf::from("/tmp/f.txt"),
            offset,
            limit: None,
            raw_output: String::new(),
            total_lines,
            extracted_images: vec![],
        }
    }
    /// An empty file must render an explicit notice, not a blank result.
    #[test]
    fn read_empty_file_prompt_says_file_is_empty() {
        let output = ToolOutput::ReadFile(ReadFileOutput::FileContent(empty_file_content(None, 0)));
        assert_eq!(output.to_prompt_format(), "File is empty.");
    }
    /// An offset beyond the last line must say past-EOF and report the real
    /// line count.
    #[test]
    fn read_past_eof_prompt_reports_line_count() {
        let output = ToolOutput::ReadFile(ReadFileOutput::FileContent(empty_file_content(
            Some(101),
            100,
        )));
        let prompt = output.to_prompt_format();
        assert!(
            prompt.contains("past the end of the file"),
            "expected past-EOF notice, got: {prompt}"
        );
        assert!(
            prompt.contains("100 lines"),
            "expected real line count, got: {prompt}"
        );
    }
    /// An empty window with an in-range offset (e.g. `limit: 0`) must render
    /// the generic notice, not a bogus past-EOF claim.
    #[test]
    fn read_empty_window_in_range_offset_is_not_past_eof() {
        let output = ToolOutput::ReadFile(ReadFileOutput::FileContent(empty_file_content(
            Some(5),
            100,
        )));
        let prompt = output.to_prompt_format();
        assert_eq!(prompt, "(no lines returned)");
        assert!(
            !prompt.contains("past the end of the file"),
            "in-range empty window must not claim past-EOF: {prompt}"
        );
    }
    /// Non-empty content renders unchanged.
    #[test]
    fn read_non_empty_content_renders_verbatim() {
        let mut fc = empty_file_content(None, 3);
        fc.content = "1→a\nb\nc".to_string();
        let output = ToolOutput::ReadFile(ReadFileOutput::FileContent(fc));
        assert_eq!(output.to_prompt_format(), "1→a\nb\nc");
    }
    #[test]
    fn text_output_to_prompt_format_omits_consumed_completion_task_id() {
        let output = ToolOutput::Text(TextOutput {
            text: "Task completed in 100ms with exit code: 0.".into(),
            consumed_completion_task_id: Some("bg-uuid-42".into()),
        });
        assert_eq!(
            output.to_prompt_format(),
            "Task completed in 100ms with exit code: 0."
        );
    }
    #[test]
    fn text_output_serde_omits_consumed_id_when_none() {
        let text = TextOutput {
            text: "hello".into(),
            consumed_completion_task_id: None,
        };
        let json = serde_json::to_value(&text).unwrap();
        let obj = json.as_object().expect("object");
        assert_eq!(obj.len(), 1);
        assert!(!obj.contains_key("consumed_completion_task_id"));
        let round_trip: TextOutput = serde_json::from_value(json).unwrap();
        assert_eq!(round_trip.text, "hello");
        assert!(round_trip.consumed_completion_task_id.is_none());
    }
    #[test]
    fn text_output_serde_includes_consumed_id_when_some() {
        let text = TextOutput {
            text: "Task completed".into(),
            consumed_completion_task_id: Some("task-abc".into()),
        };
        let json = serde_json::to_value(&text).unwrap();
        assert_eq!(json["consumed_completion_task_id"], "task-abc");
        let round_trip: TextOutput = serde_json::from_value(json).unwrap();
        assert_eq!(
            round_trip.consumed_completion_task_id.as_deref(),
            Some("task-abc")
        );
    }
    #[test]
    fn read_file_not_found_json() {
        let json =
            to_json(ReadFileOutput::FileNotFound("Error: /tmp/x does not exist.".into()).into());
        assert_eq!(
            json,
            json!({"type": "ReadFile", "FileNotFound": "Error: /tmp/x does not exist."})
        );
    }
    #[test]
    fn read_file_is_a_directory_json() {
        let json =
            to_json(ReadFileOutput::IsADirectory("Error: /tmp is a directory.".into()).into());
        assert_eq!(
            json,
            json!({"type": "ReadFile", "IsADirectory": "Error: /tmp is a directory."})
        );
    }
    #[test]
    fn read_file_permission_denied_json() {
        let json = to_json(
            ReadFileOutput::PermissionDenied("Permission denied: /etc/shadow".into()).into(),
        );
        assert_eq!(
            json,
            json!({"type": "ReadFile", "PermissionDenied": "Permission denied: /etc/shadow"})
        );
    }
    #[test]
    fn read_file_too_large_json() {
        let json = to_json(
            ReadFileOutput::FileTooLarge(
                "File content (37044 tokens) exceeds maximum allowed tokens (25000 tokens).".into(),
            )
            .into(),
        );
        assert_eq!(
            json,
            json!({"type": "ReadFile", "FileTooLarge": "File content (37044 tokens) exceeds maximum allowed tokens (25000 tokens)."})
        );
    }
    #[test]
    fn read_file_generic_error_json() {
        let json = to_json(ReadFileOutput::FileReadError("Failed to read file".into()).into());
        assert_eq!(
            json,
            json!({"type": "ReadFile", "FileReadError": "Failed to read file"})
        );
    }
    #[test]
    fn read_file_image_size_error_json() {
        let json = to_json(ReadFileOutput::ImageSizeError("Image too large".into()).into());
        assert_eq!(
            json,
            json!({"type": "ReadFile", "ImageSizeError": "Image too large"})
        );
    }
    #[test]
    fn list_dir_not_found_json() {
        let json = to_json(ListDirOutput::NotFound("does not exist".into()).into());
        assert_eq!(
            json,
            json!({"type": "ListDir", "NotFound": "does not exist"})
        );
    }
    #[test]
    fn list_dir_is_a_file_json() {
        let json = to_json(ListDirOutput::IsAFile("is a file".into()).into());
        assert_eq!(json, json!({"type": "ListDir", "IsAFile": "is a file"}));
    }
    #[test]
    fn list_dir_not_a_directory_json() {
        let json = to_json(ListDirOutput::NotADirectory("is not a directory".into()).into());
        assert_eq!(
            json,
            json!({"type": "ListDir", "NotADirectory": "is not a directory"})
        );
    }
    #[test]
    fn list_dir_permission_denied_json() {
        let json = to_json(ListDirOutput::PermissionDenied("Permission denied".into()).into());
        assert_eq!(
            json,
            json!({"type": "ListDir", "PermissionDenied": "Permission denied"})
        );
    }
    #[test]
    fn list_dir_generic_error_json() {
        let json = to_json(ListDirOutput::Error("Some error".into()).into());
        assert_eq!(json, json!({"type": "ListDir", "Error": "Some error"}));
    }
    #[test]
    fn search_replace_file_not_found_json() {
        let json = to_json(SearchReplaceOutput::FileNotFound("not found".into()).into());
        assert_eq!(
            json,
            json!({"type": "SearchReplace", "FileNotFound": "not found"})
        );
    }
    #[test]
    fn search_replace_no_matches_json() {
        let json = to_json(
            SearchReplaceOutput::NoMatchesFound(crate::types::output::NoMatchesFoundError {
                message: "no matches".into(),
                file_path: std::path::PathBuf::from("/project/src/main.c"),
                file_snapshot_at_edit: None,
            })
            .into(),
        );
        assert_eq!(
            json,
            json!({
                "type": "SearchReplace",
                "NoMatchesFound": {
                    "message": "no matches",
                    "file_path": "/project/src/main.c"
                }
            })
        );
    }
    #[test]
    fn search_replace_no_matches_omits_file_snapshot_from_json() {
        let err = crate::types::output::NoMatchesFoundError {
            message: "no matches".into(),
            file_path: std::path::PathBuf::from("/project/secret.txt"),
            file_snapshot_at_edit: Some("SECRET_PAYLOAD".repeat(20)),
        };
        let json = to_json(SearchReplaceOutput::NoMatchesFound(err).into());
        let inner = json
            .get("NoMatchesFound")
            .and_then(|v| v.as_object())
            .expect("NoMatchesFound object");
        assert_eq!(inner.len(), 2);
        assert!(!inner.contains_key("file_snapshot_at_edit"));
        assert!(!json.to_string().contains("SECRET_PAYLOAD"));
    }
    #[test]
    fn search_replace_multiple_matches_json() {
        let json = to_json(SearchReplaceOutput::MultipleMatchesFound("3 matches".into()).into());
        assert_eq!(
            json,
            json!({"type": "SearchReplace", "MultipleMatchesFound": "3 matches"})
        );
    }
    #[test]
    fn search_replace_file_already_exists_json() {
        let json = to_json(SearchReplaceOutput::FileAlreadyExists("exists".into()).into());
        assert_eq!(
            json,
            json!({"type": "SearchReplace", "FileAlreadyExists": "exists"})
        );
    }
    #[test]
    fn search_replace_invalid_input_json() {
        let json = to_json(SearchReplaceOutput::InvalidInput("same strings".into()).into());
        assert_eq!(
            json,
            json!({"type": "SearchReplace", "InvalidInput": "same strings"})
        );
    }
    #[test]
    fn search_replace_filename_too_long_json() {
        let json = to_json(SearchReplaceOutput::FilenameTooLong("name too long".into()).into());
        assert_eq!(
            json,
            json!({"type": "SearchReplace", "FilenameTooLong": "name too long"})
        );
    }
    #[test]
    fn kill_task_result_json() {
        let json = to_json(
            KillTaskOutput::Result(KillTaskResult {
                task_id: "task-1".into(),
                outcome: "killed".into(),
                message: "Task was terminated successfully".into(),
            })
            .into(),
        );
        assert_eq!(json["type"], "KillTask");
        assert!(json.get("Result").is_some(), "missing Result key: {json}");
        assert_eq!(json["Result"]["task_id"], "task-1");
        assert_eq!(json["Result"]["outcome"], "killed");
    }
    #[test]
    fn kill_task_not_found_json() {
        let json = to_json(
            KillTaskOutput::TaskNotFound(
                "Task abc not found. No background tasks exist in this session.".into(),
            )
            .into(),
        );
        assert_eq!(
            json,
            json!({
                "type": "KillTask",
                "TaskNotFound": "Task abc not found. No background tasks exist in this session."
            })
        );
    }
    #[test]
    fn kill_task_not_found_round_trip() {
        let original = KillTaskOutput::TaskNotFound("not found".into());
        let serialized = serde_json::to_value(&original).unwrap();
        let deserialized: KillTaskOutput = serde_json::from_value(serialized).unwrap();
        assert!(
            matches!(deserialized, KillTaskOutput::TaskNotFound(ref msg) if msg == "not found")
        );
    }
    #[test]
    fn task_output_result_json() {
        let json = to_json(
            TaskOutputOutput::Result(TaskOutputResult {
                task_id: "task-1".into(),
                command: "sleep 10".into(),
                status: "running".into(),
                exit_code: None,
                started: "2026-03-09T00:00:00Z".into(),
                ended: None,
                duration_secs: 5.0,
                output: "hello".into(),
                output_file: "/tmp/task-1.log".into(),
                truncated: false,
                truncation_hint: String::new(),
                raw_output_bytes: 5,
            })
            .into(),
        );
        assert_eq!(json["type"], "TaskOutput");
        assert!(json.get("Result").is_some(), "missing Result key: {json}");
        assert_eq!(json["Result"]["task_id"], "task-1");
        assert_eq!(json["Result"]["status"], "running");
    }
    /// The single-task detail view is duration-only: absolute `started` /
    /// `ended` instants stay on the wire struct but must not reach the prompt.
    #[test]
    fn task_output_prompt_is_duration_only() {
        let out = ToolOutput::TaskOutput(TaskOutputOutput::Result(TaskOutputResult {
            task_id: "task-1".into(),
            command: "sleep 10".into(),
            status: "completed".into(),
            exit_code: Some(0),
            started: "2026-03-09T00:00:00Z".into(),
            ended: Some("2026-03-09T00:00:05Z".into()),
            duration_secs: 5.0,
            output: "hello".into(),
            output_file: "/tmp/task-1.log".into(),
            truncated: false,
            truncation_hint: String::new(),
            raw_output_bytes: 5,
        }));
        let prompt = out.to_prompt_format();
        assert!(prompt.contains("Duration: 5.00s"), "{prompt}");
        assert!(prompt.contains("Output File: /tmp/task-1.log"), "{prompt}");
        assert!(
            !prompt.contains("Started") && !prompt.contains("Ended"),
            "absolute instants must not be model-visible: {prompt}"
        );
        assert!(
            !prompt.contains("2026-03-09"),
            "no wall-clock date may survive into the prompt: {prompt}"
        );
    }
    #[test]
    fn task_output_prompt_omits_empty_output_file() {
        let out = ToolOutput::TaskOutput(TaskOutputOutput::Result(TaskOutputResult {
            task_id: "task-2".into(),
            command: "true".into(),
            status: "completed".into(),
            exit_code: Some(0),
            started: String::new(),
            ended: None,
            duration_secs: 0.1,
            output: "done".into(),
            output_file: String::new(),
            truncated: false,
            truncation_hint: String::new(),
            raw_output_bytes: 4,
        }));
        let prompt = out.to_prompt_format();
        assert!(!prompt.contains("Output File"), "{prompt}");
    }
    fn make_result(status: &str, raw_output_bytes: usize) -> TaskOutputResult {
        TaskOutputResult {
            task_id: "t".into(),
            command: "cmd".into(),
            status: status.into(),
            exit_code: None,
            started: "2026-01-01T00:00:00Z".into(),
            ended: None,
            duration_secs: 1.0,
            output: "x".repeat(raw_output_bytes.min(100)),
            output_file: "/tmp/t.log".into(),
            truncated: raw_output_bytes > 100,
            truncation_hint: String::new(),
            raw_output_bytes,
        }
    }
    /// Identical status + raw_output_bytes → same signature.
    #[test]
    fn progress_signature_same_when_no_progress() {
        let a = make_result("running", 1000);
        let b = make_result("running", 1000);
        assert_eq!(
            a.progress_signature(),
            b.progress_signature(),
            "identical results must produce the same progress signature"
        );
    }
    /// Growing raw_output_bytes must produce a different signature even when the
    /// formatted output length is unchanged (truncated output scenario).
    #[test]
    fn progress_signature_differs_on_raw_output_growth() {
        let stagnant = make_result("running", 500_000);
        let growing = make_result("running", 500_001);
        assert_ne!(
            stagnant.progress_signature(),
            growing.progress_signature(),
            "raw output growth must produce a different progress signature"
        );
    }
    /// Status change must produce a different signature.
    #[test]
    fn progress_signature_differs_on_status_change() {
        let running = make_result("running", 100);
        let completed = make_result("completed", 100);
        assert_ne!(
            running.progress_signature(),
            completed.progress_signature(),
            "status change must produce a different progress signature"
        );
    }
    /// `raw_output_bytes` field is populated in JSON output.
    #[test]
    fn task_output_result_raw_output_bytes_in_json() {
        let json = to_json(
            TaskOutputOutput::Result(TaskOutputResult {
                task_id: "t2".into(),
                command: "cmd".into(),
                status: "running".into(),
                exit_code: None,
                started: "2026-03-15T00:00:00Z".into(),
                ended: None,
                duration_secs: 0.0,
                output: "hello world".into(),
                output_file: "/tmp/t2.log".into(),
                truncated: false,
                truncation_hint: String::new(),
                raw_output_bytes: 11,
            })
            .into(),
        );
        assert_eq!(json["Result"]["raw_output_bytes"], 11);
    }
    #[test]
    fn task_output_not_found_json() {
        let json = to_json(
            TaskOutputOutput::TaskNotFound(
                "Task xyz not found. Known task IDs: [task-1, task-2]".into(),
            )
            .into(),
        );
        assert_eq!(
            json,
            json!({
                "type": "TaskOutput",
                "TaskNotFound": "Task xyz not found. Known task IDs: [task-1, task-2]"
            })
        );
    }
    #[test]
    fn task_output_not_found_round_trip() {
        let original = TaskOutputOutput::TaskNotFound("not found".into());
        let serialized = serde_json::to_value(&original).unwrap();
        let deserialized: TaskOutputOutput = serde_json::from_value(serialized).unwrap();
        assert!(
            matches!(deserialized, TaskOutputOutput::TaskNotFound(ref msg) if msg == "not found")
        );
    }
    #[test]
    fn todo_write_success_json() {
        let json = to_json(
            TodoWriteOutput::TodosUpdated(TodoWriteSuccess {
                summary_for_prompt: "- [pending] 1: Task A\n".into(),
                todos: vec![TodoItem {
                    content: "Task A".into(),
                    priority: TodoPriority::Medium,
                    status: TodoStatus::Pending,
                    meta: None,
                }],
                state: TodoState::default(),
            })
            .into(),
        );
        assert_eq!(json["type"], "Todo");
        assert!(
            json.get("TodosUpdated").is_some(),
            "missing TodosUpdated key: {json}"
        );
        assert_eq!(
            json["TodosUpdated"]["summary_for_prompt"],
            "- [pending] 1: Task A\n"
        );
        assert_eq!(json["TodosUpdated"]["todos"][0]["content"], "Task A");
        assert_eq!(json["TodosUpdated"]["todos"][0]["status"], "pending");
    }
    #[test]
    fn todo_write_duplicate_id_json() {
        let json = to_json(
            TodoWriteOutput::DuplicateId(
                "Duplicate todo ID in request: \"dup\". Each todo item must have a unique ID."
                    .into(),
            )
            .into(),
        );
        assert_eq!(
            json,
            json!({
                "type": "Todo",
                "DuplicateId": "Duplicate todo ID in request: \"dup\". Each todo item must have a unique ID."
            })
        );
    }
    #[test]
    fn todo_write_duplicate_id_round_trip() {
        let original = TodoWriteOutput::DuplicateId("dup id".into());
        let serialized = serde_json::to_value(&original).unwrap();
        let deserialized: TodoWriteOutput = serde_json::from_value(serialized).unwrap();
        assert!(matches!(deserialized, TodoWriteOutput::DuplicateId(ref msg) if msg == "dup id"));
    }
    #[test]
    fn todo_write_success_round_trip() {
        let original = TodoWriteOutput::TodosUpdated(TodoWriteSuccess {
            summary_for_prompt: "summary".into(),
            todos: vec![TodoItem {
                content: "task".into(),
                priority: TodoPriority::High,
                status: TodoStatus::InProgress,
                meta: None,
            }],
            state: TodoState::default(),
        });
        let serialized = serde_json::to_value(&original).unwrap();
        let deserialized: TodoWriteOutput = serde_json::from_value(serialized).unwrap();
        match deserialized {
            TodoWriteOutput::TodosUpdated(s) => {
                assert_eq!(s.summary_for_prompt, "summary");
                assert_eq!(s.todos.len(), 1);
                assert_eq!(s.todos[0].content, "task");
                assert_eq!(s.todos[0].status, TodoStatus::InProgress);
                assert_eq!(s.todos[0].priority, TodoPriority::High);
            }
            other => panic!("expected TodosUpdated, got {other:?}"),
        }
    }
    #[test]
    fn subagent_completed_prompt_format_includes_resume_footer() {
        let output = ToolOutput::SubagentCompleted(SubagentCompletedOutput {
            output: "I found the auth middleware.".into(),
            subagent_id: "019e0000-0000-7000-8000-0000000000bb".into(),
            subagent_type: "explore".into(),
            tool_calls: 5,
            turns: 2,
            duration_ms: 3000,
            worktree_path: None,
            persona: None,
            resume_from_hint: "019e0000-0000-7000-8000-0000000000bb".into(),
            persona_hint: None,
        });
        let rendered = output.to_prompt_format();
        assert!(
            rendered.contains("I found the auth middleware."),
            "original output preserved"
        );
        assert!(
            rendered.contains("subagent_id: 019e0000-0000-7000-8000-0000000000bb"),
            "subagent_id visible in rendered text"
        );
        assert!(
            rendered.contains("resume_from=\"019e0000-0000-7000-8000-0000000000bb\""),
            "resume_from hint with correct ID"
        );
        assert!(
            rendered.contains("subagent_type: explore"),
            "subagent_type visible"
        );
        assert!(
            rendered.contains("<subagent_result>"),
            "wrapped in subagent_result tag"
        );
        assert!(
            !rendered.contains("persona"),
            "no persona hint when persona is None"
        );
    }
    #[test]
    fn subagent_completed_prompt_format_includes_persona_hint() {
        let output = ToolOutput::SubagentCompleted(SubagentCompletedOutput {
            output: "Done implementing.".into(),
            subagent_id: "abc-123".into(),
            subagent_type: "general-purpose".into(),
            tool_calls: 10,
            turns: 3,
            duration_ms: 5000,
            worktree_path: None,
            persona: Some("implementer".into()),
            resume_from_hint: "abc-123".into(),
            persona_hint: Some("implementer".into()),
        });
        let rendered = output.to_prompt_format();
        assert!(
            rendered.contains("resume_from=\"abc-123\""),
            "resume hint present"
        );
        assert!(
            rendered.contains("persona=\"implementer\""),
            "persona hint present"
        );
        assert!(
            rendered.contains("Pass the same persona when resuming"),
            "persona instruction present"
        );
    }
    #[test]
    fn subagent_completed_prompt_format_with_worktree() {
        let output = ToolOutput::SubagentCompleted(SubagentCompletedOutput {
            output: "Changes committed.".into(),
            subagent_id: "wt-agent".into(),
            subagent_type: "general-purpose".into(),
            tool_calls: 3,
            turns: 1,
            duration_ms: 2000,
            worktree_path: Some("/tmp/grow-worktree/wt-agent".into()),
            persona: None,
            resume_from_hint: "wt-agent".into(),
            persona_hint: None,
        });
        let rendered = output.to_prompt_format();
        assert!(
            rendered.contains("<worktree_path>/tmp/grow-worktree/wt-agent</worktree_path>"),
            "worktree_path preserved"
        );
        assert!(
            rendered.contains("resume_from=\"wt-agent\""),
            "resume footer still present with worktree"
        );
    }
    #[test]
    fn subagent_completed_structured_hints_serialize() {
        let output = SubagentCompletedOutput {
            output: "done".into(),
            subagent_id: "sub-abc-123".into(),
            subagent_type: "general-purpose".into(),
            tool_calls: 5,
            turns: 2,
            duration_ms: 3000,
            worktree_path: None,
            persona: Some("implementer".into()),
            resume_from_hint: "sub-abc-123".into(),
            persona_hint: Some("implementer".into()),
        };
        let json = serde_json::to_value(&output).unwrap();
        assert_eq!(json["resume_from_hint"], "sub-abc-123");
        assert_eq!(json["persona_hint"], "implementer");
        assert_eq!(json["subagent_id"], json["resume_from_hint"]);
    }
    #[test]
    fn plan_control_prompt_format_is_the_shell_validated_result() {
        let output = ToolOutput::PlanControl(PlanControlOutput {
            message: "approved".into(),
        });
        assert_eq!(output.to_prompt_format(), "approved");
    }
    #[test]
    fn subagent_completed_hints_absent_when_no_persona() {
        let output = SubagentCompletedOutput {
            output: "done".into(),
            subagent_id: "sub-xyz".into(),
            subagent_type: "explore".into(),
            tool_calls: 1,
            turns: 1,
            duration_ms: 500,
            worktree_path: None,
            persona: None,
            resume_from_hint: "sub-xyz".into(),
            persona_hint: None,
        };
        let json = serde_json::to_value(&output).unwrap();
        assert_eq!(json["resume_from_hint"], "sub-xyz");
        assert!(
            json.get("persona_hint").is_none(),
            "persona_hint should be absent when None"
        );
    }
    fn make_pdf_page_images(
        page_numbers: &[usize],
        total_pages: usize,
        file_size: usize,
    ) -> PdfPageImages {
        PdfPageImages {
            pages: page_numbers
                .iter()
                .map(|&n| PdfPageImage {
                    data: format!("base64data_page{n}"),
                    mime_type: "image/jpeg".to_string(),
                    page_number: n,
                })
                .collect(),
            total_pages,
            file_size,
        }
    }
    #[test]
    fn pdf_page_images_to_prompt_format() {
        let pdf = make_pdf_page_images(&[1, 2, 3], 25, 102400);
        let output = ToolOutput::ReadFile(ReadFileOutput::PdfPageImages(pdf));
        let text = output.to_prompt_format();
        assert!(text.contains("3 pages rendered"), "got: {text}");
        assert!(text.contains("pages 1, 2, 3"), "got: {text}");
        assert!(text.contains("25 pages"), "got: {text}");
        assert!(text.contains("100.0 KB"), "got: {text}");
    }
    #[test]
    fn pdf_page_images_to_prompt_format_single_page() {
        let pdf = make_pdf_page_images(&[5], 10, 51200);
        let output = ToolOutput::ReadFile(ReadFileOutput::PdfPageImages(pdf));
        let text = output.to_prompt_format();
        assert!(text.contains("1 pages rendered"), "got: {text}");
        assert!(text.contains("pages 5"), "got: {text}");
        assert!(text.contains("10 pages"), "got: {text}");
        assert!(text.contains("50.0 KB"), "got: {text}");
    }
    #[test]
    fn pdf_page_images_json_round_trip() {
        let pdf = make_pdf_page_images(&[1, 3], 20, 8192);
        let output = ToolOutput::ReadFile(ReadFileOutput::PdfPageImages(pdf));
        let json = to_json(output);
        assert_eq!(json["type"], "ReadFile");
        assert!(
            json.get("PdfPageImages").is_some(),
            "missing PdfPageImages key: {json}"
        );
        let inner = &json["PdfPageImages"];
        assert_eq!(inner["total_pages"], 20);
        assert_eq!(inner["file_size"], 8192);
        assert_eq!(inner["pages"].as_array().unwrap().len(), 2);
        assert_eq!(inner["pages"][0]["page_number"], 1);
        assert_eq!(inner["pages"][1]["page_number"], 3);
    }
    fn sample_bash(exit_code: i32, output: &[u8], timed_out: bool) -> BashOutput {
        BashOutput {
            output: output.to_vec(),
            output_for_prompt: String::new(),
            exit_code,
            command: "cmd".into(),
            truncated: false,
            signal: None,
            timed_out,
            description: None,
            current_dir: "/tmp".into(),
            output_file: String::new(),
            total_bytes: output.len(),
            output_delta: None,
            was_bare_echo: false,
        }
    }
    fn assert_cer(
        resp: &tool_runtime::ToolChatCompletionResponse,
        stdout: &str,
        exit_code: i32,
        timed_out: bool,
    ) {
        let result = resp.result.as_ref().unwrap();
        let cer = result.code_execution_result.as_ref().unwrap();
        assert_eq!(cer.stdout, stdout);
        assert!(cer.stderr.is_empty());
        assert_eq!(cer.exit_code, exit_code);
        assert_eq!(cer.command_timed_out, timed_out);
        assert_eq!(result.sender, "assistant");
        assert_eq!(result.message_tag.as_deref(), Some("raw_function_result"));
    }
    fn bg_started() -> BackgroundTaskStarted {
        BackgroundTaskStarted {
            task_id: "t1".into(),
            task_type: "bash".into(),
            output_file: "/tmp/out".into(),
            status: "running".into(),
            command: "sleep 99".into(),
            summary: "running".into(),
            retrieval_hint: String::new(),
            pre_formatted: None,
            pid: None,
        }
    }
    #[test]
    fn bash_output_chat_completion_carries_exit_and_stdout() {
        let resp =
            tool_runtime::ToolOutput::chat_completion_output(&sample_bash(0, b"hello\n", false))
                .unwrap();
        assert_cer(&resp, "hello\n", 0, false);
        assert!(resp.result.as_ref().unwrap().extra.is_empty());
    }
    #[test]
    fn bash_output_chat_completion_empty_stdout_still_emits() {
        let resp =
            tool_runtime::ToolOutput::chat_completion_output(&sample_bash(0, b"", false)).unwrap();
        assert_cer(&resp, "", 0, false);
    }
    #[test]
    fn bash_output_chat_completion_timeout_and_nonzero_exit() {
        let resp =
            tool_runtime::ToolOutput::chat_completion_output(&sample_bash(124, b"partial", true))
                .unwrap();
        assert_cer(&resp, "partial", 124, true);
    }
    #[test]
    fn bash_output_chat_completion_lossy_utf8() {
        let resp = tool_runtime::ToolOutput::chat_completion_output(&sample_bash(
            1,
            &[0x66, 0x6f, 0x6f, 0xff, 0x62, 0x61, 0x72],
            false,
        ))
        .unwrap();
        let stdout = &resp
            .result
            .as_ref()
            .unwrap()
            .code_execution_result
            .as_ref()
            .unwrap()
            .stdout;
        assert!(stdout.starts_with("foo"));
        assert!(stdout.ends_with("bar"));
        assert!(stdout.contains('\u{FFFD}'));
    }
    #[test]
    fn bash_output_chat_completion_truncation_marker_and_extra() {
        let mut bash = sample_bash(0, b"head...tail", false);
        bash.truncated = true;
        bash.total_bytes = 50_000;
        bash.output_file = "/tmp/out.log".into();
        let resp = tool_runtime::ToolOutput::chat_completion_output(&bash).unwrap();
        let result = resp.result.as_ref().unwrap();
        let stdout = &result.code_execution_result.as_ref().unwrap().stdout;
        assert!(stdout.starts_with("head...tail"));
        assert!(stdout.contains("[truncated:"));
        assert!(stdout.contains("full output at: /tmp/out.log"));
        assert_eq!(
            result.extra.get("truncated"),
            Some(&serde_json::Value::Bool(true))
        );
        assert_eq!(
            result.extra.get("total_bytes"),
            Some(&serde_json::Value::from(50_000u64))
        );
        assert_eq!(
            result.extra.get("output_file"),
            Some(&serde_json::Value::String("/tmp/out.log".into()))
        );
        assert!(
            result
                .code_execution_result
                .as_ref()
                .unwrap()
                .stderr
                .is_empty()
        );
    }
    #[test]
    fn bash_tool_output_foreground_delegates_background_skips() {
        let resp = tool_runtime::ToolOutput::chat_completion_output(&BashToolOutput::Bash(
            sample_bash(0, b"ok", false),
        ))
        .unwrap();
        assert_cer(&resp, "ok", 0, false);
        assert!(
            tool_runtime::ToolOutput::chat_completion_output(
                &BashToolOutput::BackgroundTaskStarted(bg_started())
            )
            .is_none()
        );
    }
    #[test]
    fn aggregate_tool_output_bash_delegates_background_skips() {
        let resp = tool_runtime::ToolOutput::chat_completion_output(&ToolOutput::Bash(
            sample_bash(0, b"agg", false),
        ))
        .unwrap();
        assert_cer(&resp, "agg", 0, false);
        assert!(
            tool_runtime::ToolOutput::chat_completion_output(&ToolOutput::BackgroundTaskStarted(
                bg_started()
            ))
            .is_none()
        );
        assert!(
            tool_runtime::ToolOutput::chat_completion_output(&ToolOutput::Text(TextOutput::from(
                "noop"
            )))
            .is_none()
        );
    }
    fn sample_run_result(output: ToolOutput) -> ToolRunResult {
        ToolRunResult {
            prompt_text: "prompt".into(),
            effective_tool_name: None,
            output,
        }
    }
    fn bash_tool_id() -> tool_protocol::ToolId {
        tool_protocol::ToolId::new("bash").unwrap()
    }
    #[test]
    fn into_typed_tool_output_preserves_bash_foreground_cco() {
        let run = sample_run_result(ToolOutput::Bash(sample_bash(0, b"hello-cco", false)));
        let expected_value = serde_json::to_value(&run).unwrap();
        let typed = run.into_typed_tool_output(bash_tool_id());
        assert_eq!(typed.value, expected_value);
        assert_eq!(typed.tool_id, bash_tool_id());
        assert_cer(
            typed
                .chat_completion_output
                .as_ref()
                .expect("bash foreground must preserve chat_completion_output"),
            "hello-cco",
            0,
            false,
        );
        let dropped = tool_runtime::TypedToolOutput::from_value(typed.tool_id, expected_value);
        assert!(dropped.chat_completion_output.is_none());
    }
    #[test]
    fn into_typed_tool_output_non_bash_cco_is_none() {
        let run = sample_run_result(ToolOutput::Text(TextOutput::from("noop")));
        let typed = run.into_typed_tool_output(tool_protocol::ToolId::new("text").unwrap());
        assert!(typed.chat_completion_output.is_none());
    }
    #[test]
    fn into_typed_tool_output_background_task_cco_is_none() {
        let run = sample_run_result(ToolOutput::BackgroundTaskStarted(bg_started()));
        let typed = run.into_typed_tool_output(bash_tool_id());
        assert!(typed.chat_completion_output.is_none());
    }
    #[test]
    fn typed_tool_output_preserving_cco_serialize_failure_yields_null_value() {
        struct AlwaysFailSerialize;
        impl Serialize for AlwaysFailSerialize {
            fn serialize<S: serde::Serializer>(&self, _serializer: S) -> Result<S::Ok, S::Error> {
                Err(serde::ser::Error::custom("intentional serialize failure"))
            }
        }
        let bash = ToolOutput::Bash(sample_bash(0, b"kept-cco", false));
        let typed = typed_tool_output_preserving_cco(bash_tool_id(), &AlwaysFailSerialize, &bash);
        assert_eq!(typed.value, serde_json::Value::Null);
        assert_cer(
            typed
                .chat_completion_output
                .as_ref()
                .expect("CCO still attached when payload serialize fails"),
            "kept-cco",
            0,
            false,
        );
    }
}
