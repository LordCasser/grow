//! Source-ready milestone for native TODO control of one real background child.
//! Run only with an explicit, current GROW_BINARY. Providers and state are test-owned.
//! Typed control handling must precede normal Result delivery and explicit parent review.

use std::path::{Path, PathBuf};
use std::time::Duration;

use acp_transport::protocol as acp;
use chat_state::{
    NotificationEvent, NotificationSource, NotificationSourceVersion, SubagentEvent,
    SubagentOutcome, SubagentSpawnEvent, Timeline, TimelineEventKind, TimelineRangeRef, ToolEvent,
};
use sampling_types::{AgentMessage, ConversationItem};
use serde_json::{Value, json};
use shell::extensions::notification::{AgentMessageNotice, SessionNotification, SessionUpdate};
use shell::session::storage::{self, SessionUpdate as HistoryUpdate};
use test_support::mock_server::LogEntry;
use test_support::*;
use tool_types::planning::{PlanReadInput, PlanReadOutput};
use tool_types::{
    LocalPlan, PlanStepStatus, ReplanChange, ReplanOutcome, ReplanProgress, ReviewDecision,
    SubagentStatus, SubagentWaitReason, TaskToolInput, WaitSubagentInput, WaitSubagentOutput,
};
use tools::implementations::grow_build::task::interaction::{
    AgentInteractionOutput, AskParentInput,
};
use tools::implementations::grow_build::todo::{TodoReadTool, TodoWriteInput};
use tools::registry::types::generate_schema;
use tools::types::tool_metadata::ToolMetadata;

const STEP: &str = "delegated-control-step";
const INVESTIGATION: &str = "valid-investigation";
const AFFECTED: &str = "affected-verification";
const OBJECTIVE: &str = "Inspect the isolated delegation and preserve completed investigation.";
const EDITED_OBJECTIVE: &str = "Inspect the isolated delegation and verify its edited acceptance.";
const EDITED_REQUIREMENTS: &[&str] = &[
    "Preserve completed investigation.",
    "Verify the changed acceptance: \"精确来源\".\nDo not restart valid work.",
];
const TASK_PROMPT: &str = "Exercise only the scripted native planning and parent-question tools in this isolated fixture. Preserve valid completed work and report a normal final result after necessary settlement.";
const QUESTION: &str = "Is the isolated acceptance limited to the delegated TODO, preserving the completed investigation? fixture-question-control-32";
const AUX_REPLY: &str = "Yes. The isolated acceptance covers the delegated TODO; preserve valid completed investigation.";
const REPORT: &str = "Completed: read the real parent plan, clarified its acceptance, and preserved completed investigation.\nSettlement: accepted the exact removal, stopped advancing the original objective, cancelled only the affected local verification, and closed this isolated work normally.\nRemaining: the removed objective's verification was not performed.\nArtifacts: no workspace files were changed.\nUncertainty: no external command or descendant was started; no external result is claimed.";
const PARENT_REPORT: &str =
    "Accepted the isolated child's exact canonical result after its normal settlement report.";
const GUIDANCE: &str = "Current planning context (summary; guidance, not permission).";
const REPLAN_PREFIX: &str = "The parent TODO changed. Use the revised objective and requirements below to adjust affected work. Preserve valid completed work. Read the parent TODO or ask for clarification only if you need a missing detail to decide what to do. Record handling once in todo_write with the handle_replans action and this change_id. If your work already covers the update, supply a short no_change_reason in that action. Keep routine confirmations out of replies to the user or parent.";
const STOP_PREFIX: &str = "The parent TODO was removed or cancelled. This delegation is terminated. Stop advancing its objective now, preserve existing work, perform only necessary cleanup and settlement, then report completed work, remaining work, artifacts, and unresolved outcomes to the parent. Do not resume this delegation because the TODO is later reopened.";

fn tool_response(call_id: &str, name: &str, arguments: Value) -> ScriptedResponse {
    // The barrier must precede ALL response content, including finish_reason.
    // A reasoning chunk followed by a terminal chunk is not a safe capture here.
    ScriptedResponse::sse(vec![SseEvent::data(
        json!({
            "id":call_id,"object":"chat.completion.chunk","created":0,"model":"fixture-wire",
            "choices":[{"index":0,"delta":{"tool_calls":[{
                "index":0,"id":call_id,"type":"function",
                "function":{"name":name,"arguments":arguments.to_string()}
            }]},"finish_reason":"tool_calls"}],
            "usage":{"prompt_tokens":10,"completion_tokens":5,"total_tokens":15}
        })
        .to_string(),
    )])
}

fn completion_response(text: &str) -> ScriptedResponse {
    ScriptedResponse::sse(vec![SseEvent::data(json!({
        "id":"fixture-completion","object":"chat.completion.chunk","created":0,"model":"fixture-wire",
        "choices":[{"index":0,"delta":{"role":"assistant","content":text},"finish_reason":"stop"}],
        "usage":{"prompt_tokens":10,"completion_tokens":5,"total_tokens":15}
    }).to_string())])
}

fn expect_tool(
    server: &MockInferenceServer,
    id: &str,
    name: &str,
    args: Value,
) -> InferenceExpectation {
    server.expect_response(
        id,
        InferenceRequestMatcher::foreground(InferenceEndpoint::ChatCompletions),
        tool_response(id, name, args),
    )
}

fn hold_tool(
    server: &MockInferenceServer,
    id: &str,
    name: &str,
    args: Value,
) -> InferenceExpectation {
    server.expect_response_blocked(
        id,
        InferenceRequestMatcher::foreground(InferenceEndpoint::ChatCompletions),
        tool_response(id, name, args),
    )
}

fn is_foreground(entry: &LogEntry) -> bool {
    if entry.method != "POST" || entry.path != "/v1/chat/completions" {
        return false;
    }
    // Mirror the fixture's documented classification, including req-id precedence.
    // Matchers cannot select a model or session; the endpoints must be independent.
    if entry
        .header("x-grow-turn-idx")
        .is_some_and(|value| !value.is_empty())
    {
        return true;
    }
    if entry
        .header("x-grow-req-id")
        .is_some_and(|value| !value.is_empty())
    {
        return false;
    }
    entry.body.as_ref().is_some_and(|body| {
        body["tools"]
            .as_array()
            .is_some_and(|tools| tools.len() >= 2)
    })
}

fn foreground(server: &MockInferenceServer) -> Vec<LogEntry> {
    server
        .requests()
        .into_iter()
        .filter(is_foreground)
        .collect()
}

async fn blocked(
    expectation: &mut InferenceExpectation,
    server: &MockInferenceServer,
    client: &GrowStdioClient,
) -> Value {
    tokio::time::timeout(scaled(Duration::from_secs(30)), expectation.wait_blocked())
        .await
        .unwrap_or_else(|_| {
            panic!(
                "{}\n{}\n{}",
                expectation.diagnostic(),
                server.request_log_summary(),
                client.stderr()
            )
        });
    assert!(
        !expectation.is_satisfied(),
        "capture before any scripted terminal content"
    );
    foreground(server).last().unwrap().body.clone().unwrap()
}

fn tool_output<'a>(body: &'a Value, call_id: &str) -> &'a str {
    let outputs: Vec<_> = body["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|message| message["role"] == "tool" && message["tool_call_id"] == call_id)
        .collect();
    assert_eq!(
        outputs.len(),
        1,
        "one actual native tool response for {call_id}"
    );
    outputs[0]["content"]
        .as_str()
        .expect("native tool text output")
}

struct Journal {
    path: PathBuf,
    timeline: Timeline,
    presentation: Vec<Value>,
}

impl Journal {
    fn plan(&self) -> &LocalPlan {
        &self
            .timeline
            .planning_snapshot()
            .expect("native planning committed a plan")
            .plan
    }
}

