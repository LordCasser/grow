//! Skill prompt loading and expansion.
//!
//! Skills are user-defined prompts stored as Markdown files that can be invoked
//! through skill references and slash commands (e.g., /commit).

use crate::implementations::skills::types::SkillInfo;

/// Build the formatted skill message shown to the model.
///
/// Canonical formatter for skill content injection. Used by the skill tool
/// (invocation path), TUI slash commands, the pager, and agent definition
/// preloading — every path that surfaces a skill to the model routes
/// through this function so the presentation stays consistent.
///
/// Format: `<skill>` envelope with name, description, and path attributes
/// wraps the raw markdown body. The open/close tags give the model a clear
/// identity and boundary — everything inside is additional instructions
/// to follow, not a program being invoked.
///
/// ```text
/// <skill name="{name}" description="{description}" path="{path}">
/// {body}
/// </skill>
/// ```
///
/// Used on both the invocation path (skill tool, slash expansion) and
/// preloading paths (agent definitions) — no separate instruct prefix.
pub fn build_skill_message(skill: &SkillInfo, content: &str) -> String {
    format!(
        "<skill name=\"{}\" description=\"{}\" path=\"{}\">\n{}\n</skill>",
        escape_xml(&skill.name),
        escape_xml(&skill.description),
        escape_xml(&skill.path),
        content
    )
}

/// Build a `<skill>` block for user-invoked skill expansion.
///
/// Used in the `<skill_information>` envelope when skills are expanded
/// at prompt-assembly time (the new zero-round-trip path). Includes the
/// `args` attribute so the model knows what arguments were provided.
///
/// ```text
/// <skill name="commit" args="fix typo">
/// {body}
/// </skill>
/// ```
pub fn build_skill_block(name: &str, args: &str, content: &str) -> String {
    let name = escape_xml(name);
    let args = escape_xml(args);
    if args.is_empty() {
        format!("<skill name=\"{name}\">\n{content}\n</skill>")
    } else {
        format!("<skill name=\"{name}\" args=\"{args}\">\n{content}\n</skill>")
    }
}

/// A skill reference for the `<skills_referenced>` index inside
/// `<skill_information>`.
pub struct SkillRef<'a> {
    pub name: &'a str,
    pub path: &'a str,
}

/// Wrap one or more `<skill>` blocks in a `<skill_information>` envelope.
///
/// Includes a `<skills_referenced>` index listing each skill's name and
/// full path so the model can quickly see what skills are loaded and where
/// they live on disk.
///
/// Returns an empty string if no blocks are provided. The caller should
/// append the returned string directly after the `<user_query>` block
/// when assembling the user message.
pub fn build_skill_information(skill_blocks: &[String], refs: &[SkillRef<'_>]) -> String {
    if skill_blocks.is_empty() {
        return String::new();
    }
    let mut out = String::from("<skill_information>\n");

    // Index of referenced skills with their paths, deduplicated by (name, path)
    // while preserving the original insertion order.
    if !refs.is_empty() {
        let mut seen = Vec::new();
        let deduped: Vec<_> = refs
            .iter()
            .filter(|r| {
                let key = (r.name, r.path);
                if seen.contains(&key) {
                    false
                } else {
                    seen.push(key);
                    true
                }
            })
            .collect();
        out.push_str("<skills_referenced>\n");
        for r in deduped {
            out.push_str(&format!(
                "<skill name=\"{}\" path=\"{}\"/>\n",
                escape_xml(r.name),
                escape_xml(r.path)
            ));
        }
        out.push_str("</skills_referenced>\n");
    }

    out.push_str(&skill_blocks.join("\n"));
    out.push_str("\n</skill_information>");
    out
}

/// Instruction prefix prepended to skill messages on the invocation path.
///
/// Format a skill name with its scope prefix (e.g. `"user:commit"`).
pub fn format_skill_name(skill: &SkillInfo) -> String {
    // Plugin skills use "plugin-name:skill-name" so skills from different
    // plugins don't collide.  Other scopes use "scope:skill-name".
    if let Some(ref pn) = skill.plugin_name {
        return format!("{pn}:{}", skill.name);
    }
    format!("{}:{}", skill.scope.as_ref(), skill.name)
}

/// Extract a clean display string from skill XML markup.
///
/// Skill invocations are encoded on the wire as XML tags:
/// ```text
/// <command-name>NAME</command-name>
/// <command-message>/NAME</command-message>
/// <command-args>ARGS</command-args>           (optional)
/// ```
///
/// Returns `Some("/NAME ARGS")` if the text contains skill markup, `None` otherwise.
/// Falls back to `<command-name>` when `<command-message>` is absent (e.g. stored
/// session titles that were truncated to just the first XML tag).
pub fn extract_skill_display_text(text: &str) -> Option<String> {
    let name_open = "<command-name>";
    let name_close = "</command-name>";
    if !text.contains(name_open) {
        return None;
    }

    // Try <command-message> first (canonical wire format).
    let command = 'cmd: {
        let cmd_open = "<command-message>";
        let cmd_close = "</command-message>";
        let start = match text.find(cmd_open) {
            Some(s) => s + cmd_open.len(),
            None => break 'cmd None,
        };
        text[start..]
            .find(cmd_close)
            .map(|rel| &text[start..start + rel])
    };

    if let Some(cmd) = command.filter(|c| !c.is_empty()) {
        let args = extract_command_args(text);
        return Some(match args {
            Some(a) => format!("{cmd} {a}"),
            None => cmd.to_string(),
        });
    }

    // Fallback: derive "/NAME" from <command-name>NAME</command-name>.
    let inner = text.find(name_open)? + name_open.len();
    let end = inner + text[inner..].find(name_close)?;
    let name = &text[inner..end];
    if name.is_empty() {
        return None;
    }
    let args = extract_command_args(text);
    Some(match args {
        Some(a) => format!("/{name} {a}"),
        None => format!("/{name}"),
    })
}

