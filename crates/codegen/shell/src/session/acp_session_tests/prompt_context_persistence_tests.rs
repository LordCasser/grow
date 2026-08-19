use super::support::create_test_actor;
use super::*;

// ── Large-prompt truncation: maybe_truncate_large_prompt_with_skills ────
//
// Oversized prompts are offloaded to an owner-only file; the bounding logic is
// the pure `build_truncated_prompt_message` helper, tested directly below.

// Distinctive markers so head/middle/tail are individually assertable.
const HEAD_TOKEN: &str = "HEADSTART_TOKEN_aaa";
const TAIL_TOKEN: &str = "TAILEND_TOKEN_zzz";

fn fake_prompt_path() -> std::path::PathBuf {
    std::path::PathBuf::from("/tmp/grow-test-home/sessions/cwd/sid/prompts/0123456789abcdef.txt")
}
fn fake_prompt_ref() -> &'static str {
    "artifact:prompt:blake3:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
}

/// `truncate_bytes_suffix` keeps a char-boundary-safe suffix (multibyte-safe).
#[test]
fn truncate_bytes_suffix_is_utf8_safe() {
    assert_eq!(truncate_bytes_suffix("hello", 5), "hello");
    assert_eq!(truncate_bytes_suffix("hello world", 5), "world");
    // "a🎉🎉b" = 10 bytes; asking for 6 lands mid-codepoint → advances to boundary.
    let s = "a🎉🎉b";
    let out = truncate_bytes_suffix(s, 6);
    assert!(out.len() <= 6);
    assert!(s.ends_with(out));
    assert!(std::str::from_utf8(out.as_bytes()).is_ok());
}

/// `bound_head_tail`: input when it fits, else head+marker+tail within budget.
#[test]
fn bound_head_tail_boundary_and_utf8() {
    // At budget → unchanged (`<=`).
    let fits = "a".repeat(100);
    assert_eq!(bound_head_tail(&fits, 100), fits);
    // One over → bounded.
    let over = "a".repeat(101);
    let out = bound_head_tail(&over, 100);
    assert!(
        out.len() <= 100,
        "bounded output ({}) exceeds budget",
        out.len()
    );
    assert!(out.contains(ELISION_MARKER));
    // Multibyte: no panic, within budget.
    let mb = "🎉".repeat(5_000); // 20_000 bytes
    let out_mb = bound_head_tail(&mb, 8_000);
    assert!(out_mb.len() <= 8_000);
    assert!(out_mb.starts_with('🎉'));
    assert!(out_mb.ends_with('🎉'));
}

/// (a) Oversized query: the bounded message keeps a HEAD and a TAIL (trailing
/// question survives), elides the middle; full body never inlined.
#[test]
fn build_truncated_keeps_query_head_and_tail() {
    let middle = "M".repeat(LARGE_PROMPT_THRESHOLD * 3);
    let query = format!("{HEAD_TOKEN} {middle} {TAIL_TOKEN} what does this say?");
    let full =
        crate::session::prompt_parser::ParsedPrompt::assemble_parts_with_skills("", &query, "");

    let message = build_truncated_prompt_message("", &query, "", fake_prompt_ref(), full.len());

    assert!(message.contains(HEAD_TOKEN), "head must survive inline");
    assert!(message.contains(TAIL_TOKEN), "tail must survive inline");
    assert!(
        message.contains("what does this say?"),
        "trailing question must survive inline"
    );
    let head_idx = message.find(HEAD_TOKEN).expect("head present");
    let tail_idx = message.find(TAIL_TOKEN).expect("tail present");
    assert!(
        head_idx < tail_idx,
        "head must appear before tail in the bounded inline message"
    );
    assert!(
        !message.contains(&middle),
        "middle bulk must not be inlined"
    );
    assert!(
        message.contains(ELISION_MARKER),
        "elision marker must mark the cut"
    );
    assert!(
        !message.contains(&query),
        "full query body must not be inlined"
    );
    assert!(message.contains(OFFLOAD_NOTICE_MARKER));
    assert!(message.contains(fake_prompt_ref()));
    assert!(
        message.len() <= TRUNCATED_PROMPT_PREFIX_SIZE,
        "message ({}) must stay within budget",
        message.len()
    );
}

/// (b) Large context + small query: query intact, context truncated.
#[test]
fn build_truncated_preserves_small_query_truncates_context() {
    let context = format!("CTXHEAD_TOKEN {}", "C".repeat(LARGE_PROMPT_THRESHOLD * 3));
    let query = "please summarise the attached file".to_string();
    let full = crate::session::prompt_parser::ParsedPrompt::assemble_parts_with_skills(
        &context, &query, "",
    );

    let message =
        build_truncated_prompt_message(&context, &query, "", fake_prompt_ref(), full.len());

    assert!(message.contains(&query), "small query preserved intact");
    assert!(
        message.starts_with(&query),
        "grow ordering: query block first"
    );
    assert!(message.contains("CTXHEAD_TOKEN"), "context head preserved");
    assert!(!message.contains(&context), "oversized context truncated");
    assert!(message.len() <= TRUNCATED_PROMPT_PREFIX_SIZE);
}