fn journal(grow_home: &Path, session_id: &str) -> Journal {
    uuid::Uuid::parse_str(session_id).expect("only canonical session IDs created by this test");
    let paths: Vec<_> = std::fs::read_dir(grow_home.join("sessions"))
        .unwrap()
        .map(|entry| {
            entry
                .unwrap()
                .path()
                .join(session_id)
                .join("timeline.jsonl")
        })
        .filter(|path| path.is_file())
        .collect();
    assert_eq!(
        paths.len(),
        1,
        "one canonical path in the private GROW_HOME"
    );
    let bytes = std::fs::read_to_string(&paths[0]).unwrap();
    assert!(
        bytes.ends_with('\n'),
        "capture a complete physical journal prefix"
    );
    let mut events = Vec::new();
    let mut presentation = Vec::new();
    for (seq, line) in bytes.lines().enumerate() {
        let record: Value = serde_json::from_str(line).unwrap();
        assert_eq!(record["version"], 1);
        assert_eq!(record["seq"], seq as u64);
        match record["fact"]["kind"].as_str() {
            Some("domain") => {
                events.push(serde_json::from_value(record["fact"]["event"].clone()).unwrap())
            }
            Some("presentation") => presentation.push(record["fact"].clone()),
            kind => panic!("unexpected physical fact {kind:?}"),
        }
    }
    Journal {
        path: paths.into_iter().next().unwrap(),
        timeline: Timeline::from_events(events).expect("all real domain events pass the full fold"),
        presentation,
    }
}

fn spawn(parent: &Journal) -> (u64, &SubagentSpawnEvent) {
    let spawns: Vec<_> = parent
        .timeline
        .events()
        .iter()
        .filter_map(|event| match &event.kind {
            TimelineEventKind::Subagent(SubagentEvent::Spawned(spawn)) => {
                Some((event.seq.get(), spawn.as_ref()))
            }
            _ => None,
        })
        .collect();
    assert_eq!(spawns.len(), 1);
    assert_eq!(
        spawns[0].1.origin_tool_call_id.as_deref(),
        Some("call_spawn")
    );
    spawns[0]
}

fn planning_at(journal: &Journal, seq: u64) -> &LocalPlan {
    let event = journal
        .timeline
        .events()
        .iter()
        .find(|event| event.seq.get() == seq)
        .expect("exact source seq exists");
    let TimelineEventKind::Planning(planning) = &event.kind else {
        panic!("source must name a Planning event: {event:?}")
    };
    &planning.plan
}