/// Extract trimmed args from `<command-args>…</command-args>`, if present and non-empty.
/// Falls back to taking everything after `<command-args>` when the closing tag is
/// missing (a truncated generated display title can end mid-tag).
fn extract_command_args(text: &str) -> Option<&str> {
    let open = "<command-args>";
    let close = "</command-args>";
    let start = text.find(open)? + open.len();
    let end = text[start..]
        .find(close)
        .map_or(text.len(), |rel| start + rel);
    let args = text[start..end].trim();
    if args.is_empty() { None } else { Some(args) }
}

/// Escape XML attribute special characters. Keep skill bodies unchanged.
fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Non-argument substitution inputs for `apply_substitutions`.
///
/// Bundles the four same-typed `Option<&str>` context values so callers name
/// each field by hand and cannot transpose them positionally.
#[derive(Default)]
pub struct SubstitutionContext<'a> {
    pub skill_dir: Option<&'a str>,
    pub session_id: Option<&'a str>,
    pub plugin_root: Option<&'a str>,
    pub plugin_data: Option<&'a str>,
}

/// Apply variable substitutions to skill content.
///
/// Supported variables:
///
/// | Variable | Description |
/// |----------|-------------|
/// | `$ARGUMENTS` | Full arguments string (empty if none) |
/// | `$ARGUMENTS[N]` | Nth argument (0-indexed, whitespace-split) |
/// | `$N` | Positional shorthand within the argument count plus 20 slots |
/// | `${SKILL_DIR}` | Directory containing the SKILL.md |
/// | `${SESSION_ID}` | Current session ID |
/// | `${GROW_PLUGIN_ROOT}` | Plugin root dir (plugin-backed skills) |
/// | `${GROW_PLUGIN_DATA}` | Plugin data dir (plugin-backed skills) |
///
/// The body is treated as argument-aware only when it contains an *argument*
/// token (`$ARGUMENTS`, `$ARGUMENTS[N]`, or `$N`); in that case the args are
/// expanded inline and the `**ARGUMENTS:** ...` suffix is **not** appended.
/// Path/metadata tokens (`${SKILL_DIR}`, `${SESSION_ID}`,
/// `${GROW_PLUGIN_ROOT}`, and `${GROW_PLUGIN_DATA}`) are
/// expanded but do NOT suppress the suffix, so a body that references only a
/// path token still receives its arguments. If no argument token is present,
/// arguments are appended as a suffix in the traditional format for backward
/// compatibility.
///
/// Unknown `$` tokens are left unchanged.
pub fn apply_substitutions(content: &mut String, args: Option<&str>, ctx: &SubstitutionContext) {
    let args_str = args.unwrap_or("");
    let argv: Vec<&str> = if args_str.is_empty() {
        vec![]
    } else {
        args_str.split_whitespace().collect()
    };

    // Track whether an *argument* token consumed the args. Only that suppresses
    // the **ARGUMENTS:** fallback; path/metadata tokens (SKILL_DIR, SESSION_ID,
    // plugin root/data) expand without suppressing it, so a body that uses only
    // a path token still receives its arguments.
    let mut args_substituted = false;

    static TOKENS: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(
            r"\$ARGUMENTS\[[0-9]+\]|\$[0-9]+|\$ARGUMENTS|\$\{(?:SKILL_DIR|SESSION_ID|GROW_PLUGIN_ROOT|GROW_PLUGIN_DATA)\}",
        ).expect("valid skill substitution token pattern")
    });
    // Replacement values are literal output, never input to another pass.
    let expanded = TOKENS.replace_all(content, |captures: &regex::Captures<'_>| {
        let token = captures.get(0).unwrap().as_str();
        if let Some(index) = token
            .strip_prefix("$ARGUMENTS[")
            .and_then(|s| s.strip_suffix(']'))
        {
            args_substituted = true;
            return index
                .parse::<usize>()
                .ok()
                .and_then(|index| argv.get(index))
                .copied()
                .unwrap_or("")
                .to_owned();
        }
        if token == "$ARGUMENTS" {
            args_substituted = true;
            return args_str.to_owned();
        }
        if let Some(index) = token
            .strip_prefix('$')
            .and_then(|s| s.parse::<usize>().ok())
        {
            // Preserve the established shorthand range, so ordinary dollar
            // amounts such as $100 do not consume the argument suffix.
            if index < argv.len().max(1).saturating_add(20) {
                args_substituted = true;
                return argv.get(index).copied().unwrap_or("").to_owned();
            }
        }
        match token {
            "${SKILL_DIR}" => ctx.skill_dir,
            "${SESSION_ID}" => ctx.session_id,
            "${GROW_PLUGIN_ROOT}" => ctx.plugin_root,
            "${GROW_PLUGIN_DATA}" => ctx.plugin_data,
            _ => None,
        }
        .unwrap_or(token)
        .to_owned()
    });
    *content = expanded.into_owned();

    // Append the **ARGUMENTS:** suffix only when no argument token consumed the
    // args (the path/metadata tokens above do not count), preserving args for
    // bodies that reference only a path token.
    if !args_substituted
        && let Some(a) = args
        && !a.is_empty()
    {
        content.push_str("\n\n**ARGUMENTS:** ");
        content.push_str(a);
    }
}

/// Locate the destination after the already-parsed label, preserving source spelling.
fn skill_link_destination_span(source: &str, reference: bool) -> Option<std::ops::Range<usize>> {
    let after_label = if reference {
        source.match_indices("]:").find_map(|(offset, _)| {
            let escapes = source[..offset]
                .bytes()
                .rev()
                .take_while(|b| *b == b'\\')
                .count();
            (escapes % 2 == 0).then_some(offset + 2)
        })?
    } else {
        // Destination/title are not text events. Nested markup (including code
        // and images) remains inside the label; outer wrappers span the whole link.
        let label_end = pulldown_cmark::Parser::new(source)
            .into_offset_iter()
            .map(|(_, range)| range.end)
            .filter(|end| *end < source.len())
            .max()
            .unwrap_or(0);
        label_end + source[label_end..].find("](")? + 2
    };
    let bytes = source.as_bytes();
    let mut start = after_label;
    while bytes.get(start).is_some_and(u8::is_ascii_whitespace) {
        start += 1;
    }
    let angled = bytes.get(start) == Some(&b'<');
    let mut cursor = start + usize::from(angled);
    let mut depth = 0usize;
    while let Some(&byte) = bytes.get(cursor) {
        if byte == b'\\' {
            cursor += 2;
            continue;
        }
        if angled {
            if byte == b'>' {
                return Some(start..cursor + 1);
            }
        } else {
            match byte {
                b'(' => depth += 1,
                b')' if depth == 0 => break,
                b')' => depth -= 1,
                b if b.is_ascii_whitespace() => break,
                _ => {}
            }
        }
        cursor += 1;
    }
    (!angled && cursor <= source.len() && cursor > start).then_some(start..cursor)
}

