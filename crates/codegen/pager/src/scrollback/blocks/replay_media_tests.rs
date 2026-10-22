use super::{AgentMessageBlock, ThinkingBlock};
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
        appearance: crate::appearance::AppearanceConfig::default(),
        is_selected: false,
        cwd: None,
    }
}

fn output_signature(
    output: &BlockOutput,
) -> Vec<(String, String, Option<u16>, Option<String>, Option<String>)> {
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

fn image_snapshot(block: &AgentMessageBlock) -> Vec<(PathBuf, Option<(u32, u32)>, String)> {
    block
        .image_references()
        .iter()
        .map(|image| (image.path.clone(), image.dimensions, image.alt_text.clone()))
        .collect()
}

fn png_bytes(width: u32, height: u32) -> Vec<u8> {
    let image = image::RgbImage::from_pixel(width, height, image::Rgb([20, 80, 140]));
    let mut bytes = Vec::new();
    image
        .write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )
        .expect("encode PNG fixture");
    bytes
}

fn jpeg_bytes(width: u32, height: u32) -> Vec<u8> {
    let image = image::RgbImage::from_pixel(width, height, image::Rgb([140, 80, 20]));
    let mut bytes = Vec::new();
    image
        .write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Jpeg,
        )
        .expect("encode JPEG fixture");
    bytes
}

fn compare_eager_and_deferred(
    text: &str,
    chunks: &[&str],
) -> (AgentMessageBlock, AgentMessageBlock) {
    let eager = AgentMessageBlock::new(text);
    let mut deferred = AgentMessageBlock::streaming();
    for chunk in chunks {
        deferred.push_chunk_deferred(chunk);
    }
    deferred.finish();
    (eager, deferred)
}

#[test]
fn replay_finish_captures_image_snapshot_and_preserves_mixed_markdown() {
    let directory = tempfile::tempdir().expect("temporary image directory");
    let png = directory.path().join("small.png");
    let jpeg = directory.path().join("small.jpg");
    std::fs::write(&png, png_bytes(3, 2)).expect("write PNG fixture");
    std::fs::write(&jpeg, jpeg_bytes(5, 4)).expect("write JPEG fixture");
    let text = format!(
        "Before **bold** text.\n\n![png alt]({})\n\n![jpeg alt]({})\n\nAfter *italic* text.",
        png.display(),
        jpeg.display(),
    );
    let midpoint = text.len() / 2;
    let (mut eager, mut deferred) =
        compare_eager_and_deferred(&text, &[&text[..midpoint], &text[midpoint..]]);
    let expected = vec![
        (png.clone(), Some((3, 2)), "png alt".to_owned()),
        (jpeg.clone(), Some((5, 4)), "jpeg alt".to_owned()),
    ];
    assert_eq!(image_snapshot(&eager), expected);
    assert_eq!(image_snapshot(&deferred), expected);
    assert_eq!(deferred.text(), text);
    // The first output after replay finish must use the verified snapshot even
    // if the referenced files disappear or become undecodable.
    std::fs::remove_file(&png).expect("remove captured PNG");
    std::fs::write(&jpeg, b"no longer a JPEG").expect("corrupt captured JPEG");
    assert_eq!(image_snapshot(&deferred), expected);
    assert_eq!(deferred.copy_text(true), eager.copy_text(true));
    assert_eq!(deferred.copy_text(false), eager.copy_text(false));
    assert!(deferred.copy_text(false).contains("Before bold text."));
    assert!(deferred.copy_text(false).contains("After italic text."));

    for width in [32, 64, 80, 100, 120] {
        for raw in [false, true] {
            deferred.set_raw_mode(raw);
            let output = deferred.output(&context(width, raw));
            eager.set_raw_mode(raw);
            assert_eq!(
                output_signature(&output),
                output_signature(&eager.output(&context(width, raw))),
                "render differs at width={width}, raw={raw}",
            );
            assert_eq!(image_snapshot(&deferred), expected);
        }
    }
}

#[test]
fn missing_and_corrupt_image_references_do_not_consume_markdown_text() {
    let directory = tempfile::tempdir().expect("temporary image directory");
    let missing = directory.path().join("missing.png");
    let corrupt = directory.path().join("corrupt.jpg");
    std::fs::write(&corrupt, b"not a JPEG").expect("write corrupt image");
    let text = format!(
        "Keep this lead. ![missing]({})\n\n![broken]({})\n\nKeep this ending.",
        missing.display(),
        corrupt.display(),
    );
    let split = text.find("![broken]").expect("image marker") + 5;
    let (eager, deferred) = compare_eager_and_deferred(&text, &[&text[..split], &text[split..]]);
    assert!(eager.image_references().is_empty());
    assert!(deferred.image_references().is_empty());
    assert_eq!(deferred.text(), text);
    assert_eq!(deferred.copy_text(true), eager.copy_text(true));
    assert!(deferred.copy_text(true).contains("![missing]"));
    assert!(deferred.copy_text(true).contains("![broken]"));
    assert!(deferred.copy_text(false).contains("Keep this lead."));
    assert!(deferred.copy_text(false).contains("Keep this ending."));
    assert_eq!(
        output_signature(&deferred.output(&context(48, false))),
        output_signature(&eager.output(&context(48, false))),
    );
}

