use super::AgentMessageBlock;
use crate::appearance::AppearanceConfig;
use crate::scrollback::block::BlockContent;
use crate::scrollback::types::{BlockContext, BlockOutput, DisplayMode};
use std::path::PathBuf;

fn context(width: u16, raw: bool) -> BlockContext {
    BlockContext {
        mode: DisplayMode::Expanded,
        is_running: false,
        width,
        raw,
        max_lines: None,
        appearance: AppearanceConfig::default(),
        is_selected: false,
        cwd: None,
    }
}

type OutputSignature = (String, String, Option<u16>, Option<String>, Option<String>);

fn output_signature(output: &BlockOutput) -> Vec<OutputSignature> {
    output
        .lines
        .iter()
        .map(|line| {
            (
                line.content
                    .spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect(),
                format!("{:?}", line.selectable),
                line.selection_range,
                line.selection_text.clone(),
                line.joiner.clone(),
            )
        })
        .collect()
}

fn png_bytes(width: u32, height: u32) -> Vec<u8> {
    let image = image::RgbImage::from_pixel(width, height, image::Rgb([26, 88, 144]));
    let mut bytes = Vec::new();
    image
        .write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )
        .expect("encode PNG fixture");
    bytes
}

fn repeat_to_bytes(seed: &str, target_bytes: usize) -> String {
    let repeats = target_bytes.div_ceil(seed.len());
    seed.repeat(repeats)
}

#[test]
fn relative_markdown_image_reference_is_resolved_from_current_directory() {
    let cwd = std::env::current_dir().expect("current directory");
    let directory = tempfile::Builder::new()
        .prefix("grow-source-scan-")
        .tempdir_in(&cwd)
        .expect("temporary fixture under current directory");
    let image_path = directory.path().join("relative.png");
    std::fs::write(&image_path, png_bytes(7, 5)).expect("write PNG fixture");
    let relative_path = image_path
        .strip_prefix(&cwd)
        .expect("fixture is below current directory");
    let text = format!("![relative image]({})", relative_path.display());

    let block = AgentMessageBlock::new(text);
    assert_eq!(block.image_references().len(), 1);
    assert_eq!(
        block.image_references()[0].path,
        PathBuf::from(relative_path)
    );
    assert_eq!(block.image_references()[0].alt_text, "relative image");
    assert_eq!(block.image_references()[0].dimensions, Some((7, 5)));
}

#[test]
fn mermaid_candidate_detection_keeps_case_and_entity_encoded_fences() {
    let valid = [
        "```mermaid\ngraph TD\nA-->B\n```",
        "```Mermaid theme=base\ngraph TD\nA-->B\n```",
        "```MERMAID\ngraph TD\nA-->B\n```",
        "```m&#101;rmaid\ngraph TD\nA-->B\n```",
        "```m&#x65;rmaid\ngraph TD\nA-->B\n```",
    ];
    for source in valid {
        let eager = AgentMessageBlock::new(source);
        assert_eq!(eager.mermaid().len(), 1, "source: {source}");

        let mut replay = AgentMessageBlock::streaming();
        replay.push_chunk_deferred(&source[..source.len() / 2]);
        replay.push_chunk_deferred(&source[source.len() / 2..]);
        replay.finish();
        assert_eq!(replay.mermaid().len(), 1, "replayed source: {source}");
        assert_eq!(
            replay.mermaid().source(0),
            eager.mermaid().source(0),
            "replayed source: {source}"
        );
    }
}

#[test]
fn mermaid_candidate_detection_rejects_prose_and_fence_lookalikes() {
    for source in [
        "The word mermaid appears in ordinary prose.",
        "```mermaidx\ngraph TD\nA-->B\n```",
        "",
    ] {
        let block = AgentMessageBlock::new(source);
        assert!(block.mermaid().is_empty(), "false candidate: {source}");
        let mut replay = AgentMessageBlock::streaming();
        replay.push_chunk_deferred(source);
        replay.finish();
        assert!(
            replay.mermaid().is_empty(),
            "deferred false candidate: {source}"
        );
        assert_eq!(replay.copy_text(false), block.copy_text(false));
    }
}

#[test]
fn copy_first_and_render_first_match_after_late_append_at_wide_columns() {
    let prefix = "# Replay 🌱\n\nA [link](https://example.test) and **bold** text.\n";
    let suffix = "\nLate append: 雪 and `code`, followed by a long line that wraps differently at eighty and one hundred twenty columns.";
    let expected = AgentMessageBlock::new(format!("{prefix}{suffix}"));

    for width in [80, 120] {
        for copy_first in [true, false] {
            let mut replay = AgentMessageBlock::streaming();
            replay.push_chunk_deferred(prefix);
            replay.finish();

            if copy_first {
                let prefix_block = AgentMessageBlock::new(prefix);
                assert_eq!(replay.copy_text(true), prefix_block.copy_text(true));
                assert_eq!(replay.copy_text(false), prefix_block.copy_text(false));
            } else {
                let prefix_block = AgentMessageBlock::new(prefix);
                assert_eq!(
                    output_signature(&replay.output(&context(width, false))),
                    output_signature(&prefix_block.output(&context(width, false))),
                    "prefix render at width={width}"
                );
            }

            replay.push_chunk_deferred(suffix);
            replay.finish();
            assert_eq!(replay.copy_text(true), expected.copy_text(true));
            assert_eq!(replay.copy_text(false), expected.copy_text(false));
            assert_eq!(
                output_signature(&replay.output(&context(width, false))),
                output_signature(&expected.output(&context(width, false))),
                "final render at width={width}, copy_first={copy_first}"
            );
        }
    }
}