fn skill_link_destination_text(path: &str, angled: bool) -> String {
    let angled = angled || path.chars().any(char::is_whitespace);
    let mut result = String::new();
    if angled {
        result.push('<');
    }
    for ch in path.chars() {
        match ch {
            '&' => result.push_str("&amp;"),
            '\\' | '<' | '>' => {
                result.push('\\');
                result.push(ch);
            }
            '(' | ')' if !angled => {
                result.push('\\');
                result.push(ch);
            }
            _ => result.push(ch),
        }
    }
    if angled {
        result.push('>');
    }
    result
}

/// Resolve relative markdown link/image targets to absolute paths when
/// the referenced file exists inside `skill_dir`.
pub fn resolve_skill_internal_links(body: &str, skill_dir: &std::path::Path) -> String {
    use pulldown_cmark::{Event, LinkType, Options, Parser, Tag};

    let options =
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    let parser = Parser::new_ext(body, options);

    let ref_def_spans: std::collections::HashMap<String, std::ops::Range<usize>> = parser
        .reference_definitions()
        .iter()
        .map(|(label, def)| (label.to_string(), def.span.clone()))
        .collect();

    let mut edits: Vec<(std::ops::Range<usize>, String)> = Vec::new();

    for (event, event_range) in parser.into_offset_iter() {
        let (url, link_type, id) = match &event {
            Event::Start(Tag::Link {
                dest_url,
                link_type,
                id,
                ..
            }) => (dest_url, *link_type, id.as_ref()),
            Event::Start(Tag::Image {
                dest_url,
                link_type,
                id,
                ..
            }) => (dest_url, *link_type, id.as_ref()),
            _ => continue,
        };

        if url.is_empty() {
            continue;
        }

        let resolved = skill_dir.join(url.as_ref());
        if !resolved.exists() || resolved.to_string_lossy() == url.as_ref() {
            continue;
        }

        let canonical = match dunce::canonicalize(&resolved) {
            Ok(p) => p,
            Err(_) => continue,
        };
        let skill_dir_canonical = match dunce::canonicalize(skill_dir) {
            Ok(p) => p,
            Err(_) => continue,
        };
        if !canonical.starts_with(&skill_dir_canonical) {
            continue;
        }

        let span = match link_type {
            LinkType::Inline => Some(event_range.clone()),
            LinkType::Reference | LinkType::Collapsed | LinkType::Shortcut => {
                ref_def_spans.get(id).cloned()
            }
            _ => None,
        };
        let Some(span) = span else { continue };
        let source = &body[span.clone()];
        let Some(destination) = skill_link_destination_span(source, link_type != LinkType::Inline)
        else {
            continue;
        };
        let replacement = skill_link_destination_text(
            &resolved.to_string_lossy(),
            source[destination.clone()].starts_with('<'),
        );
        let range = span.start + destination.start..span.start + destination.end;
        if !edits.iter().any(|(existing, _)| *existing == range) {
            edits.push((range, replacement));
        }
    }

    if edits.is_empty() {
        return body.to_string();
    }

    edits.sort_by(|a, b| b.0.start.cmp(&a.0.start));
    let mut result = body.to_string();
    for (range, replacement) in edits {
        result.replace_range(range, &replacement);
    }
    result
}

/// Split only complete frontmatter delimiter lines, matching the disk reader.
pub(super) fn split_skill_frontmatter(content: &str) -> Option<(&str, &str)> {
    let content = content.trim_start();
    let mut lines = content.split_inclusive('\n');
    let opening = lines.next()?;
    if opening.trim() != "---" {
        return None;
    }
    let yaml_start = opening.len();
    let mut offset = yaml_start;
    for line in lines {
        if line.trim() == "---" {
            return Some((
                &content[yaml_start..offset],
                &content[offset + line.len()..],
            ));
        }
        offset += line.len();
    }
    None
}

/// Extract the skill body after complete YAML frontmatter, trimming leading whitespace.
/// Without complete frontmatter, retain the content after leading whitespace.
pub fn extract_skill_body(content: &str) -> String {
    split_skill_frontmatter(content)
        .map_or(content, |(_, body)| body)
        .trim_start()
        .to_string()
}

/// Load skill content from its file, stripping YAML frontmatter.
///
/// Public entrypoint for the shell crate to load skill content at
/// prompt-assembly time. Preloaded bodies, including empty bodies, are snapshots.
pub async fn load_skill_content(skill: &SkillInfo) -> Result<String, String> {
    // Producers strip frontmatter before setting `body`. Re-strip would drop a
    // leading Markdown HR (`---`) and skip link resolution for disk skills.
    if let Some(body) = skill.body.as_ref() {
        return Ok(body.clone());
    }
    // Synthetic product paths are never on disk and require a preloaded body.
    if skill.path.contains("://") {
        return Err(format!(
            "Skill '{}' has no preloaded body (path '{}')",
            skill.name, skill.path
        ));
    }
    let path = std::path::Path::new(&skill.path);
    match tokio::fs::read_to_string(path).await {
        Ok(content) => {
            let body = extract_skill_body(&content);
            Ok(match path.parent() {
                Some(skill_dir) => resolve_skill_internal_links(&body, skill_dir),
                None => body,
            })
        }
        Err(e) => Err(format!("Failed to read skill file '{}': {}", skill.path, e)),
    }
}