fn schema(body: &Value, name: &str, mut expected: Value) -> Value {
    // Registry finalization interpolates documented limits and adds the real
    // blocking-wait maximum; the raw Rust input schema has neither yet.
    tools::types::context::TruncationConfig::default().apply_to_schema(
        &mut expected,
        name,
        tools::DEFAULT_TOOL_OUTPUT_BYTES,
        tool_types::max_wait_block_ms(),
    );
    let definitions: Vec<_> = body["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|tool| tool["function"]["name"] == name)
        .collect();
    assert_eq!(
        definitions.len(),
        1,
        "native tool is callable in this actual request: {name}"
    );
    let definition = &definitions[0]["function"];
    assert_eq!(
        definition["parameters"], expected,
        "actual native schema for {name}"
    );
    definition.clone()
}

fn runtime(body: &Value) -> Value {
    let mut blocks = Vec::new();
    for message in body["messages"].as_array().unwrap() {
        let texts: Vec<_> = match &message["content"] {
            Value::String(text) => vec![text.as_str()],
            Value::Array(parts) => parts
                .iter()
                .filter_map(|part| part["text"].as_str())
                .collect(),
            _ => Vec::new(),
        };
        for text in texts {
            let Some(start) = text.find(GUIDANCE) else {
                continue;
            };
            assert_eq!(message["role"], "user");
            assert_eq!(text.matches(GUIDANCE).count(), 1);
            let context = &text[start..];
            let (guidance, payload) = context
                .split_once('\n')
                .expect("runtime GUIDANCE then JSON");
            let mut stream = serde_json::Deserializer::from_str(payload).into_iter::<Value>();
            let value = stream.next().unwrap().unwrap();
            let end = stream.byte_offset();
            assert!(matches!(payload[end..].trim(), "" | "</system-reminder>"));
            assert!(
                guidance.len() + 1 + end <= 8192,
                "runtime's final escaped UTF-8 budget"
            );
            assert_eq!(value["summary"], true);
            blocks.push(value);
        }
    }
    assert_eq!(blocks.len(), 1, "one fresh request-only planning context");
    blocks.pop().unwrap()
}

fn assert_context(
    body: &Value,
    parent: &Journal,
    spawn: &SubagentSpawnEvent,
    objective: &str,
    removed: bool,
) -> Value {
    let context = runtime(body);
    assert_eq!(body["model"], "child-wire");
    assert_eq!(context["child_session_id"], spawn.child_session_id);
    assert_eq!(context["read_entry"]["availability"], "available");
    let read = context["read_entry"]["tool"].as_str().unwrap();
    assert_eq!(
        read, "todo_read",
        "this fixture uses the default native aliases"
    );
    let definition = schema(body, read, generate_schema::<PlanReadInput>());
    assert_eq!(
        definition["description"],
        TodoReadTool.description_template()
    );
    assert_eq!(context["parent"]["observation"], "available");
    assert_eq!(
        context["parent"]["binding"],
        json!({
            "parent_session_id":parent.plan().session_id,"task_id":spawn.subagent_id,"parent_step_id":STEP
        })
    );
    let source = &context["parent"]["source"];
    let plan = planning_at(parent, source["timeline_seq"].as_u64().unwrap());
    assert_eq!(
        source,
        &json!({"session_id":plan.session_id,"scope_id":plan.scope_id,"revision":plan.revision,"timeline_seq":source["timeline_seq"]})
    );
    assert_eq!(plan.session_id, spawn.security_parent_session_id);
    let (index, step) = plan
        .steps
        .iter()
        .enumerate()
        .find(|(_, step)| step.id == STEP)
        .unwrap();
    assert_eq!(step.content, objective);
    assert_eq!(step.removed, removed);
    assert_eq!(context["parent"]["associated_step"]["id"], step.id);
    assert_eq!(context["parent"]["associated_step"]["position"], index + 1);
    assert_eq!(context["parent"]["associated_step"]["removed"], removed);
    assert_eq!(
        context["parent"]["associated_step"]["status"],
        serde_json::to_value(step.status).unwrap()
    );
    assert_eq!(
        context["parent"]["associated_step"]["priority"],
        serde_json::to_value(step.priority).unwrap()
    );
    let requirements = step.requirements.as_deref().unwrap_or_default();
    assert_eq!(
        context["parent"]["associated_step"]["requirements_summary"],
        json!({
            "count":requirements.len(),"omitted_count":0,
            "items":requirements.iter().map(|text| json!({"text":text,"omitted_bytes":0})).collect::<Vec<_>>()
        })
    );
    assert_eq!(
        context["parent"]["associated_step"]["content_summary"],
        json!({"text":objective,"omitted_bytes":0})
    );
    assert_eq!(context["parent"]["counts"]["steps"], plan.steps.len());
    let link = plan
        .links
        .iter()
        .find(|link| link.id == spawn.subagent_id)
        .unwrap();
    assert_eq!(link.parent_step_id, STEP);
    assert_eq!(link.child_session_id, spawn.child_session_id);
    assert!(link.required && !link.terminal && link.result_id.is_none());
    context
}

fn read_output(
    body: &Value,
    call_id: &str,
    source: &Journal,
    target: &str,
    delegated: bool,
) -> PlanReadOutput {
    let output: PlanReadOutput =
        serde_json::from_str(tool_output(body, call_id)).expect("actual native todo_read output");
    assert_eq!(serde_json::to_value(output.target).unwrap(), target);
    let page = output.plan.as_ref().expect("the actual owner has a plan");
    let plan = planning_at(source, page.timeline_seq);
    assert_eq!(
        (&page.session_id, &page.scope_id, page.revision),
        (&plan.session_id, &plan.scope_id, plan.revision)
    );
    assert!(
        page.next_cursor.is_none(),
        "this small fixture fits in the native 50-item page"
    );
    assert_eq!(page.steps, plan.steps);
    assert_eq!(page.links, plan.links);
    assert_eq!(page.replans, plan.replans);
    assert_eq!(page.waits, plan.waits);
    assert_eq!(page.reviews, plan.reviews);
    assert_eq!(page.counts, page.matched_counts);
    assert_eq!(page.counts.steps as usize, plan.steps.len());
    assert_eq!(page.counts.links as usize, plan.links.len());
    assert_eq!(page.counts.replans as usize, plan.replans.len());
    assert_eq!(page.counts.waits as usize, plan.waits.len());
    assert_eq!(page.counts.reviews as usize, plan.reviews.len());
    assert_eq!(page.delegation.is_some(), delegated);
    if let Some(binding) = &page.delegation {
        let link = plan
            .links
            .iter()
            .find(|link| link.id == binding.task_id)
            .unwrap();
        assert_eq!(binding.parent_session_id, plan.session_id);
        assert_eq!(binding.parent_scope_id, plan.scope_id);
        assert_eq!(binding.parent_step_id, link.parent_step_id);
        assert_eq!(binding.child_session_id, link.child_session_id);
    }
    output
}

fn same_change(expected: &ReplanChange, actual: &ReplanChange) {
    let mut actual = actual.clone();
    actual.progress = expected.progress.clone();
    assert_eq!(
        &actual, expected,
        "control identity and payload stay immutable"
    );
}

fn change_seq(journal: &Journal, change: &ReplanChange, progress: &ReplanProgress) -> u64 {
    journal
        .timeline
        .events()
        .iter()
        .find_map(|event| match &event.kind {
            TimelineEventKind::Planning(planning) => planning
                .plan
                .replans
                .iter()
                .find(|record| record.id == change.id && &record.progress == progress)
                .map(|record| {
                    same_change(change, record);
                    event.seq.get()
                }),
            _ => None,
        })
        .unwrap_or_else(|| panic!("missing exact durable stage {progress:?} for {}", change.id))
}

struct ControlEvidence {
    message: AgentMessage,
    source: NotificationSource,
    version: NotificationSourceVersion,
    received_seq: u64,
    consumed_seq: u64,
}

fn control_evidence(child: &Journal, parent_id: &str, change: &ReplanChange) -> ControlEvidence {
    let consumed: Vec<_> = child
        .timeline
        .events()
        .iter()
        .filter_map(|event| match &event.kind {
            TimelineEventKind::Notification(NotificationEvent::Consumed {
                turn,
                input: Some(ConversationItem::AgentMessage(batch)),
                ..
            }) => Some((event.seq.get(), *turn, batch)),
            _ => None,
        })
        .flat_map(|(seq, turn, batch)| {
            batch.messages.iter().filter_map(move |message| {
                let (_, details) = message.message.split_once('\n')?;
                let details: Value = serde_json::from_str(details).ok()?;
                (details["change_id"] == change.id).then_some((seq, turn, message))
            })
        })
        .collect();
    assert_eq!(
        consumed.len(),
        1,
        "one complete exact AgentMessage input per control"
    );
    let (consumed_seq, turn, message) = consumed[0];
    assert_eq!(message.source_session_id, parent_id);
    assert_eq!(message.target_session_id, change.child_session_id);
    assert!(message.reply_to.is_none());
    let receipts: Vec<_> = child
        .timeline
        .events()
        .iter()
        .filter_map(|event| match &event.kind {
            TimelineEventKind::Notification(NotificationEvent::Received {
                id,
                owner_session_id,
                source,
                source_version,
                payload_ref,
            }) if id == &message.receipt_id => Some((
                event.seq.get(),
                owner_session_id,
                source,
                source_version,
                payload_ref,
            )),
            _ => None,
        })
        .collect();
    assert_eq!(receipts.len(), 1, "exact durable typed receipt");
    let (received_seq, owner, source, version, payload) = receipts[0];
    assert_eq!(owner, &change.child_session_id);
    assert_eq!(
        source,
        &NotificationSource::PlanGuidance {
            parent_session_id: parent_id.into(),
            message_id: message.message_id.clone()
        }
    );
    assert_eq!(chat_state::PARENT_MESSAGE_SOURCE_VERSION, 2);
    assert_eq!(version, &NotificationSourceVersion::Ordinal { value: 2 });
    assert_eq!(
        child.timeline.received_notification_id(source, version),
        Some(message.receipt_id.as_str())
    );
    assert_eq!(
        child.timeline.consumed_notification_turn(source, version),
        Some(turn)
    );
    assert_eq!(
        chat_state::notification_id(owner, source, version).unwrap(),
        message.receipt_id
    );
    // An ordinary same-parent/message receipt would be a different source.
    assert!(
        child
            .timeline
            .consumed_notification_turn(
                &NotificationSource::ParentMessage {
                    parent_session_id: parent_id.into(),
                    message_id: message.message_id.clone(),
                    interrupt: true,
                },
                version
            )
            .is_none()
    );
    assert_eq!(payload.bytes, message.message.len() as u64);
    assert_eq!(
        payload.blake3,
        blake3::hash(message.message.as_bytes()).to_hex().as_str()
    );
    let artifact = child
        .path
        .parent()
        .unwrap()
        .join("artifacts/notifications")
        .join(format!("{}.txt", payload.blake3));
    assert_eq!(
        std::fs::read_to_string(artifact).unwrap(),
        message.message.as_ref()
    );
    let (prefix, details) = message.message.split_once('\n').unwrap();
    assert_eq!(
        prefix,
        if change.is_stop() {
            STOP_PREFIX
        } else {
            REPLAN_PREFIX
        }
    );
    let details: Value = serde_json::from_str(details).unwrap();
    assert_eq!(details["parent_session_id"], parent_id);
    assert_eq!(details["child_session_id"], change.child_session_id);
    assert_eq!(details["link_id"], change.link_id);
    assert_eq!(details["step_id"], change.step_id);
    assert_eq!(details["change_id"], change.id);
    assert_eq!(details["source_revision"], change.source_revision);
    assert_eq!(
        details["directive"],
        if change.is_stop() { "stop" } else { "replan" }
    );
    assert_eq!(details["removed"], change.removed);
    assert_eq!(
        details["priority"],
        serde_json::to_value(change.priority).unwrap()
    );
    assert_eq!(
        details["status"],
        serde_json::to_value(change.status).unwrap()
    );
    let requirements = change.requirements.as_deref().unwrap_or_default();
    assert_eq!(
        details["requirements_summary"],
        json!({
            "total":requirements.len(),"omitted_items":0,
            "items":requirements.iter().map(|text| json!({"text":text,"omitted_bytes":0})).collect::<Vec<_>>()
        })
    );
    assert_eq!(
        details["content_summary"],
        json!({"text":change.content,"omitted_bytes":0})
    );
    assert!(message.message.len() <= 16 * 1024);
    assert!(change_seq(child, change, &ReplanProgress::Received) < received_seq);
    assert!(received_seq < consumed_seq);
    eprintln!(
        "typed control evidence={}",
        json!({"source":source,"version":version,"received_seq":received_seq,"consumed_seq":consumed_seq,"consumed_turn":turn,"input":message,"details":details})
    );
    ControlEvidence {
        message: message.clone(),
        source: source.clone(),
        version: version.clone(),
        received_seq,
        consumed_seq,
    }
}

fn assert_wire_control(body: &Value, evidence: &ControlEvidence) {
    let call_id = sampling_types::agent_message_call_id(&evidence.message.receipt_id);
    let calls: Vec<_> = body["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|message| message["tool_calls"].as_array())
        .flatten()
        .filter(|call| call["id"] == call_id)
        .collect();
    assert_eq!(
        calls.len(),
        1,
        "one typed input in this actual provider request"
    );
    assert_eq!(
        calls[0]["function"]["name"],
        sampling_types::RECEIVE_AGENT_MESSAGE_TOOL_NAME
    );
    let arguments: Value =
        serde_json::from_str(calls[0]["function"]["arguments"].as_str().unwrap()).unwrap();
    assert_eq!(arguments["receipt_id"], evidence.message.receipt_id);
    assert_eq!(
        arguments["source_session_id"],
        evidence.message.source_session_id
    );
    assert_eq!(arguments["message_id"], evidence.message.message_id);
    let actual: Value = serde_json::from_str(tool_output(body, &call_id)).unwrap();
    assert_eq!(actual, serde_json::to_value(&evidence.message).unwrap());
    eprintln!(
        "actual control wire={}",
        json!({"source":evidence.source,"version":evidence.version,"call":calls[0],"tool_result":actual})
    );
    assert!(
        body["messages"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|message| message["role"] == "user")
            .all(|message| !message["content"].to_string().contains(
                if evidence.message.message.starts_with(STOP_PREFIX) {
                    STOP_PREFIX
                } else {
                    REPLAN_PREFIX
                }
            )),
        "ordinary user text cannot stand in for typed control"
    );
}

fn assert_handled(
    journal: &Journal,
    change: &ReplanChange,
    outcome: &ReplanOutcome,
    incoming: bool,
) -> u64 {
    let handled = ReplanProgress::Handled {
        outcome: outcome.clone(),
    };
    let received = change_seq(journal, change, &ReplanProgress::Received);
    let consumed = change_seq(journal, change, &ReplanProgress::Consumed);
    let handled_seq = change_seq(journal, change, &handled);
    assert!(
        received < consumed && consumed < handled_seq,
        "Received, Consumed and Handled are separate durable facts"
    );
    let record = journal
        .plan()
        .replans
        .iter()
        .find(|record| record.id == change.id)
        .unwrap();
    same_change(change, record);
    assert_eq!(record.progress, handled);
    if !incoming {
        let pending = change_seq(journal, change, &ReplanProgress::Pending);
        let dispatching = change_seq(journal, change, &ReplanProgress::Dispatching);
        assert!(pending < dispatching && dispatching < received);
    }
    handled_seq
}

fn assert_unfinished(parent: &Journal, child: &Journal, task_id: &str) {
    let link = parent
        .plan()
        .links
        .iter()
        .find(|link| link.id == task_id)
        .unwrap();
    assert!(link.required && !link.terminal);
    assert!(link.result_id.is_none() && link.dispatch_failure.is_none());
    assert_eq!(
        parent
            .plan()
            .unresolved_required_deliveries()
            .iter()
            .map(|link| link.id.as_str())
            .collect::<Vec<_>>(),
        [task_id]
    );
    assert!(
        parent.plan().reviews.is_empty(),
        "control ACK cannot auto-accept required delivery"
    );
    assert!(!parent.timeline.events().iter().any(|event| matches!(
        &event.kind,
        TimelineEventKind::Subagent(SubagentEvent::Ended(_))
    )));
    assert!(
        !child
            .timeline
            .events()
            .iter()
            .any(|event| matches!(&event.kind, TimelineEventKind::SubagentResult(_)))
    );
}

fn assert_local_work(child: &Journal, content: &str, status: PlanStepStatus) {
    let investigation = child
        .plan()
        .steps
        .iter()
        .find(|step| step.id == INVESTIGATION)
        .unwrap();
    assert_eq!(
        investigation.content,
        "Completed investigation remains valid."
    );
    assert_eq!(investigation.status, PlanStepStatus::Completed);
    assert!(!investigation.removed);
    let affected = child
        .plan()
        .steps
        .iter()
        .find(|step| step.id == AFFECTED)
        .unwrap();
    assert_eq!(affected.content, content);
    assert_eq!(affected.status, status);
    assert!(!affected.removed);
    assert_eq!(
        child.plan().steps.len(),
        2,
        "only affected local work changes"
    );
}

// The ACK worker is asynchronous. Observe it via real native reads, each bounded
// by a provider response barrier, instead of assuming a delay or racing a sleep.
async fn parent_observes_handled(
    server: &MockInferenceServer,
    client: &GrowStdioClient,
    grow_home: &Path,
    parent_id: &str,
    mut current: InferenceExpectation,
    change: &ReplanChange,
    outcome: &ReplanOutcome,
) -> InferenceExpectation {
    for attempt in 0..32 {
        let previous_call = current.name().to_owned();
        let next_call = format!(
            "call_parent_observe_{}_{}",
            if change.is_stop() { "stop" } else { "edit" },
            attempt
        );
        let mut next = hold_tool(
            server,
            &next_call,
            "todo_read",
            json!({"target":"self","limit":50}),
        );
        current.release();
        let body = blocked(&mut next, server, client).await;
        current.assert_satisfied();
        let parent = journal(grow_home, parent_id);
        let read = read_output(&body, &previous_call, &parent, "self", false);
        let record = read
            .plan
            .as_ref()
            .unwrap()
            .replans
            .iter()
            .find(|record| record.id == change.id)
            .unwrap();
        same_change(change, record);
        if record.progress
            == (ReplanProgress::Handled {
                outcome: outcome.clone(),
            })
        {
            assert_handled(&parent, change, outcome, false);
            eprintln!(
                "parent observed exact {} ACK after {} native reads: {}",
                if change.is_stop() {
                    "Stopping"
                } else {
                    "Updated"
                },
                attempt + 1,
                serde_json::to_string(&read).unwrap()
            );
            return next;
        }
        assert!(matches!(
            record.progress,
            ReplanProgress::Dispatching | ReplanProgress::Received | ReplanProgress::Consumed
        ));
        current = next;
    }
    panic!(
        "parent did not observe the exact canonical child ACK\n{}",
        client.stderr()
    );
}

fn assert_no_runtime(value: &Value) {
    match value {
        Value::String(text) => {
            assert!(
                !text.contains(GUIDANCE),
                "request-only runtime GUIDANCE persisted"
            );
            assert!(
                !(text.contains("\"read_entry\"") && text.contains("\"associated_step\"")),
                "runtime structure persisted without its header"
            );
        }
        Value::Array(values) => values.iter().for_each(assert_no_runtime),
        Value::Object(object) => {
            assert!(
                !(object.contains_key("read_entry")
                    && object.contains_key("child_session_id")
                    && object.contains_key("parent"))
            );
            object.values().for_each(assert_no_runtime);
        }
        _ => {}
    }
}

fn assert_no_control_notice(notification: &SessionNotification, controls: &[&ControlEvidence]) {
    if let SessionUpdate::UiNotice(notice) = &notification.update {
        for control in controls {
            assert_ne!(notice.correlation_id, control.message.receipt_id);
            if let Some(message) = AgentMessageNotice::from_notice(notice) {
                assert!(
                    message.source_session_id != control.message.source_session_id
                        || message.message_id != control.message.message_id,
                    "hidden planning control became an ordinary coordination notice"
                );
            }
        }
    }
}

fn assert_visible(value: &Value) {
    assert_no_runtime(value);
    let text = value.to_string();
    assert!(
        !text.contains(REPLAN_PREFIX) && !text.contains(STOP_PREFIX),
        "fixed internal controls leaked to visible history"
    );
}

fn assert_persistent_projection(
    grow_home: &Path,
    journal: &Journal,
    controls: &[&ControlEvidence],
) {
    assert_no_runtime(&serde_json::to_value(journal.timeline.events()).unwrap());
    assert_no_runtime(&serde_json::to_value(journal.timeline.surface()).unwrap());
    for fact in &journal.presentation {
        assert_visible(fact);
    }
    let mut rows = 0;
    let emission =
        storage::stream_session_history_at(&journal.plan().session_id, grow_home, |update| {
            if let HistoryUpdate::Grow(notification) = &update {
                assert_no_control_notice(notification, controls);
            }
            assert_visible(&serde_json::to_value(&update).unwrap());
            rows += 1;
        })
        .expect("public cold history projection validates the actual journal");
    assert_eq!(emission, storage::ReplayEmission::Emitted);
    assert!(rows > 0);
    let transcript = storage::transcript::read_session_at(&journal.plan().session_id, grow_home)
        .expect("public transcript validates actual canonical references");
    assert_eq!(
        transcript.last_timeline_seq,
        journal
            .timeline
            .events()
            .last()
            .map(|event| event.seq.get())
    );
    for event in transcript.events {
        if let HistoryUpdate::Grow(notification) = &event.update {
            assert_no_control_notice(notification, controls);
        }
        assert_visible(&serde_json::to_value(event.update).unwrap());
    }
}

fn assert_result(
    parent: &Journal,
    child: &Journal,
    spawn: &SubagentSpawnEvent,
    stop: &ReplanChange,
) -> TimelineRangeRef {
    let results: Vec<_> = child
        .timeline
        .events()
        .iter()
        .filter_map(|event| match &event.kind {
            TimelineEventKind::SubagentResult(result) => Some((event.seq.get(), result)),
            _ => None,
        })
        .collect();
    assert_eq!(results.len(), 1, "one normal canonical Result");
    let (result_seq, result) = results[0];
    assert_eq!(result.subagent_id, spawn.subagent_id);
    assert_eq!(result.outcome, SubagentOutcome::Completed);
    assert!(result.error.is_none());
    assert!(assert_handled(child, stop, &ReplanOutcome::Stopping, true) < result_seq);
    let reference = TimelineRangeRef {
        timeline_id: spawn.child_session_id.clone(),
        first_seq: result_seq,
        last_seq: result_seq,
    };
    let ended: Vec<_> = parent
        .timeline
        .events()
        .iter()
        .filter_map(|event| match &event.kind {
            TimelineEventKind::Subagent(SubagentEvent::Ended(ended)) => {
                Some((event.seq.get(), ended))
            }
            _ => None,
        })
        .collect();
    assert_eq!(ended.len(), 1);
    let (ended_seq, ended) = ended[0];
    assert!(assert_handled(parent, stop, &ReplanOutcome::Stopping, false) < ended_seq);
    if let Some(accepted) = parent.timeline.events().iter().find(|event| matches!(&event.kind,
        TimelineEventKind::Planning(planning) if planning.plan.reviews.iter().any(|review| review.link_id == spawn.subagent_id && review.decision == ReviewDecision::Accepted))) {
        assert!(ended_seq < accepted.seq.get(), "native acceptance follows the canonical parent terminal");
    }
    assert_eq!(ended.subagent_id, spawn.subagent_id);
    assert_eq!(ended.child_session_id, spawn.child_session_id);
    assert_eq!(ended.result_ref.as_ref(), Some(&reference));
    assert_eq!(ended.outcome, result.outcome);
    assert_eq!(ended.error, result.error);
    assert_eq!(ended.duration_ms, result.duration_ms);
    assert_eq!(ended.tool_calls, result.tool_calls);
    assert_eq!(ended.turns, result.turns);
    assert_eq!(ended.tokens_used, result.tokens_used);
    let link = parent
        .plan()
        .links
        .iter()
        .find(|link| link.id == spawn.subagent_id)
        .unwrap();
    assert!(link.required && link.terminal && link.dispatch_failure.is_none());
    assert!(link.pending_source_revision.is_none());
    assert_eq!(
        serde_json::from_str::<TimelineRangeRef>(link.result_id.as_deref().unwrap()).unwrap(),
        reference
    );
    let output_ref = result
        .output_ref
        .as_deref()
        .expect("normal output has an immutable artifact");
    let hash = output_ref
        .strip_prefix("artifact:subagent-output:blake3:")
        .unwrap();
    assert_eq!(hash.len(), 64);
    assert!(hash.bytes().all(|byte| byte.is_ascii_hexdigit()));
    let bytes = std::fs::read(
        child
            .path
            .parent()
            .unwrap()
            .join("artifacts/subagent-output")
            .join(format!("{hash}.json")),
    )
    .unwrap();
    assert_eq!(blake3::hash(&bytes).to_hex().as_str(), hash);
    let output: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(output["schema_version"], 1);
    assert_eq!(output["output"], REPORT);
    eprintln!(
        "normal terminal evidence={}",
        json!({"child_result_seq":result_seq,"result":result,"parent_ended_seq":ended_seq,"ended":ended,"canonical_reference":reference})
    );
    reference
}

fn assert_not_dispatched(journal: &Journal, call_id: &str) {
    assert!(!journal.timeline.events().iter().any(|event| matches!(&event.kind, TimelineEventKind::Tool(ToolEvent::Started { call_id: actual, .. }) if actual == call_id)), "abandoned response {call_id} must never dispatch its tool");
}

#[tokio::test]
#[ignore = "requires the current GROW_BINARY; only loopback providers and test-owned state"]
async fn native_todo_edit_remove_controls_background_child_through_normal_result_and_review() {
    tokio::task::LocalSet::new().run_until(async {
        // Fail before the shared harness can select or build a fallback binary.
        let binary = std::env::var("GROW_BINARY").expect("set GROW_BINARY to the current build");
        let binary = std::fs::canonicalize(binary).expect("GROW_BINARY must exist");
        assert!(binary.is_file());
        let parent_server = MockInferenceServer::start_with_models(vec![MockModelEntry::with_agent_type("parent-wire", "grow-build")]).await.unwrap();
        let child_server = MockInferenceServer::start_with_models(vec![MockModelEntry::with_agent_type("child-wire", "grow-build")]).await.unwrap();
        assert_ne!(parent_server.url(), child_server.url());
        // ask_parent uses a tool-free auxiliary call on the parent's endpoint.
        // Foreground expectations must never steal that sideband response.
        parent_server.set_response(AUX_REPLY);
        child_server.set_response("Unexpected unscripted foreground request in the isolated fixture.");
        let sandbox = TestSandbox::builder().git().mock_url(parent_server.url()).build();
        let workspace = sandbox.workspace().to_path_buf();
        let grow_home = sandbox.grow_home().to_path_buf();
        for path in [sandbox.home(), sandbox.grow_home(), sandbox.workspace(), sandbox.temp_dir()] {
            assert!(path.starts_with(sandbox.root()));
        }
        assert!(sandbox.env().iter().any(|(key, value)| key == "GROW_HOME" && Path::new(value) == grow_home));
        assert!(!sandbox.env().iter().any(|(key, _)| key == "OPENAI_API_KEY" || key == "ANTHROPIC_API_KEY" || key == "GROW_GOAL"));
        std::fs::write(grow_home.join("config.toml"), format!(r#"[models]
default = "mock/parent"
[provider.mock]
api_backend = "chat_completions"
[provider.mock.options]
base_url = {:?}
env_key = "GROW_API_KEY"
[provider.mock.models.parent]
model = "parent-wire"
context_window = 200000
agent_type = "grow-build"
[provider.child]
api_backend = "chat_completions"
[provider.child.options]
base_url = {:?}
env_key = "GROW_API_KEY"
[provider.child.models.control]
model = "child-wire"
context_window = 200000
agent_type = "grow-build"
"#, parent_server.url(), child_server.url())).unwrap();
        eprintln!("isolated milestone binary={} GROW_HOME={} workspace={} parent={} child={}", binary.display(), grow_home.display(), workspace.display(), parent_server.url(), child_server.url());
        let mut client = GrowStdioClient::spawn_with_sandbox(&parent_server, &workspace, sandbox).await;
        client.initialize_with_timeout().await;
        let session = client.create_session_with_model_timeout(&workspace, "mock/parent").await;
        let parent_id = session.0.as_ref();
        let create = expect_tool(&parent_server, "call_parent_plan", "todo_write", json!({"todos":[
            {"id":"neighbor-before","content":"Preserve the preceding parent neighbor.","status":"completed"},
            {"id":STEP,"content":OBJECTIVE,"requirements":["Preserve completed investigation."],"status":"in_progress","priority":"high"},
            {"id":"neighbor-after","content":"Preserve the following parent neighbor.","status":"pending"}
        ]}));
        let spawn_call = expect_tool(&parent_server, "call_spawn", "spawn_subagent", json!({
            "parent_step_id":STEP,"description":"Verify isolated TODO control lifecycle","prompt":TASK_PROMPT,
            "subagent_type":"general-purpose","background":true,"required_deliverable":true,"isolation":"none","model":"child/control"
        }));
        let mut before_edit = hold_tool(&parent_server, "call_parent_before_edit", "todo_read", json!({"target":"self","limit":50}));
        let mut first = hold_tool(&child_server, "call_child_first_parent_read", "todo_read", json!({"target":"parent","limit":50}));
        let (reply, (actual_spawn, edit, stop, edit_message, stop_message, binding)) = tokio::join!(
            tokio::time::timeout(scaled(Duration::from_secs(120)), client.prompt(&session, "Exercise this isolated native TODO delegation, control, normal result, and explicit review.")),
            async {
                let (parent_body, first_body) = tokio::join!(blocked(&mut before_edit, &parent_server, &client), blocked(&mut first, &child_server, &client));
                create.assert_satisfied();
                spawn_call.assert_satisfied();
                assert_eq!(foreground(&parent_server).len(), 3);
                assert_eq!(foreground(&child_server).len(), 1, "capture the actual FIRST child request");
                let parent = journal(&grow_home, parent_id);
                let (spawn_seq, actual_spawn) = spawn(&parent);
                let actual_spawn = actual_spawn.clone();
                let task_id = actual_spawn.subagent_id.as_str();
                let child_id = actual_spawn.child_session_id.as_str();
                let child = journal(&grow_home, child_id);
                assert_eq!(actual_spawn.prompt, TASK_PROMPT, "authored prompt cannot carry legacy appended planning context");
                assert!(!actual_spawn.prompt.contains("Current delegated parent step"));
                assert_eq!(actual_spawn.parent_step_id.as_deref(), Some(STEP));
                assert_eq!(actual_spawn.security_parent_session_id, parent_id);
                assert_eq!(actual_spawn.effective_model_id, "child/control");
                assert!(actual_spawn.resumed_from.is_none() && actual_spawn.worktree_path.is_none());
                assert_eq!(Path::new(&actual_spawn.child_cwd), workspace);
                assert!(actual_spawn.birth_mcp_servers.as_ref().unwrap().is_empty());
                let returned_task_id = tool_output(&parent_body, "call_spawn").lines().find_map(|line| line.trim().strip_prefix("subagent_id: ")).unwrap();
                assert_eq!(returned_task_id, task_id, "native background spawn reports the actual canonical identity");
                let first_link = parent.timeline.events().iter().find(|event| matches!(&event.kind, TimelineEventKind::Planning(planning) if planning.plan.links.iter().any(|link| link.id == task_id))).unwrap();
                assert!(first_link.seq.get() < spawn_seq, "durable Link ACK must precede real Spawn");
                let seeds: Vec<_> = child.timeline.events().iter().filter_map(|event| match &event.kind { TimelineEventKind::SubagentSeed(seed) => Some(seed), _ => None }).collect();
                assert_eq!(seeds.len(), 1);
                assert_eq!(seeds[0].parent_timeline_id, parent_id);
                assert_eq!(seeds[0].security_parent_session_id, parent_id);
                assert_eq!(seeds[0].parent_spawn_seq, spawn_seq);
                assert_eq!(seeds[0].subagent_id, task_id);
                assert!(child.timeline.planning_snapshot().is_none());
                let first_context = assert_context(&first_body, &parent, &actual_spawn, OBJECTIVE, false);
                assert!(first_context["self"].is_null());
                assert_eq!(first_context["parent"]["associated_step"]["position"], 2);
                assert_eq!(first_context["parent"]["counts"]["steps"], 3);
                assert_eq!(first_context["parent"]["control_summary"]["directive"], "none");
                assert_unfinished(&parent, &child, task_id);
                // The standard Agent exposes Task's canonical parameter through
                // the registry's configured model-facing name (agent/config.rs).
                let spawn_schema = tools::util::remap::remap_schema_properties(
                    &generate_schema::<TaskToolInput>(),
                    &std::collections::HashMap::from([("run_in_background".to_owned(), "background".to_owned())]),
                );
                let native_schemas = json!({
                    "spawn_subagent":schema(&parent_body,"spawn_subagent",spawn_schema),
                    "todo_write":schema(&first_body,"todo_write",generate_schema::<TodoWriteInput>()),
                    "todo_read":schema(&first_body,"todo_read",generate_schema::<PlanReadInput>()),
                    "ask_parent":schema(&first_body,"ask_parent",generate_schema::<AskParentInput>()),
                    "wait_subagent":schema(&parent_body,"wait_subagent",generate_schema::<WaitSubagentInput>())
                });
                assert!(native_schemas["spawn_subagent"]["parameters"]["required"].as_array().unwrap().iter().any(|field| field == "parent_step_id"));
                eprintln!("FIRST evidence={}", json!({"request_id":foreground(&child_server)[0].header("x-grow-req-id"),"runtime":first_context,"schemas":native_schemas,"link_event":first_link,"spawn_seq":spawn_seq,"spawn":actual_spawn,"seed":seeds[0]}));

                let ask = expect_tool(&child_server, "call_child_ask_parent", "ask_parent", json!({"question":QUESTION}));
                let local = expect_tool(&child_server, "call_child_local_plan", "todo_write", json!({"todos":[
                    {"id":INVESTIGATION,"content":"Completed investigation remains valid.","status":"completed"},
                    {"id":AFFECTED,"content":"Verify the original acceptance.","status":"in_progress"}
                ]}));
                let mut abandoned_edit = hold_tool(&child_server, "call_abandoned_before_edit", "todo_read", json!({"target":"self","limit":50}));
                first.release();
                let ready_body = blocked(&mut abandoned_edit, &child_server, &client).await;
                first.assert_satisfied(); ask.assert_satisfied(); local.assert_satisfied();
                let parent = journal(&grow_home, parent_id);
                let child = journal(&grow_home, child_id);
                let first_read = read_output(&ready_body, "call_child_first_parent_read", &parent, "parent", true);
                assert_eq!(first_read.plan.as_ref().unwrap().steps[1].content, OBJECTIVE);
                let answer: AgentInteractionOutput = serde_json::from_str(tool_output(&ready_body, "call_child_ask_parent")).unwrap();
                assert_eq!(answer.status, "answered");
                assert_eq!(answer.answer.as_deref(), Some(AUX_REPLY));
                assert!(answer.error.is_none());
                let auxiliary: Vec<_> = parent_server.requests().into_iter().filter(|entry| !is_foreground(entry) && entry.body.as_ref().is_some_and(|body| body.to_string().contains(QUESTION))).collect();
                assert_eq!(auxiliary.len(), 1, "the real parent answered through its auxiliary endpoint");
                let auxiliary_body = auxiliary[0].body.as_ref().unwrap();
                assert!(auxiliary_body["tools"].is_null() || auxiliary_body["tools"].as_array().is_some_and(Vec::is_empty), "parent sideband is tool-free");
                assert_eq!(foreground(&parent_server).len(), 3, "ask_parent cannot consume the parked parent foreground script");
                assert_local_work(&child, "Verify the original acceptance.", PlanStepStatus::InProgress);
                assert_no_runtime(&serde_json::to_value(child.timeline.events()).unwrap());
                let question_wire = auxiliary_body["messages"].as_array().unwrap().iter().rev().find(|message| message["content"].to_string().contains(QUESTION)).unwrap();
                eprintln!("native clarification evidence={}", json!({"read":first_read,"ask":answer,"aux_request_id":auxiliary[0].header("x-grow-req-id"),"aux_model":auxiliary_body["model"],"aux_tools":auxiliary_body["tools"],"question_wire":question_wire}));

                let parent_edit = expect_tool(&parent_server, "call_parent_edit", "todo_write", json!({"todos":[{"id":STEP,"content":EDITED_OBJECTIVE,"requirements":EDITED_REQUIREMENTS}]}));
                let mut after_edit = hold_tool(&parent_server, "call_parent_after_edit", "todo_read", json!({"target":"self","limit":50}));
                let expected_edit_id = format!("{task_id}:{STEP}:{}", parent.plan().revision + 1);
                let mut edit_request = hold_tool(&child_server, "call_child_handle_edit", "todo_write", json!({
                    "todos":[{"id":AFFECTED,"content":"Verify the edited acceptance.","requirements":EDITED_REQUIREMENTS}],
                    "actions":[{"handle_replans":{"ids":[expected_edit_id]}}]
                }));
                before_edit.release();
                let (_, edit_body) = tokio::join!(blocked(&mut after_edit, &parent_server, &client), blocked(&mut edit_request, &child_server, &client));
                before_edit.assert_satisfied(); parent_edit.assert_satisfied();
                let parent = journal(&grow_home, parent_id);
                let child = journal(&grow_home, child_id);
                assert_eq!(parent.plan().replans.len(), 1);
                let edit = parent.plan().replans[0].clone();
                assert_eq!(edit.id, expected_edit_id);
                assert!(!edit.is_stop());
                assert_eq!(edit.link_id, task_id);
                assert_eq!(edit.child_session_id, child_id);
                assert_eq!(edit.step_id, STEP);
                assert_eq!(edit.content, EDITED_OBJECTIVE);
                assert_eq!(edit.requirements.as_deref().unwrap(), EDITED_REQUIREMENTS);
                assert_eq!(edit.source_revision, parent.plan().revision);
                let edit_control = control_evidence(&child, parent_id, &edit);
                assert_wire_control(&edit_body, &edit_control);
                let edit_context = assert_context(&edit_body, &parent, &actual_spawn, OBJECTIVE, false);
                assert_eq!(edit_context["parent"]["control_summary"]["directive"], "none");
                assert_eq!(edit_context["parent"]["source"], first_context["parent"]["source"]);
                assert_unfinished(&parent, &child, task_id);
                // The soft interrupt abandoned this old sample. Only the NEW
                // request above proves input consumption; release is cleanup.
                abandoned_edit.release();
                assert_not_dispatched(&child, "call_abandoned_before_edit");
                eprintln!("EDIT request evidence={}", json!({"runtime":edit_context,"typed_source":edit_control.source,"source_version":edit_control.version,"source_change":edit}));

                let mut abandoned_stop = hold_tool(&child_server, "call_abandoned_before_stop", "todo_read", json!({"target":"parent","limit":50}));
                // Apply the exact notification directly, without a plan read.
                edit_request.release();
                let _ = blocked(&mut abandoned_stop, &child_server, &client).await;
                edit_request.assert_satisfied();
                let child = journal(&grow_home, child_id);
                assert_local_work(&child, "Verify the edited acceptance.", PlanStepStatus::InProgress);
                let edit_outcome = match &child.plan().replans.iter().find(|change| change.id == edit.id).unwrap().progress {
                    ReplanProgress::Handled { outcome: outcome @ ReplanOutcome::Updated { .. } } => outcome.clone(),
                    other => panic!("native affected-step update must produce Updated: {other:?}"),
                };
                let child_edit_handled = assert_handled(&child, &edit, &edit_outcome, true);
                assert!(edit_control.consumed_seq < change_seq(&child, &edit, &ReplanProgress::Consumed));
                assert!(edit_control.received_seq < edit_control.consumed_seq && edit_control.consumed_seq < child_edit_handled);
                let parent_parked = parent_observes_handled(&parent_server, &client, &grow_home, parent_id, after_edit, &edit, &edit_outcome).await;
                let parent = journal(&grow_home, parent_id);
                assert_unfinished(&parent, &child, task_id);
                assert_handled(&parent, &edit, &edit_outcome, false);

                let remove = expect_tool(&parent_server, "call_parent_remove", "todo_write", json!({"todos":[{"id":STEP,"removed":true}]}));
                let mut after_remove = hold_tool(&parent_server, "call_parent_after_remove", "todo_read", json!({"target":"self","limit":50}));
                let mut stop_request = hold_tool(&child_server, "call_child_stop_parent_read", "todo_read", json!({"target":"parent","limit":50}));
                parent_parked.release();
                let (_, stop_body) = tokio::join!(blocked(&mut after_remove, &parent_server, &client), blocked(&mut stop_request, &child_server, &client));
                parent_parked.assert_satisfied(); remove.assert_satisfied();
                let parent = journal(&grow_home, parent_id);
                let child = journal(&grow_home, child_id);
                assert_eq!(parent.plan().replans.len(), 2);
                let stop = parent.plan().replans.iter().find(|change| change.is_stop()).unwrap().clone();
                assert!(stop.removed);
                assert_ne!(stop.id, edit.id);
                assert_eq!(stop.content, EDITED_OBJECTIVE);
                assert!(stop.source_revision > edit.source_revision);
                assert!(parent.plan().link_stop_requested(task_id));
                let stop_control = control_evidence(&child, parent_id, &stop);
                assert_ne!(stop_control.message.message_id, edit_control.message.message_id);
                assert_wire_control(&stop_body, &stop_control);
                assert_wire_control(&stop_body, &edit_control);
                let stop_context = assert_context(&stop_body, &parent, &actual_spawn, OBJECTIVE, false);
                assert_eq!(stop_context["parent"]["source"], first_context["parent"]["source"]);
                assert_eq!(stop_context["self"]["replans_summary"][0]["id"], stop.id);
                assert_eq!(stop_context["self"]["replans_summary"][0]["directive"], "stop_and_wrap_up");
                assert_unfinished(&parent, &child, task_id);
                abandoned_stop.release();
                assert_not_dispatched(&child, "call_abandoned_before_stop");
                eprintln!("STOP request evidence={}", json!({"runtime":stop_context,"typed_source":stop_control.source,"source_version":stop_control.version,"source_change":stop}));

                let stop_self_read = expect_tool(&child_server, "call_child_stop_self_read", "todo_read", json!({"target":"self","limit":50}));
                let mut stop_reads = hold_tool(&child_server, "call_child_after_stop_reads", "todo_read", json!({"target":"self","limit":50}));
                stop_request.release();
                let stop_reads_body = blocked(&mut stop_reads, &child_server, &client).await;
                stop_request.assert_satisfied(); stop_self_read.assert_satisfied();
                let parent = journal(&grow_home, parent_id);
                let child = journal(&grow_home, child_id);
                let parent_read = read_output(&stop_reads_body, "call_child_stop_parent_read", &parent, "parent", true);
                let self_read = read_output(&stop_reads_body, "call_child_stop_self_read", &child, "self", false);
                let native_stop = self_read.plan.as_ref().unwrap().replans.iter().find(|change| change.id == stop.id).unwrap();
                same_change(&stop, native_stop);
                same_change(native_stop, parent_read.plan.as_ref().unwrap().replans.iter().find(|change| change.id == native_stop.id).unwrap());
                eprintln!("STOP native read evidence={}", json!({"parent":parent_read,"self":self_read,"source_planning_event":parent.timeline.events().iter().find(|event| event.seq.get() == change_seq(&parent, &stop, &ReplanProgress::Pending)).unwrap()}));
                let before_invalid = child.plan().clone();
                // Counterexample: a genuine typed stop cannot become permission
                // to continue by attaching an ordinary no-change decision.
                let invalid = expect_tool(&child_server, "call_child_reject_stop_no_change", "todo_write", json!({
                    "todos":[{"id":AFFECTED,"content":"Forbidden resumed objective."}],
                    "actions":[{"handle_replans":{"ids":[native_stop.id],"no_change_reason":"The previous plan still applies."}}]
                }));
                let mut rejected = hold_tool(&child_server, "call_child_after_invalid_stop", "todo_read", json!({"target":"self","limit":50}));
                stop_reads.release();
                let rejected_body = blocked(&mut rejected, &child_server, &client).await;
                stop_reads.assert_satisfied(); invalid.assert_satisfied();
                let error = tool_output(&rejected_body, "call_child_reject_stop_no_change");
                assert!(error.contains("stop guidance cannot be handled with a no-change reason"), "actual native preflight rejection: {error}");
                let child = journal(&grow_home, child_id);
                assert_eq!(child.plan(), &before_invalid, "invalid whole command cannot commit edits or Consumed/Handled");
                assert_local_work(&child, "Verify the edited acceptance.", PlanStepStatus::InProgress);
                let handle_stop = expect_tool(&child_server, "call_child_handle_stop", "todo_write", json!({
                    "todos":[{"id":AFFECTED,"status":"cancelled"}],
                    "actions":[{"handle_replans":{"ids":[native_stop.id]}}]
                }));
                let mut report = child_server.expect_response_blocked("child-normal-settlement-report", InferenceRequestMatcher::foreground(InferenceEndpoint::ChatCompletions), completion_response(REPORT));
                rejected.release();
                let report_body = blocked(&mut report, &child_server, &client).await;
                rejected.assert_satisfied(); handle_stop.assert_satisfied();
                let child = journal(&grow_home, child_id);
                assert_local_work(&child, "Verify the edited acceptance.", PlanStepStatus::Cancelled);
                assert_handled(&child, &stop, &ReplanOutcome::Stopping, true);
                assert!(stop_control.consumed_seq < change_seq(&child, &stop, &ReplanProgress::Consumed));
                assert_wire_control(&report_body, &stop_control);
                let parent_parked = parent_observes_handled(&parent_server, &client, &grow_home, parent_id, after_remove, &stop, &ReplanOutcome::Stopping).await;
                let parent = journal(&grow_home, parent_id);
                assert_unfinished(&parent, &child, task_id);
                assert!(parent.plan().steps.iter().find(|step| step.id == STEP).unwrap().removed);
                assert!(parent.plan().links[0].pending_source_revision.is_none());
                assert_eq!(child.plan().replans.len(), 2);
                assert_handled(&parent, &edit, &edit_outcome, false);
                assert_handled(&parent, &stop, &ReplanOutcome::Stopping, false);
                eprintln!("Stopping is nonterminal evidence={}", json!({"parent_plan":parent.plan(),"child_plan":child.plan(),"result_exists":false,"required_review_accepted":false}));

                let wait = expect_tool(&parent_server, "call_parent_wait", "wait_subagent", json!({"subagent_ids":[task_id],"timeout_ms":30000}));
                let mut after_wait = hold_tool(&parent_server, "call_parent_after_wait", "todo_read", json!({"target":"self","limit":50}));
                parent_parked.release();
                // The only terminal response is released after BOTH canonical
                // planning owners have durably recorded the same Stopping ACK.
                report.release();
                let waited_body = blocked(&mut after_wait, &parent_server, &client).await;
                parent_parked.assert_satisfied(); report.assert_satisfied(); wait.assert_satisfied();
                let waited: WaitSubagentOutput = serde_json::from_str(tool_output(&waited_body, "call_parent_wait")).unwrap();
                assert_eq!(waited.reason, SubagentWaitReason::AllFinished);
                assert_eq!(waited.subagents.len(), 1);
                assert_eq!(waited.subagents[0].subagent_id, task_id);
                assert_eq!(waited.subagents[0].status, SubagentStatus::Completed);
                assert_eq!(waited.subagents[0].output.as_deref(), Some(REPORT));
                assert!(waited.subagents[0].error.is_none());
                let parent = journal(&grow_home, parent_id);
                let child = journal(&grow_home, child_id);
                let reference = assert_result(&parent, &child, &actual_spawn, &stop);
                assert!(parent.plan().reviews.is_empty(), "normal Result still requires explicit native review");
                assert_eq!(parent.plan().unresolved_required_deliveries().len(), 1);
                assert_eq!(parent.plan().steps.iter().map(|step| step.id.as_str()).collect::<Vec<_>>(), ["neighbor-before",STEP,"neighbor-after"]);
                assert_eq!(parent.plan().steps[0].status, PlanStepStatus::Completed);
                assert_eq!(parent.plan().steps[2].content, "Preserve the following parent neighbor.");
                assert_not_dispatched(&child, "call_abandoned_before_edit");
                assert_not_dispatched(&child, "call_abandoned_before_stop");

                let summary = expect_tool(&parent_server, "call_parent_review_binding", "todo_write", json!({"todos":[]}));
                let mut review_barrier = hold_tool(&parent_server, "call_parent_before_review", "todo_read", json!({"target":"self","limit":50}));
                after_wait.release();
                let review_body = blocked(&mut review_barrier, &parent_server, &client).await;
                after_wait.assert_satisfied(); summary.assert_satisfied();
                let parent = journal(&grow_home, parent_id);
                let delivered_read = read_output(&review_body, "call_parent_after_wait", &parent, "self", false);
                assert_eq!(serde_json::from_str::<TimelineRangeRef>(delivered_read.plan.as_ref().unwrap().links[0].result_id.as_deref().unwrap()).unwrap(), reference);
                let supplied = tool_output(&review_body, "call_parent_review_binding").lines().find_map(|line| line.strip_prefix("    review binding: ")).expect("native parent tool supplies the exact actionable binding");
                let binding: Value = serde_json::from_str(supplied).unwrap();
                assert_eq!(binding["linkId"], task_id);
                assert_eq!(binding["resultId"], parent.plan().links[0].result_id.as_deref().unwrap());
                assert_eq!(binding["reviewedSourceRevision"], parent.plan().revision);
                let mut review = binding.clone();
                review["decision"] = json!("accepted");
                review["reason"] = json!("Verified exact canonical Result and normal stop settlement; preserved completed investigation and explicitly reported unperformed work.");
                review["verificationRefs"] = json!([parent.plan().links[0].result_id.as_ref().unwrap(), edit.id, stop.id]);
                let acceptance = expect_tool(&parent_server, "call_parent_accept_result", "todo_write", json!({"todos":[],"actions":[{"review":{"review":review}}]}));
                let final_reply = parent_server.expect_response("parent-normal-end", InferenceRequestMatcher::foreground(InferenceEndpoint::ChatCompletions), completion_response(PARENT_REPORT));
                review_barrier.release();
                // Satisfaction here is response emission, not terminal authority;
                // the enclosing real ACP prompt must still return EndTurn.
                let mut final_reply = final_reply;
                tokio::time::timeout(scaled(Duration::from_secs(30)), final_reply.wait_satisfied()).await.unwrap();
                review_barrier.assert_satisfied(); acceptance.assert_satisfied(); final_reply.assert_satisfied();
                (actual_spawn, edit, stop, edit_control.message, stop_control.message, binding)
            }
        );
        assert_eq!(reply.expect("bounded real parent prompt").unwrap().stop_reason, acp::StopReason::EndTurn);
        let parent = journal(&grow_home, parent_id);
        let child = journal(&grow_home, &actual_spawn.child_session_id);
        assert_result(&parent, &child, &actual_spawn, &stop);
        assert_eq!(parent.plan().reviews.len(), 1);
        let review = &parent.plan().reviews[0];
        assert_eq!(review.decision, ReviewDecision::Accepted);
        assert_eq!(review.link_id, binding["linkId"].as_str().unwrap());
        assert_eq!(review.result_id.as_deref(), binding["resultId"].as_str());
        assert_eq!(review.reviewed_source_revision, binding["reviewedSourceRevision"].as_u64().unwrap());
        assert_eq!(review.verification_refs.len(), 3);
        assert!(parent.plan().unresolved_required_deliveries().is_empty());
        let live_parent_plan = parent.plan().clone();
        let live_child_plan = child.plan().clone();
        let edit_control = control_evidence(&child, parent_id, &edit);
        let stop_control = control_evidence(&child, parent_id, &stop);
        assert_eq!(edit_control.message, edit_message);
        assert_eq!(stop_control.message, stop_message);
        for notification in client.grow_notifications() {
            let parsed: SessionNotification = serde_json::from_value(notification.clone()).unwrap();
            assert_no_control_notice(&parsed, &[&edit_control, &stop_control]);
            assert_visible(&notification);
        }
        assert_visible(&json!(client.captured_text()));
        assert!(foreground(&parent_server).iter().all(|request| request.body.as_ref().unwrap()["model"] == "parent-wire"));
        assert!(foreground(&child_server).iter().all(|request| request.body.as_ref().unwrap()["model"] == "child-wire"));
        tokio::time::timeout(scaled(Duration::from_secs(30)), client.ext_method("grow/session/close", json!({"sessionId":parent_id}))).await.unwrap().unwrap();
        // The session writer closes through the normal host command above.
        // TestProcess::close then terminates/reaps the transport process; its
        // platform signal exit status is not the child's canonical outcome.
        let transport_exit = tokio::time::timeout(scaled(Duration::from_secs(10)), client.close()).await.unwrap().unwrap();
        assert!(client.child_pid().is_none(), "test-owned writer process must be reaped");
        eprintln!("test-owned transport reaped: {transport_exit}");
        // All cold checks run only after normal close, with no writer or child.
        let cold_parent = journal(&grow_home, parent_id);
        let cold_child = journal(&grow_home, &actual_spawn.child_session_id);
        assert_eq!(cold_parent.plan(), &live_parent_plan);
        assert_eq!(cold_child.plan(), &live_child_plan);
        assert_result(&cold_parent, &cold_child, &actual_spawn, &stop);
        let cold_edit = control_evidence(&cold_child, parent_id, &edit);
        let cold_stop = control_evidence(&cold_child, parent_id, &stop);
        assert_eq!(cold_edit.message, edit_message);
        assert_eq!(cold_stop.message, stop_message);
        assert_persistent_projection(&grow_home, &cold_parent, &[&cold_edit, &cold_stop]);
        assert_persistent_projection(&grow_home, &cold_child, &[&cold_edit, &cold_stop]);
        assert_eq!(cold_child.timeline.parent_message_receipts().len(), 2, "two hidden controls retain their exact internal receipt facts");
        assert!(cold_child.timeline.pending_notifications().is_empty());
        assert!(cold_parent.plan().link_stop_requested(&actual_spawn.subagent_id));
        assert!(cold_parent.plan().steps.iter().find(|step| step.id == STEP).unwrap().removed);
        assert_eq!(cold_parent.plan().replans.len(), 2);
        for replan in &cold_parent.plan().replans {
            assert!(matches!(replan.progress, ReplanProgress::Handled { .. }));
        }
        assert_not_dispatched(&cold_child, "call_abandoned_before_edit");
        assert_not_dispatched(&cold_child, "call_abandoned_before_stop");
        eprintln!("cold milestone evidence={}", json!({"parent_plan":cold_parent.plan(),"child_plan":cold_child.plan(),"parent_review":cold_parent.plan().reviews,"parent_ended_reference_verified":true,"typed_controls_retained":2,"normal_writer_reaped":true}));
    }).await;
}