/// Both query and context oversized (the 80/20 split arm): both bounded, neither full body inlined.
#[test]
fn build_truncated_both_oversized_keeps_bounded_heads() {
    let query = format!(
        "QHEAD_TOKEN {} QTAIL_TOKEN",
        "Q".repeat(LARGE_PROMPT_THRESHOLD * 2)
    );
    let context = format!("CHEAD_TOKEN {}", "C".repeat(LARGE_PROMPT_THRESHOLD * 2));
    let full = crate::session::prompt_parser::ParsedPrompt::assemble_parts_with_skills(
        &context, &query, "",
    );

    let message =
        build_truncated_prompt_message(&context, &query, "", fake_prompt_ref(), full.len());

    assert!(
        message.contains("QHEAD_TOKEN"),
        "bounded query head present"
    );
    assert!(
        message.contains("QTAIL_TOKEN"),
        "bounded query tail present"
    );
    assert!(
        message.contains("CHEAD_TOKEN"),
        "bounded context head present"
    );
    assert!(!message.contains(&query), "full query not inlined");
    assert!(!message.contains(&context), "full context not inlined");
    assert!(
        message.starts_with("QHEAD_TOKEN"),
        "grow ordering: query first"
    );
    assert!(
        message.len() <= TRUNCATED_PROMPT_PREFIX_SIZE,
        "message ({}) must stay within budget",
        message.len()
    );
}

/// Skills survive inline even when the query is oversized (own reservation).
#[test]
fn build_truncated_preserves_skill_information() {
    let query = "Q".repeat(LARGE_PROMPT_THRESHOLD * 3);
    let skills = "SKILL_MARKER: follow the xyz skill steps".to_string();
    let full = crate::session::prompt_parser::ParsedPrompt::assemble_parts_with_skills(
        "", &query, &skills,
    );

    let message =
        build_truncated_prompt_message("", &query, &skills, fake_prompt_ref(), full.len());

    assert!(
        message.contains("SKILL_MARKER"),
        "invoked-skill text must survive inline even with an oversized query"
    );
    assert!(
        !message.contains(&query),
        "full query body must not be inlined"
    );
    assert!(message.len() <= TRUNCATED_PROMPT_PREFIX_SIZE);
}

/// A skill over `SKILL_INLINE_BUDGET` is bounded head+tail; full body not inlined.
#[test]
fn build_truncated_bounds_oversized_skill_head_and_tail() {
    let query = "short query".to_string();
    // Skill well over the 4 KB budget, with distinct head/tail markers.
    let skills = format!(
        "SKILLHEAD_TOKEN {} SKILLTAIL_TOKEN",
        "S".repeat(SKILL_INLINE_BUDGET * 2)
    );
    let full = crate::session::prompt_parser::ParsedPrompt::assemble_parts_with_skills(
        "", &query, &skills,
    );

    let message =
        build_truncated_prompt_message("", &query, &skills, fake_prompt_ref(), full.len());

    assert!(
        message.contains("SKILLHEAD_TOKEN"),
        "skill head must survive inline"
    );
    assert!(
        message.contains("SKILLTAIL_TOKEN"),
        "skill tail (closing framing) must survive inline"
    );
    assert!(
        !message.contains(&skills),
        "full skill body must not be inlined"
    );
    assert!(
        message.contains(ELISION_MARKER),
        "oversized skill must be marked as elided"
    );
    assert!(message.contains(&query), "small query stays intact");
    assert!(message.len() <= TRUNCATED_PROMPT_PREFIX_SIZE);
}

/// Multibyte query + context: bounding must not panic, stays within budget.
#[test]
fn build_truncated_multibyte_no_panic() {
    let query = "路".repeat(LARGE_PROMPT_THRESHOLD); // 3 bytes each → oversized
    let context = "🎉".repeat(LARGE_PROMPT_THRESHOLD); // 4 bytes each → oversized
    let full = crate::session::prompt_parser::ParsedPrompt::assemble_parts_with_skills(
        &context, &query, "",
    );

    let message =
        build_truncated_prompt_message(&context, &query, "", fake_prompt_ref(), full.len());

    assert!(message.len() <= TRUNCATED_PROMPT_PREFIX_SIZE);
    assert!(message.contains(OFFLOAD_NOTICE_MARKER));
}