/// Load skill body into SkillInfo.
pub async fn load_skill_with_body(skill: &SkillInfo) -> Result<SkillInfo, String> {
    let path = std::path::Path::new(&skill.path);
    let content = tokio::fs::read_to_string(path)
        .await
        .map_err(|e| format!("Failed to read {}: {}", skill.path, e))?;
    let body = extract_skill_body(&content);
    let body = match path.parent() {
        Some(skill_dir) => resolve_skill_internal_links(&body, skill_dir),
        None => body,
    };
    let mut loaded = skill.clone();
    loaded.body = Some(body);
    Ok(loaded)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::implementations::skills::types::SkillScope;

    #[test]
    fn skill_envelope_escapes_attributes_and_preserves_body() {
        let name = "review\"<&";
        let args = "say \"hello\" <tag> & more";
        let body = "Raw <example> & **Markdown**";
        let block = build_skill_block(name, args, body);
        assert_eq!(
            block,
            "<skill name=\"review&quot;&lt;&amp;\" args=\"say &quot;hello&quot; &lt;tag&gt; &amp; more\">\nRaw <example> & **Markdown**\n</skill>"
        );
        assert_eq!(
            build_skill_block(name, "", body),
            "<skill name=\"review&quot;&lt;&amp;\">\nRaw <example> & **Markdown**\n</skill>"
        );
        let refs = [
            SkillRef {
                name,
                path: "/skills/\"<&/SKILL.md",
            },
            SkillRef {
                name,
                path: "/skills/\"<&/SKILL.md",
            },
        ];
        let info = build_skill_information(&[block.clone()], &refs);
        let reference =
            "<skill name=\"review&quot;&lt;&amp;\" path=\"/skills/&quot;&lt;&amp;/SKILL.md\"/>";
        assert_eq!(info.matches(reference).count(), 1, "{info}");
        assert!(info.contains(&block));
    }

    #[test]
    fn test_escape_xml() {
        assert_eq!(escape_xml("hello"), "hello");
        assert_eq!(escape_xml("<script>"), "&lt;script&gt;");
        assert_eq!(escape_xml("a & b"), "a &amp; b");
        assert_eq!(escape_xml("say \"hi\""), "say &quot;hi&quot;");
    }

    #[test]
    fn test_extract_skill_body() {
        let content = r#"---
name: test
description: A test skill
---
This is the skill body.

It has multiple lines."#;

        let body = extract_skill_body(content);
        assert_eq!(body, "This is the skill body.\n\nIt has multiple lines.");
    }

    #[test]
    fn frontmatter_requires_complete_delimiter_lines() {
        for content in [
            "---not-frontmatter\nname: test\n---\nKeep this body",
            "---\nname: test\n---not-a-close\nKeep this body",
        ] {
            assert_eq!(extract_skill_body(content), content);
            assert!(
                super::super::discovery::parse_skill_frontmatter(content, Some("test")).is_err()
            );
        }
        for content in [
            "---\nname: test\n---\nBody",
            "---\r\nname: test\r\n---\r\nBody",
        ] {
            assert_eq!(extract_skill_body(content), "Body");
            assert!(
                super::super::discovery::parse_skill_frontmatter(content, Some("test")).is_ok()
            );
        }
        assert_eq!(extract_skill_body("---\nname: test\n---"), "");
    }

    #[test]
    fn test_extract_skill_body_no_frontmatter() {
        let content = "Just some content without frontmatter";
        let body = extract_skill_body(content);
        assert_eq!(body, content);
    }

    #[tokio::test]
    async fn empty_skill_snapshot_survives_disk_changes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("SKILL.md");
        std::fs::write(&path, "---\nname: empty\n---\n").unwrap();
        let skill = SkillInfo {
            name: "empty".into(),
            path: path.to_string_lossy().into_owned(),
            ..Default::default()
        };
        let frozen = load_skill_with_body(&skill).await.unwrap();
        std::fs::write(&path, "Changed after freezing").unwrap();
        assert_eq!(load_skill_content(&frozen).await.unwrap(), "");
        assert_eq!(
            load_skill_content(&skill).await.unwrap(),
            "Changed after freezing"
        );
        std::fs::remove_file(&path).unwrap();
        assert_eq!(load_skill_content(&frozen).await.unwrap(), "");
        assert_eq!(frozen.body.as_deref(), Some(""));
    }

    #[tokio::test]
    async fn empty_preloaded_synthetic_body_is_authoritative() {
        let skill = SkillInfo {
            name: "empty".into(),
            path: "chat-product://empty".into(),
            body: Some(String::new()),
            ..Default::default()
        };
        assert_eq!(load_skill_content(&skill).await.unwrap(), "");
    }

    #[tokio::test]
    async fn load_skill_content_trusts_preloaded_body_with_leading_hr() {
        let skill = SkillInfo {
            name: "hr-body".to_string(),
            display_name: None,
            description: "test".to_string(),
            short_description: None,
            author: None,
            argument_hint: None,
            path: "chat-product://hr-body".to_string(),
            scope: SkillScope::User,
            config_source: None,
            plugin_name: None,
            plugin_version: None,
            plugin_root: None,
            plugin_data: None,
            allowed_tools: None,
            license: None,
            compatibility: None,
            metadata: None,

            user_invocable: true,
            disable_model_invocation: false,
            when_to_use: None,
            has_user_specified_description: true,
            paths: None,
            enabled: true,
            body: Some("---\n\nParagraph after a markdown HR.".to_string()),
        };
        let loaded = load_skill_content(&skill).await.unwrap();
        assert_eq!(loaded, "---\n\nParagraph after a markdown HR.");
    }

    #[tokio::test]
    async fn load_skill_content_rejects_synthetic_path_without_body() {
        let skill = SkillInfo {
            name: "pdf".to_string(),
            display_name: None,
            description: "test".to_string(),
            short_description: None,
            author: None,
            argument_hint: None,
            path: "chat-product://pdf".to_string(),
            scope: SkillScope::Server,
            config_source: None,
            plugin_name: None,
            plugin_version: None,
            plugin_root: None,
            plugin_data: None,
            allowed_tools: None,
            license: None,
            compatibility: None,
            metadata: None,

            user_invocable: true,
            disable_model_invocation: false,
            when_to_use: None,
            has_user_specified_description: true,
            paths: None,
            enabled: true,
            body: None,
        };
        let err = load_skill_content(&skill).await.unwrap_err();
        assert!(err.contains("no preloaded body"), "{err}");
        assert!(err.contains("chat-product://pdf"), "{err}");
    }

    #[test]
    fn test_format_skill_name() {
        let skill = SkillInfo {
            name: "commit".to_string(),
            display_name: None,
            description: "test".to_string(),
            short_description: None,
            author: None,
            argument_hint: None,
            path: "/path".to_string(),
            scope: SkillScope::User,
            config_source: None,
            plugin_name: None,
            plugin_version: None,
            plugin_root: None,
            plugin_data: None,
            allowed_tools: None,
            license: None,
            compatibility: None,
            metadata: None,

            user_invocable: true,
            disable_model_invocation: false,
            when_to_use: None,
            has_user_specified_description: false,
            paths: None,
            enabled: true,
            body: None,
        };
        assert_eq!(format_skill_name(&skill), "user:commit");

        let local_skill = SkillInfo {
            name: "build".to_string(),
            display_name: None,
            description: "test".to_string(),
            short_description: None,
            author: None,
            argument_hint: None,
            path: "/path".to_string(),
            scope: SkillScope::Local,
            config_source: None,
            plugin_name: None,
            plugin_version: None,
            plugin_root: None,
            plugin_data: None,
            allowed_tools: None,
            license: None,
            compatibility: None,
            metadata: None,

            user_invocable: true,
            disable_model_invocation: false,
            when_to_use: None,
            has_user_specified_description: false,
            paths: None,
            enabled: true,
            body: None,
        };
        assert_eq!(format_skill_name(&local_skill), "local:build");
    }

    #[test]
    fn test_format_skill_name_plugin() {
        let skill = SkillInfo {
            name: "deploy".to_string(),
            display_name: None,
            description: "Deploy to staging".to_string(),
            short_description: None,
            author: None,
            argument_hint: None,
            path: "/path".to_string(),
            scope: SkillScope::Plugin,
            config_source: None,
            plugin_name: Some("my-plugin".to_string()),
            plugin_version: None,
            plugin_root: None,
            plugin_data: None,
            allowed_tools: None,
            license: None,
            compatibility: None,
            metadata: None,

            user_invocable: true,
            disable_model_invocation: false,
            when_to_use: None,
            has_user_specified_description: false,
            paths: None,
            enabled: true,
            body: None,
        };
        // Plugin skills use plugin_name as prefix, not scope.
        assert_eq!(format_skill_name(&skill), "my-plugin:deploy");

        // Plugin skill without plugin_name falls back to scope.
        let mut no_name = skill.clone();
        no_name.plugin_name = None;
        assert_eq!(format_skill_name(&no_name), "plugin:deploy");
    }

    #[test]
    fn test_build_skill_message_exact_format() {
        let skill = SkillInfo {
            name: "commit".to_string(),
            display_name: None,
            description: "Create a git commit".to_string(),
            short_description: None,
            author: None,
            argument_hint: None,
            path: "/home/user/.grow/skills/commit/SKILL.md".to_string(),
            scope: SkillScope::User,
            config_source: None,
            plugin_name: None,
            plugin_version: None,
            plugin_root: None,
            plugin_data: None,
            allowed_tools: None,
            license: None,
            compatibility: None,
            metadata: None,

            user_invocable: true,
            disable_model_invocation: false,
            when_to_use: None,
            has_user_specified_description: false,
            paths: None,
            enabled: true,
            body: None,
        };

        let content = "# Git Commit Skill\n\nYou are helping the user create a commit.";
        let message = build_skill_message(&skill, content);

        // Assert the exact output so this breaks if any field or structural
        // detail changes (attribute order, newlines, tags).
        let expected = "\
<skill name=\"commit\" description=\"Create a git commit\" path=\"/home/user/.grow/skills/commit/SKILL.md\">
# Git Commit Skill

You are helping the user create a commit.
</skill>";
        assert_eq!(message, expected);
    }

    #[test]
    fn test_build_skill_message_special_chars_in_fields() {
        // Attribute values are escaped; the Markdown body stays verbatim.
        let skill = SkillInfo {
            name: "deploy-v2".to_string(),
            description: "Deploy \"staging\" & <prod>".to_string(),
            path: "/path/with spaces/SKILL.md".to_string(),
            ..SkillInfo::default()
        };

        let content = "Deploy instructions.";
        let message = build_skill_message(&skill, content);

        let expected = "\
<skill name=\"deploy-v2\" description=\"Deploy &quot;staging&quot; &amp; &lt;prod&gt;\" path=\"/path/with spaces/SKILL.md\">
Deploy instructions.
</skill>";
        assert_eq!(message, expected);
    }

    #[test]
    fn test_build_skill_message_empty_content() {
        let skill = SkillInfo {
            name: "empty".to_string(),
            description: "An empty skill".to_string(),
            path: "/skills/empty/SKILL.md".to_string(),
            ..SkillInfo::default()
        };

        let message = build_skill_message(&skill, "");

        let expected = "\
<skill name=\"empty\" description=\"An empty skill\" path=\"/skills/empty/SKILL.md\">

</skill>";
        assert_eq!(message, expected);
    }

    #[test]
    fn test_build_skill_message_multiline_content() {
        let skill = SkillInfo {
            name: "review".to_string(),
            description: "Review code".to_string(),
            path: "/repo/.grow/skills/review/SKILL.md".to_string(),
            ..SkillInfo::default()
        };

        let content = "# Code Review\n\nStep 1: Read the diff.\nStep 2: Check for bugs.\n\n## Checklist\n- Tests pass\n- No warnings";
        let message = build_skill_message(&skill, content);

        let expected = "\
<skill name=\"review\" description=\"Review code\" path=\"/repo/.grow/skills/review/SKILL.md\">
# Code Review

Step 1: Read the diff.
Step 2: Check for bugs.

## Checklist
- Tests pass
- No warnings
</skill>";
        assert_eq!(message, expected);
    }

    // ── apply_substitutions ─────────────────────────────────────────

    #[test]
    fn test_substitutions_arguments_full() {
        let mut content = "Run: $ARGUMENTS".to_string();
        apply_substitutions(
            &mut content,
            Some("fix typo"),
            &SubstitutionContext::default(),
        );
        assert_eq!(content, "Run: fix typo");
    }

    #[test]
    fn test_substitutions_indexed_arguments() {
        let mut content = "File: $ARGUMENTS[0], Action: $ARGUMENTS[1]".to_string();
        apply_substitutions(
            &mut content,
            Some("main.rs refactor"),
            &SubstitutionContext::default(),
        );
        assert_eq!(content, "File: main.rs, Action: refactor");
    }

    #[test]
    fn test_substitutions_shorthand_n() {
        let mut content = "Commit with message: $0".to_string();
        apply_substitutions(
            &mut content,
            Some("fix bug"),
            &SubstitutionContext::default(),
        );
        assert_eq!(content, "Commit with message: fix");
    }

    #[test]
    fn test_substitutions_skill_dir() {
        let mut content = "Config at ${SKILL_DIR}/config.json".to_string();
        apply_substitutions(
            &mut content,
            None,
            &SubstitutionContext {
                skill_dir: Some("/home/user/.grow/skills/deploy"),
                ..Default::default()
            },
        );
        assert_eq!(
            content,
            "Config at /home/user/.grow/skills/deploy/config.json"
        );
    }

    #[test]
    fn test_substitutions_missing_arg_index_becomes_empty() {
        let mut content = "Arg 0: $0, Arg 5: $5".to_string();
        apply_substitutions(
            &mut content,
            Some("only-one"),
            &SubstitutionContext::default(),
        );
        assert_eq!(content, "Arg 0: only-one, Arg 5: ");
    }

    #[test]
    fn test_no_substitutions_appends_suffix() {
        let mut content = "# Commit\n\nDo the commit.".to_string();
        apply_substitutions(
            &mut content,
            Some("fix bug"),
            &SubstitutionContext::default(),
        );
        assert_eq!(
            content,
            "# Commit\n\nDo the commit.\n\n**ARGUMENTS:** fix bug"
        );
    }

    #[test]
    fn test_no_substitutions_no_args_unchanged() {
        let mut content = "# Commit\n\nDo the commit.".to_string();
        apply_substitutions(&mut content, None, &SubstitutionContext::default());
        assert_eq!(content, "# Commit\n\nDo the commit.");
    }

    #[test]
    fn test_no_substitutions_empty_args_unchanged() {
        let mut content = "# Commit\n\nDo the commit.".to_string();
        apply_substitutions(&mut content, Some(""), &SubstitutionContext::default());
        assert_eq!(content, "# Commit\n\nDo the commit.");
    }

    #[test]
    fn test_session_id() {
        let mut content = "Session: ${SESSION_ID}".to_string();
        apply_substitutions(
            &mut content,
            None,
            &SubstitutionContext {
                session_id: Some("abc-123"),
                ..Default::default()
            },
        );
        assert_eq!(content, "Session: abc-123");
    }

    // ── plugin root/data substitution ───────────────────────────────

    #[test]
    fn test_plugin_root_and_data_substitution() {
        let mut content = "Root ${GROW_PLUGIN_ROOT}, data ${GROW_PLUGIN_DATA}".to_string();
        apply_substitutions(
            &mut content,
            None,
            &SubstitutionContext {
                plugin_root: Some("/plugins/vdc"),
                plugin_data: Some("/data/vdc"),
                ..Default::default()
            },
        );
        assert_eq!(content, "Root /plugins/vdc, data /data/vdc");
    }

    #[test]
    fn test_plugin_tokens_unchanged_when_root_none() {
        let mut content = "Root ${GROW_PLUGIN_ROOT}, data ${GROW_PLUGIN_DATA}".to_string();
        apply_substitutions(&mut content, None, &SubstitutionContext::default());
        assert_eq!(
            content,
            "Root ${GROW_PLUGIN_ROOT}, data ${GROW_PLUGIN_DATA}"
        );
    }

    #[test]
    fn test_plugin_token_with_args_appends_suffix() {
        // A path token expands, but with no argument token the args must still
        // be appended via the **ARGUMENTS:** suffix (not silently dropped).
        let mut content = "Run ${GROW_PLUGIN_ROOT}/tool.py".to_string();
        apply_substitutions(
            &mut content,
            Some("--flag"),
            &SubstitutionContext {
                plugin_root: Some("/plugins/vdc"),
                ..Default::default()
            },
        );
        assert_eq!(content, "Run /plugins/vdc/tool.py\n\n**ARGUMENTS:** --flag");
    }

    #[test]
    fn test_skill_dir_token_with_args_appends_suffix() {
        // Same rule for ${SKILL_DIR}: a metadata token expands, suffix still added.
        let mut content = "Config ${SKILL_DIR}/c.json".to_string();
        apply_substitutions(
            &mut content,
            Some("prod"),
            &SubstitutionContext {
                skill_dir: Some("/skills/deploy"),
                ..Default::default()
            },
        );
        assert_eq!(
            content,
            "Config /skills/deploy/c.json\n\n**ARGUMENTS:** prod"
        );
    }

    #[test]
    fn test_argument_token_with_path_token_no_suffix() {
        // When an argument token IS present, args expand inline and the path
        // token also expands; no **ARGUMENTS:** suffix is appended.
        let mut content = "Run ${GROW_PLUGIN_ROOT}/tool.py $ARGUMENTS".to_string();
        apply_substitutions(
            &mut content,
            Some("--flag"),
            &SubstitutionContext {
                plugin_root: Some("/plugins/vdc"),
                ..Default::default()
            },
        );
        assert_eq!(content, "Run /plugins/vdc/tool.py --flag");
    }

    // ── >9 indexed arguments ────────────────────────────────────────

    #[test]
    fn test_more_than_10_indexed_args() {
        let mut content = "Arg 12: $12".to_string();
        let many_args = "a b c d e f g h i j k l the-twelfth";
        apply_substitutions(
            &mut content,
            Some(many_args),
            &SubstitutionContext::default(),
        );
        assert_eq!(content, "Arg 12: the-twelfth");
    }

    // ── Mixed variables ─────────────────────────────────────────────

    #[test]
    fn test_mixed_arguments_and_skill_dir() {
        let mut content = "Deploy $0 from ${SKILL_DIR}. Full: $ARGUMENTS".to_string();
        apply_substitutions(
            &mut content,
            Some("staging --force"),
            &SubstitutionContext {
                skill_dir: Some("/skills/deploy"),
                ..Default::default()
            },
        );
        assert_eq!(
            content,
            "Deploy staging from /skills/deploy. Full: staging --force"
        );
    }

    #[test]
    fn substitutions_preserve_inserted_token_text() {
        let ctx = SubstitutionContext {
            skill_dir: Some("/dir/${SESSION_ID}"),
            session_id: Some("real-session"),
            ..Default::default()
        };
        for (template, args, expected) in [
            ("$ARGUMENTS", Some("${SESSION_ID}"), "${SESSION_ID}"),
            ("$ARGUMENTS[1]", Some("first $0"), "$0"),
            ("${SKILL_DIR}", None, "/dir/${SESSION_ID}"),
            ("$ARGUMENTS[1000000]", Some("first"), ""),
        ] {
            let mut content = template.to_owned();
            apply_substitutions(&mut content, args, &ctx);
            assert_eq!(content, expected, "template: {template}");
        }
    }

    #[test]
    fn test_unknown_dollar_tokens_left_unchanged() {
        let mut content = "Price: $100, var: ${UNKNOWN}".to_string();
        apply_substitutions(&mut content, None, &SubstitutionContext::default());
        assert_eq!(content, "Price: $100, var: ${UNKNOWN}");
    }

    #[test]
    fn test_dollar_amount_does_not_suppress_suffix() {
        // $100 should NOT trigger substitution mode — args should still
        // be appended as **ARGUMENTS:** suffix.
        let mut content = "Price: $100 per unit.".to_string();
        apply_substitutions(
            &mut content,
            Some("deploy staging"),
            &SubstitutionContext::default(),
        );
        assert_eq!(
            content,
            "Price: $100 per unit.\n\n**ARGUMENTS:** deploy staging"
        );
    }

    #[test]
    fn test_real_substitution_suppresses_suffix() {
        // When $ARGUMENTS is present, args are expanded inline
        // and **ARGUMENTS:** suffix is NOT appended.
        let mut content = "Run: $ARGUMENTS (cost: $100)".to_string();
        apply_substitutions(
            &mut content,
            Some("deploy"),
            &SubstitutionContext::default(),
        );
        assert_eq!(content, "Run: deploy (cost: $100)");
    }

    // ── extract_skill_display_text ──────────────────────────────────

    #[test]
    fn extract_skill_with_args() {
        let input = "<command-name>implement</command-name>\n\
                      <command-message>/implement</command-message>\n\
                      <command-args>fix the rendering bug</command-args>";
        assert_eq!(
            extract_skill_display_text(input),
            Some("/implement fix the rendering bug".to_string()),
        );
    }

    #[test]
    fn extract_skill_without_args() {
        let input = "<command-name>deploy</command-name>\n\
                      <command-message>/deploy</command-message>";
        assert_eq!(
            extract_skill_display_text(input),
            Some("/deploy".to_string()),
        );
    }

    #[test]
    fn extract_skill_empty_args() {
        let input = "<command-name>commit</command-name>\n\
                      <command-message>/commit</command-message>\n\
                      <command-args>  </command-args>";
        assert_eq!(
            extract_skill_display_text(input),
            Some("/commit".to_string()),
        );
    }

    #[test]
    fn extract_skill_qualified_name() {
        let input = "<command-name>local:compact</command-name>\n\
                      <command-message>/local:compact</command-message>";
        assert_eq!(
            extract_skill_display_text(input),
            Some("/local:compact".to_string()),
        );
    }

    #[test]
    fn extract_skill_returns_none_for_plain_text() {
        assert_eq!(extract_skill_display_text("just a normal message"), None);
    }

    #[test]
    fn extract_skill_returns_none_for_empty_text() {
        assert_eq!(extract_skill_display_text(""), None);
    }

    #[test]
    fn extract_skill_partial_xml_no_command_message() {
        let input = "<command-name>deploy</command-name>";
        assert_eq!(extract_skill_display_text(input), Some("/deploy".into()));
    }

    #[test]
    fn extract_skill_partial_xml_unclosed_command_message() {
        let input = "<command-name>deploy</command-name>\n\
                      <command-message>/deploy";
        // <command-message> present but unclosed — falls back to <command-name>.
        assert_eq!(extract_skill_display_text(input), Some("/deploy".into()));
    }

    #[test]
    fn extract_skill_name_only_with_args() {
        let input = "<command-name>review</command-name>\n\
                      <command-args>198653</command-args>";
        assert_eq!(
            extract_skill_display_text(input),
            Some("/review 198653".into())
        );
    }

    #[test]
    fn extract_skill_truncated_args_no_close_tag() {
        // Generated display titles can truncate mid-args, losing </command-args>.
        let input = "<command-name>implement</command-name> \
                      <command-message>/implement</command-message> \
                      <command-args>there are still 2 issues * the reverse";
        assert_eq!(
            extract_skill_display_text(input),
            Some("/implement there are still 2 issues * the reverse".into())
        );
    }

    #[test]
    fn extract_skill_empty_command_message_falls_back_to_name() {
        let input = "<command-name>deploy</command-name>\n\
                      <command-message></command-message>";
        assert_eq!(extract_skill_display_text(input), Some("/deploy".into()));
    }

    #[test]
    fn extract_skill_empty_command_name_returns_none() {
        let input = "<command-name></command-name>";
        assert_eq!(extract_skill_display_text(input), None);
    }

    // ── build_skill_block ───────────────────────────────────────────

    #[test]
    fn test_build_skill_block_with_args() {
        let block = build_skill_block("commit", "fix typo", "# Commit\n\nMake a commit.");
        let expected = "\
<skill name=\"commit\" args=\"fix typo\">
# Commit

Make a commit.
</skill>";
        assert_eq!(block, expected);
    }

    #[test]
    fn test_build_skill_block_without_args() {
        let block = build_skill_block("review", "", "# Review\n\nReview code.");
        let expected = "\
<skill name=\"review\">
# Review

Review code.
</skill>";
        assert_eq!(block, expected);
    }

    // ── build_skill_information ─────────────────────────────────────

    #[test]
    fn test_build_skill_information_single() {
        let blocks = vec![build_skill_block("commit", "fix typo", "Body here.")];
        let refs = vec![SkillRef {
            name: "commit",
            path: "/home/user/.grow/skills/commit/SKILL.md",
        }];
        let result = build_skill_information(&blocks, &refs);
        assert!(result.starts_with("<skill_information>\n"));
        assert!(result.ends_with("\n</skill_information>"));
        assert!(result.contains("<skills_referenced>\n"));
        assert!(
            result.contains(
                "<skill name=\"commit\" path=\"/home/user/.grow/skills/commit/SKILL.md\"/>"
            )
        );
        assert!(result.contains("<skill name=\"commit\" args=\"fix typo\">"));
    }

    #[test]
    fn test_build_skill_information_multi() {
        let blocks = vec![
            build_skill_block("review", "fix auth", "Review body."),
            build_skill_block("lint", "--strict", "Lint body."),
        ];
        let refs = vec![
            SkillRef {
                name: "review",
                path: "/project/.grow/skills/review/SKILL.md",
            },
            SkillRef {
                name: "lint",
                path: "/project/.grow/skills/lint/SKILL.md",
            },
        ];
        let result = build_skill_information(&blocks, &refs);
        assert!(result.starts_with("<skill_information>\n"));
        assert!(result.contains("<skills_referenced>\n"));
        assert!(result.contains("<skill name=\"review\" path="));
        assert!(result.contains("<skill name=\"lint\" path="));
        assert!(result.contains("<skill name=\"review\" args=\"fix auth\">"));
        assert!(result.contains("<skill name=\"lint\" args=\"--strict\">"));
    }

    #[test]
    fn test_build_skill_information_deduplicates_refs() {
        let blocks = vec![
            build_skill_block("art", "a", "Body A."),
            build_skill_block("art", "b", "Body B."),
        ];
        let refs = vec![
            SkillRef {
                name: "art",
                path: "/skills/art/SKILL.md",
            },
            SkillRef {
                name: "art",
                path: "/skills/art/SKILL.md",
            },
        ];
        let result = build_skill_information(&blocks, &refs);
        // The path should appear only once in <skills_referenced>.
        assert_eq!(
            result
                .matches("<skill name=\"art\" path=\"/skills/art/SKILL.md\"/>")
                .count(),
            1,
            "duplicate refs should be deduplicated: {result}"
        );
        // Both <skill> content blocks still present.
        assert_eq!(result.matches("<skill name=\"art\" args=").count(), 2);
    }

    #[test]
    fn test_build_skill_information_empty() {
        let result = build_skill_information(&[], &[]);
        assert_eq!(result, "");
    }

    // ── resolve_skill_internal_links ────────────────────────────────

    #[test]
    fn internal_link_title_matching_destination_stays_literal() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        std::fs::write(root.join("guide.md"), "guide").unwrap();
        for (body, expected) in [
            (
                "[guide](guide.md \"guide.md\")".to_owned(),
                format!("[guide]({}/guide.md \"guide.md\")", root.display()),
            ),
            (
                "[guide][ref]\n\n[ref]: guide.md \"guide.md\"".to_owned(),
                format!(
                    "[guide][ref]\n\n[ref]: {}/guide.md \"guide.md\"",
                    root.display()
                ),
            ),
        ] {
            assert_eq!(resolve_skill_internal_links(&body, &root), expected);
        }
    }

    #[test]
    fn internal_link_resolution_preserves_labels_code_and_nested_images() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        std::fs::write(root.join("guide.md"), "guide").unwrap();
        let target = root.join("guide.md").display().to_string();
        for (body, expected) in [
            (
                "[guide.md](guide.md \"guide.md\")".to_owned(),
                format!("[guide.md]({target} \"guide.md\")"),
            ),
            (
                "[`fake](guide.md)` label](guide.md)".to_owned(),
                format!("[`fake](guide.md)` label]({target})"),
            ),
            ("[](guide.md)".to_owned(), format!("[]({target})")),
            (
                "[![image](guide.md)](guide.md)".to_owned(),
                format!("[![image]({target})]({target})"),
            ),
            (
                "[guide.md]\n\n[guide.md]: <guide.md> \"guide.md\"".to_owned(),
                format!("[guide.md]\n\n[guide.md]: <{target}> \"guide.md\""),
            ),
            (
                "`[guide](guide.md)`".to_owned(),
                "`[guide](guide.md)`".to_owned(),
            ),
        ] {
            assert_eq!(
                resolve_skill_internal_links(&body, &root),
                expected,
                "{body}"
            );
        }
    }

    #[test]
    fn internal_link_resolution_keeps_escaped_destinations_parseable() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap().join("skill root");
        std::fs::create_dir(&root).unwrap();
        for (file, body) in [
            ("guide(1).md", r"[guide](guide\(1\).md)"),
            ("a&b.md", "[guide](a&amp;b.md)"),
            ("with space.md", "[guide](<with space.md>)"),
        ] {
            std::fs::write(root.join(file), "guide").unwrap();
            let resolved = resolve_skill_internal_links(body, &root);
            let destinations: Vec<_> = pulldown_cmark::Parser::new(&resolved)
                .filter_map(|event| match event {
                    pulldown_cmark::Event::Start(pulldown_cmark::Tag::Link {
                        dest_url, ..
                    }) => Some(dest_url.into_string()),
                    _ => None,
                })
                .collect();
            assert_eq!(
                destinations,
                vec![root.join(file).display().to_string()],
                "{resolved}"
            );
        }
    }

    #[test]
    fn resolve_internal_links_blocks_path_traversal() {
        let tmp = tempfile::tempdir().unwrap();
        let skill_dir = tmp.path().join("skills").join("my-skill");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(tmp.path().join("secret.md"), "secret").unwrap();

        let body = "See [escape](../../secret.md) here.";
        let result = resolve_skill_internal_links(body, &skill_dir);
        assert_eq!(result, body);
    }
}