#[test]
fn empty_message_keeps_empty_source_and_copy() {
    let mut replay = AgentMessageBlock::streaming();
    replay.finish();
    let expected = AgentMessageBlock::new("");

    assert_eq!(replay.text(), expected.text());
    assert_eq!(replay.copy_text(true), expected.copy_text(true));
    assert_eq!(replay.copy_text(false), expected.copy_text(false));
    assert!(replay.image_references().is_empty());
    assert!(replay.mermaid().is_empty());
}

#[test]
fn unicode_false_image_markers_keep_source_and_copy_text() {
    let source = "雪 ! [ not an image ] /\n\nmermaid is plain text 🌿";
    let expected = AgentMessageBlock::new(source);
    let mut replay = AgentMessageBlock::streaming();
    replay.push_chunk_deferred(source);
    replay.finish();

    assert_eq!(replay.text(), expected.text());
    assert_eq!(replay.copy_text(true), expected.copy_text(true));
    assert_eq!(replay.copy_text(false), expected.copy_text(false));
    assert!(replay.image_references().is_empty());
    assert!(replay.mermaid().is_empty());
}

#[test]
fn replay_window_resets_attribution_without_advancing_reconnect_cursor() {
    let mut agent = crate::test_util::make_agent_view(Some("source-scan"), "/tmp");
    agent.session.last_seen_event_id = Some("retained-cursor".into());
    agent.session.replay_first_applied = Some("previous-history".into());
    agent.session.replay_first_frame = true;
    agent.begin_replay_window();
    assert!(agent.session.replay_first_applied.is_none());
    assert!(!agent.session.replay_first_frame);
    agent.note_history_applied(None);
    assert!(agent.session.replay_first_applied.is_none());
    assert_eq!(
        agent.session.last_seen_event_id.as_deref(),
        Some("retained-cursor")
    );
}

#[test]
#[ignore = "manual source scan microbenchmark; set SOURCE_SCAN_BENCH_ITERATIONS to adjust repetitions"]
fn source_scan_deferred_finish_microbenchmark() {
    let iterations = std::env::var("SOURCE_SCAN_BENCH_ITERATIONS")
        .map(|value| {
            value
                .parse::<usize>()
                .expect("SOURCE_SCAN_BENCH_ITERATIONS must be a positive integer")
        })
        .unwrap_or(512);
    assert!(iterations > 0, "benchmark requires at least one iteration");

    let plain = repeat_to_bytes(
        "Plain Unicode text with no media markers. 雪🌱\n",
        32 * 1024,
    );
    let slash_code = repeat_to_bytes(
        "/// src/render/path.rs\\module\\item ```rust fn f() { let p = \"a/b\"; } ```\n",
        32 * 1024,
    );
    let unicode_false_markers = repeat_to_bytes(
        "Unicode 雪🌱 with separated ! and [ markers, mermaidx fence hints, and prose.\n",
        32 * 1024,
    );

    let cwd = std::env::current_dir().expect("current directory");
    let image_directory = tempfile::Builder::new()
        .prefix("grow-source-scan-bench-")
        .tempdir_in(&cwd)
        .expect("temporary benchmark fixture under current directory");
    let image_path = image_directory.path().join("repeat.png");
    std::fs::write(&image_path, png_bytes(8, 6)).expect("write benchmark PNG once");
    let image_markdown = format!("![benchmark]({})\n", image_path.display());
    let repeated_image_markdown = image_markdown.repeat(512);

    let cases = [
        ("plain_32k", plain),
        ("slash_code_dense", slash_code),
        ("image_markdown_repeated", repeated_image_markdown),
        ("unicode_false_markers", unicode_false_markers),
    ];

    for (case, source) in cases {
        let expected = AgentMessageBlock::new(source.clone());
        let expected_images = expected.image_references().len();
        let expected_mermaid = expected.mermaid().len();
        let mut elapsed = std::time::Duration::ZERO;
        let mut retained = 0usize;

        for _ in 0..iterations {
            let mut block = AgentMessageBlock::streaming();
            let started = std::time::Instant::now();
            block.push_chunk_deferred(&source);
            block.finish();
            elapsed += started.elapsed();

            assert_eq!(block.image_references().len(), expected_images, "{case}");
            assert_eq!(block.mermaid().len(), expected_mermaid, "{case}");
            retained ^= std::hint::black_box(block.text().len());
            std::hint::black_box(block.copy_text(true));
        }

        std::hint::black_box(retained);
        eprintln!(
            "[source-scan] case={case} source_bytes={} messages={iterations} elapsed_ms={:.3}",
            source.len(),
            elapsed.as_secs_f64() * 1000.0,
        );
    }
}