/// The persisted notice reports bytes, the marker, the logical ref, and `read_file`.
#[test]
fn build_offload_notice_reports_bytes_marker_and_ref() {
    let notice = build_offload_notice(123_456, fake_prompt_ref());
    assert!(notice.contains(OFFLOAD_NOTICE_MARKER));
    assert!(notice.contains("123456 bytes"));
    assert!(notice.contains(fake_prompt_ref()));
    assert!(notice.contains("read_file"));
}

// ── Method gate + call-site wiring (hermetic) ───────────────────────────
//
// The actor owns an explicit entity directory, so both the threshold gate and
// the real offload path can be exercised without process-global path state.

/// Threshold gate: a prompt exactly at `LARGE_PROMPT_THRESHOLD` is returned unchanged, no file.
#[tokio::test(flavor = "current_thread")]
async fn maybe_truncate_at_threshold_returns_unchanged_no_file() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let (gateway_tx, _) =
                tokio::sync::mpsc::unbounded_channel::<acp_transport::AcpClientMessage>();
            let (persistence_tx, _) = tokio::sync::mpsc::unbounded_channel::<PersistenceMsg>();
            let actor = create_test_actor(0, 1_000_000, 85, gateway_tx, persistence_tx).await;

            // Empty context ⇒ full_message == query.
            let at = "Q".repeat(LARGE_PROMPT_THRESHOLD);
            let expected = crate::session::prompt_parser::ParsedPrompt::assemble_parts_with_skills(
                "", &at, "",
            );
            let message = actor
                .maybe_truncate_large_prompt_with_skills(String::new(), at, String::new())
                .await;
            assert_eq!(message, expected, "at-threshold prompt returned unchanged");
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn oversized_prompt_blob_is_owned_by_the_explicit_entity_directory() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let temp = tempfile::tempdir().unwrap();
            let (gateway_tx, _) =
                tokio::sync::mpsc::unbounded_channel::<acp_transport::AcpClientMessage>();
            let (persistence_tx, _) = tokio::sync::mpsc::unbounded_channel::<PersistenceMsg>();
            let mut actor = create_test_actor(0, 1_000_000, 85, gateway_tx, persistence_tx).await;
            actor.session_dir = temp.path().join("parent/subagents/child");
            let query = "Q".repeat(LARGE_PROMPT_THRESHOLD + 1);

            let full = crate::session::prompt_parser::ParsedPrompt::assemble_parts_with_skills(
                "", &query, "",
            );
            let path = crate::session::persistence::get_prompt_blob_path(&actor.session_dir, &full);
            let message = actor
                .maybe_truncate_large_prompt_with_skills(String::new(), query, String::new())
                .await;
            assert!(path.starts_with(actor.session_dir.join("prompts")));
            assert!(path.is_file());
            assert!(message.contains(crate::session::persistence::PROMPT_BLOB_REF_PREFIX));
            assert!(!message.contains(&path.to_string_lossy().to_string()));
        })
        .await;
}

/// Call-site wiring (injected-writer seam): success keeps the logical notice;
/// write failure rewrites it and never returns the oversized original.
#[test]
fn write_offload_and_build_wires_offload_and_fallback() {
    let temp = tempfile::tempdir().unwrap();
    let file_path = temp
        .path()
        .join("sid")
        .join("prompts")
        .join("0123456789abcdef.txt");
    std::fs::create_dir_all(file_path.parent().unwrap()).unwrap();

    let query = format!(
        "HEAD_TOKEN {} TAIL_TOKEN",
        "Q".repeat(LARGE_PROMPT_THRESHOLD * 3)
    );
    let full =
        crate::session::prompt_parser::ParsedPrompt::assemble_parts_with_skills("", &query, "");
    let bounded = build_truncated_prompt_message("", &query, "", fake_prompt_ref(), full.len());

    // Success path: real secure writer.
    let message = write_offload_and_build(
        &full,
        bounded.clone(),
        file_path.clone(),
        fake_prompt_ref(),
        crate::util::secure_file::write_secure_file,
    );
    assert_eq!(
        std::fs::read_to_string(&file_path).unwrap(),
        full,
        "file holds the full message bytes"
    );
    assert_eq!(message, bounded, "success returns the bounded message");
    assert!(!message.contains(&query), "full query body not inlined");
    assert!(
        message.contains(OFFLOAD_NOTICE_MARKER),
        "success keeps the file-referencing offload notice"
    );
    assert!(
        message.contains(fake_prompt_ref()),
        "persisted message retains the logical artifact identity"
    );

    // Failure path: erroring writer → bounded excerpt (NOT the oversized
    // original), no path, AND the file-referencing notice stripped so the model
    // is never told to read a file that was never written.
    let fallback_msg = write_offload_and_build(
        &full,
        bounded.clone(),
        file_path.clone(),
        fake_prompt_ref(),
        |_p, _b| Err(std::io::Error::other("simulated disk full")),
    );
    assert_ne!(
        fallback_msg, bounded,
        "write failure must rewrite the notice, not return it verbatim"
    );
    assert!(
        !fallback_msg.contains(OFFLOAD_NOTICE_MARKER),
        "write failure must strip the file-referencing offload notice"
    );
    assert!(
        !fallback_msg.contains(fake_prompt_ref()),
        "write failure must not point the model at a file that was never written"
    );
    assert!(
        fallback_msg.contains("could not be saved"),
        "write failure must explain the excerpt is all there is"
    );
    assert!(
        fallback_msg.contains("HEAD_TOKEN") && fallback_msg.contains("TAIL_TOKEN"),
        "the bounded head+tail excerpt must survive the failure path"
    );
    assert!(
        fallback_msg.len() <= TRUNCATED_PROMPT_PREFIX_SIZE,
        "fallback must stay within budget (no re-overflow)"
    );
    assert!(
        !fallback_msg.contains(&query),
        "fallback must not inline the full query"
    );
}