#[test]
fn replay_mermaid_sources_affordances_and_rows_match_eager_at_each_width() {
    use crate::appearance::{RenderMermaid, cache};

    let previous = cache::load_render_mermaid();
    cache::set_render_mermaid(RenderMermaid::On);
    let text = "intro\n\n> ```Mermaid theme=base\n> flowchart TD\n>   A-->B\n> ```\n\n- nested item\n  ```mermaid\n  graph LR\n  X-->Y\n  ```\n\n```MERMAID\nsequenceDiagram\nA->>B: hi\n```\n";
    let chunks = [
        "intro\n\n> ```Mer",
        "maid theme=base\n> flowchart TD\n",
        ">   A-->B\n> ```\n\n- nested item\n  ```mermaid\n",
        "  graph LR\n  X-->Y\n  ```\n\n```MERMAID\nsequenceDiagram\nA->>B: hi\n```\n",
    ];
    let (mut eager, mut deferred) = compare_eager_and_deferred(text, &chunks);
    let sources = (0..3)
        .map(|index| {
            deferred
                .mermaid()
                .source(index)
                .expect("diagram source")
                .to_owned()
        })
        .collect::<Vec<_>>();
    assert_eq!(eager.mermaid().len(), 3);
    assert_eq!(deferred.mermaid().len(), 3);
    assert_eq!(
        sources,
        (0..3)
            .map(|i| eager.mermaid().source(i).unwrap().to_owned())
            .collect::<Vec<_>>()
    );
    assert!(sources[0].contains("A-->B"));
    assert!(sources[1].contains("X-->Y"));
    assert!(sources[2].contains("A->>B: hi"));

    for width in [24, 48, 80, 88, 120] {
        for raw in [false, true] {
            eager.set_raw_mode(raw);
            deferred.set_raw_mode(raw);
            let ctx = context(width, raw);
            assert_eq!(
                output_signature(&deferred.output(&ctx)),
                output_signature(&eager.output(&ctx)),
                "output differs at width={width}, raw={raw}",
            );
            let eager_affordances = eager.diagram_affordances(&ctx);
            let deferred_affordances = deferred.diagram_affordances(&ctx);
            if raw {
                assert!(eager_affordances.is_empty());
                assert!(deferred_affordances.is_empty());
            } else {
                assert_eq!(deferred_affordances, eager_affordances);
                assert_eq!(deferred_affordances.len(), 3);
                assert!(
                    deferred_affordances
                        .windows(2)
                        .all(|pair| pair[0].row_offset < pair[1].row_offset)
                );
                assert_eq!(
                    deferred_affordances
                        .iter()
                        .map(|aff| aff.source.clone())
                        .collect::<Vec<_>>(),
                    sources,
                );
            }
        }
    }
    cache::set_render_mermaid(previous);
}

#[test]
fn replay_internal_image_projection_placeholders_stay_hidden_until_late_suffix() {
    for placeholder in shell::sampling::conversation::IMAGE_PROJECTION_PLACEHOLDERS {
        let mut complete = AgentMessageBlock::streaming();
        let split = placeholder.len() / 2;
        complete.push_chunk_deferred(&placeholder[..split]);
        complete.push_chunk_deferred(&placeholder[split..]);
        complete.finish();
        assert!(complete.is_empty());
        assert_eq!(complete.copy_text(true), "");
        assert!(complete.output(&context(80, false)).lines.is_empty());

        let mut partial = AgentMessageBlock::streaming();
        partial.push_chunk_deferred(&placeholder[..placeholder.len() - 1]);
        partial.finish();
        assert_eq!(partial.text(), &placeholder[..placeholder.len() - 1]);
        assert_eq!(partial.copy_text(true), partial.text());
        assert!(!partial.output(&context(80, false)).lines.is_empty());

        complete.push_chunk_deferred(" explanation");
        complete.finish();
        assert_eq!(complete.text(), format!("{placeholder} explanation"));
        assert_eq!(complete.copy_text(true), complete.text());
        assert!(!complete.output(&context(80, false)).lines.is_empty());
    }
}

#[test]
fn replay_thinking_finish_does_not_invent_local_elapsed_time() {
    let mut replay = ThinkingBlock::streaming_replay();
    replay.push_chunk_deferred("replayed reasoning");
    replay.finish();
    assert_eq!(replay.elapsed_time_ms(), None);
}
