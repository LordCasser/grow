// Diagnostic only: exercise the current library without contacting a provider.
use sampling_types::{
    ConversationItem, ConversationRequest, NativeContinuationProjection, ToolCall,
    build_messages_request,
    messages::{ContentBlock, MessageContent},
};

fn inspect(prefix: usize) -> (usize, usize, usize) {
    let mut request = ConversationRequest::from_items(vec![
        ConversationItem::user("continue the authorized task"),
        ConversationItem::assistant_tool_calls(vec![ToolCall {
            id: "call_probe".into(),
            name: "read_file".into(),
            arguments: r#"{"path":"fixture.txt"}"#.into(),
        }]),
        ConversationItem::tool_result("call_probe", "fixture contents"),
    ]);
    request.native_continuation = Some(NativeContinuationProjection {
        portable_prefix_len: prefix,
        spans: Vec::new(),
    });
    let mut counts = (0, 0, 0);
    for message in build_messages_request(&request).messages {
        if let MessageContent::Blocks(blocks) = message.content {
            for block in blocks {
                match block {
                    ContentBlock::ToolUse { .. } => counts.0 += 1,
                    ContentBlock::ToolResult { .. } => counts.1 += 1,
                    ContentBlock::Text { text, .. }
                        if text.starts_with("[Historical tool exchange;") =>
                    {
                        counts.2 += 1;
                    }
                    _ => {}
                }
            }
        }
    }
    println!("prefix={prefix}: tool_use={}, tool_result={}, historical_exchange={}",
        counts.0, counts.1, counts.2);
    counts
}

fn main() {
    // Reset immediately after response admission, before the result is appended.
    assert_eq!(inspect(2), (0, 1, 0));
    // Controls: the same complete exchange entirely after/before the boundary.
    assert_eq!(inspect(1), (1, 1, 0));
    assert_eq!(inspect(3), (0, 0, 1));
}