#[test]
fn prompt_blob_is_immutable_and_idempotent() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("prompt.txt");
    write_immutable_blob(&path, b"canonical prompt").unwrap();
    write_immutable_blob(&path, b"canonical prompt").unwrap();
    let error = write_immutable_blob(&path, b"different bytes").unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
    assert_eq!(std::fs::read(&path).unwrap(), b"canonical prompt");
}

#[cfg(unix)]
#[test]
fn prompt_blob_read_and_write_reject_symlink_targets() {
    use std::os::unix::fs::symlink;

    let dir = tempfile::tempdir().unwrap();
    let content = b"canonical prompt";
    let hash = blake3::hash(content).to_hex().to_string();
    let target = dir.path().join("outside.txt");
    let prompts = dir.path().join("prompts");
    let link = prompts.join(format!("{hash}.txt"));
    std::fs::create_dir(&prompts).unwrap();
    std::fs::write(&target, content).unwrap();
    symlink(&target, &link).unwrap();

    let write_error = write_immutable_blob(&link, content)
        .expect_err("immutable writes must not accept an existing symlink");
    assert_eq!(write_error.kind(), std::io::ErrorKind::InvalidData);
    let read_error = crate::session::persistence::verified_prompt_blob_bytes(dir.path(), &hash)
        .expect_err("immutable reads must not follow a symlink");
    assert_eq!(read_error.kind(), std::io::ErrorKind::InvalidData);
    assert_eq!(std::fs::read(&target).unwrap(), content);
}

#[test]
fn prompt_blob_reference_is_host_independent_until_request_projection() {
    let dir = tempfile::tempdir().unwrap();
    let content = "canonical oversized prompt";
    let hash = blake3::hash(content.as_bytes()).to_hex().to_string();
    let path = dir.path().join("prompts").join(format!("{hash}.txt"));
    write_immutable_blob(&path, content.as_bytes()).unwrap();
    let reference = format!(
        "{}{hash}",
        crate::session::persistence::PROMPT_BLOB_REF_PREFIX
    );
    let original = ConversationItem::user(format!("read\n{reference}\nthen continue"));
    let mut request_items = vec![original.clone()];

    assert_eq!(
        crate::session::persistence::materialize_prompt_blob_refs(&mut request_items, dir.path(),)
            .unwrap(),
        1
    );
    assert!(original.text_content().contains(&reference));
    assert!(
        request_items[0]
            .text_content()
            .contains(&path.to_string_lossy().to_string())
    );
    assert!(!request_items[0].text_content().contains(&reference));
}

/// `strip_offload_notice` swaps the exact file-referencing notice for the no-file
/// failure notice, and is a no-op when the notice is absent (defensive).
#[test]
fn strip_offload_notice_swaps_notice_for_no_file_text() {
    let notice = build_offload_notice(45_177, fake_prompt_ref());
    let message = format!("bounded excerpt body{notice}");
    let stripped = strip_offload_notice(&message, &notice);
    assert!(
        stripped.starts_with("bounded excerpt body"),
        "excerpt preserved"
    );
    assert!(
        !stripped.contains(OFFLOAD_NOTICE_MARKER),
        "file-referencing marker removed"
    );
    assert!(
        !stripped.contains(fake_prompt_ref()),
        "artifact reference removed"
    );
    assert!(
        stripped.contains("could not be saved"),
        "no-file failure notice substituted"
    );
    // Absent notice → message returned unchanged.
    assert_eq!(
        strip_offload_notice("plain message", &notice),
        "plain message"
    );
}
