//! Decoding of the shell's `grow/*` extension notifications into the headless
//! [`ExtEvent`] the orchestrator dispatches. Owns the wire envelope shapes and
//! the method to event mapping, kept out of `headless.rs`.

use agent_client_protocol as acp;

use crate::headless::reducer::{Lifecycle, StreamEvent};

/// Tolerate a numeric `task_id` (version skew) by coercing it to a string, so a
/// numeric id does not fail the decode and leak an untracked background task.
fn de_task_id<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize;
    match serde_json::Value::deserialize(deserializer)? {
        serde_json::Value::String(s) => Ok(s),
        serde_json::Value::Number(n) => Ok(n.to_string()),
        other => Err(serde::de::Error::custom(format!(
            "task_id must be a JSON string or number, got {other}"
        ))),
    }
}

fn session_update_tag(params: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(params)
        .ok()?
        .get("update")?
        .get("sessionUpdate")?
        .as_str()
        .map(str::to_string)
}

pub(crate) enum ExtEvent {
    None,
    TaskBackgrounded { task_id: String, is_monitor: bool },
    TaskCompleted { task_id: String },
    SubagentSpawned { subagent_id: String },
    SubagentFinished { subagent_id: String },
    MonitorEvent,
    Lifecycle(Lifecycle),
    Stream(Box<StreamEvent>),
}

pub(crate) fn handle_ext_notification(
    notif: &acp_transport::AcpArgsBox<acp::ExtNotification>,
) -> ExtEvent {
    let method = notif.request.method.as_ref();
    let params = notif.request.params.get();
    match method {
        "grow/task_backgrounded" => decode_task_backgrounded(method, params),
        "grow/task_completed" => decode_task_completed(method, params),
        "grow/monitor_event" => ExtEvent::MonitorEvent,
        "grow/session_notification" | "grow/session/update" => {
            decode_session_notification(method, params)
        }
        _ => ExtEvent::None,
    }
}

fn decode_task_backgrounded(method: &str, params: &str) -> ExtEvent {
    #[derive(serde::Deserialize)]
    struct TaskBgEnvelope {
        update: TaskBgUpdate,
    }
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "snake_case", tag = "sessionUpdate")]
    enum TaskBgUpdate {
        TaskBackgrounded {
            #[serde(deserialize_with = "de_task_id")]
            task_id: String,
            #[serde(default)]
            monitor_description: Option<String>,
        },
        #[serde(other)]
        Other,
    }
    match serde_json::from_str::<TaskBgEnvelope>(params) {
        Ok(env) => match env.update {
            TaskBgUpdate::TaskBackgrounded {
                task_id,
                monitor_description,
            } => ExtEvent::TaskBackgrounded {
                task_id,
                is_monitor: monitor_description.is_some(),
            },
            // Known-tag-on-wrong-carrier: log loudly instead of silently dropping.
            TaskBgUpdate::Other => {
                tracing::error!(
                    method,
                    payload = params,
                    "headless: x.ai/task_backgrounded with mismatched sessionUpdate \
                     tag; background task will not be tracked for reaping"
                );
                ExtEvent::None
            }
        },
        Err(e) => {
            tracing::error!(
                method,
                error = %e,
                payload = params,
                "headless: undecodable x.ai/task_backgrounded notification; \
                 background task will not be tracked for reaping"
            );
            ExtEvent::None
        }
    }
}

fn decode_task_completed(method: &str, params: &str) -> ExtEvent {
    #[derive(serde::Deserialize)]
    struct TaskDoneEnvelope {
        update: TaskDoneUpdate,
    }
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "snake_case", tag = "sessionUpdate")]
    enum TaskDoneUpdate {
        TaskCompleted {
            task_snapshot: TaskSnapshotLite,
        },
        #[serde(other)]
        Other,
    }
    #[derive(serde::Deserialize)]
    struct TaskSnapshotLite {
        #[serde(deserialize_with = "de_task_id")]
        task_id: String,
    }
    match serde_json::from_str::<TaskDoneEnvelope>(params) {
        Ok(env) => match env.update {
            TaskDoneUpdate::TaskCompleted { task_snapshot } => ExtEvent::TaskCompleted {
                task_id: task_snapshot.task_id,
            },
            // Known-tag-on-wrong-carrier: log loudly instead of silently dropping.
            TaskDoneUpdate::Other => {
                tracing::error!(
                    method,
                    payload = params,
                    "headless: x.ai/task_completed with mismatched sessionUpdate \
                     tag; background task completion will not be recorded"
                );
                ExtEvent::None
            }
        },
        Err(e) => {
            tracing::error!(
                method,
                error = %e,
                payload = params,
                "headless: undecodable x.ai/task_completed notification; \
                 background task completion will not be recorded"
            );
            ExtEvent::None
        }
    }
}

fn decode_session_notification(method: &str, params: &str) -> ExtEvent {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "snake_case", tag = "sessionUpdate")]
    enum GrowUpdate {
        AutoCompactStarted {
            percentage: u8,
        },
        AutoCompactCompleted {
            tokens_before: u64,
        },
        AutoCompactFailed {
            error: String,
        },
        AutoCompactCancelled {},
        ImageCompressed {
            message: String,
        },
        SubagentSpawned {
            subagent_id: String,
        },
        SubagentFinished {
            subagent_id: String,
        },
        ResponseStarted {
            #[serde(default)]
            message_id: Option<String>,
            #[serde(default)]
            model: Option<String>,
            #[serde(default)]
            input_tokens: u64,
            #[serde(default)]
            cache_read_input_tokens: u64,
            #[serde(default)]
            cache_creation_input_tokens: u64,
        },
        ReasoningCompleted {
            #[serde(default)]
            signature: Option<String>,
        },
        ResponseCompleted {
            #[serde(default)]
            message_id: Option<String>,
            #[serde(default)]
            stop_reason: Option<String>,
            #[serde(default)]
            usage: Option<shell::extensions::notification::ResponseUsage>,
            #[serde(default)]
            signature: Option<String>,
            #[serde(default)]
            stop_sequence: Option<String>,
        },
        #[serde(other)]
        Other,
    }
    #[derive(serde::Deserialize)]
    struct GrowNotif {
        update: GrowUpdate,
    }

    let grow_notif = match serde_json::from_str::<GrowNotif>(params) {
        Ok(n) => n,
        Err(e) => {
            tracing::warn!(
                method,
                error = %e,
                "headless: malformed x.ai session notification; ignoring"
            );
            return ExtEvent::None;
        }
    };

    match grow_notif.update {
        GrowUpdate::AutoCompactStarted { percentage } => {
            ExtEvent::Lifecycle(Lifecycle::CompactStarted { percentage })
        }
        GrowUpdate::AutoCompactCompleted { tokens_before } => {
            ExtEvent::Lifecycle(Lifecycle::CompactCompleted {
                pre_tokens: tokens_before,
            })
        }
        GrowUpdate::AutoCompactFailed { error } => {
            ExtEvent::Lifecycle(Lifecycle::CompactFailed { error })
        }
        GrowUpdate::AutoCompactCancelled {} => ExtEvent::Lifecycle(Lifecycle::CompactCancelled),
        GrowUpdate::ImageCompressed { message } => {
            ExtEvent::Lifecycle(Lifecycle::ImageCompressed { message })
        }
        GrowUpdate::SubagentSpawned { subagent_id } => ExtEvent::SubagentSpawned { subagent_id },
        GrowUpdate::SubagentFinished { subagent_id, .. } => {
            ExtEvent::SubagentFinished { subagent_id }
        }
        GrowUpdate::ResponseStarted {
            message_id,
            model,
            input_tokens,
            cache_read_input_tokens,
            cache_creation_input_tokens,
        } => ExtEvent::Stream(Box::new(StreamEvent::ResponseStarted {
            message_id,
            model,
            input_tokens,
            cache_read_input_tokens,
            cache_creation_input_tokens,
        })),
        GrowUpdate::ReasoningCompleted { signature } => {
            ExtEvent::Stream(Box::new(StreamEvent::ReasoningCompleted { signature }))
        }
        GrowUpdate::ResponseCompleted {
            message_id,
            stop_reason,
            usage,
            signature,
            stop_sequence,
        } => ExtEvent::Stream(Box::new(StreamEvent::ResponseCompleted {
            message_id,
            stop_reason,
            usage,
            signature,
            stop_sequence,
        })),
        // Background lifecycle tag on the wrong carrier: log loudly, but a
        // genuinely unknown display tag stays a clean ignore.
        GrowUpdate::Other => {
            if let Some(tag) = session_update_tag(params)
                && matches!(tag.as_str(), "task_backgrounded" | "task_completed")
            {
                tracing::error!(
                    method,
                    tag,
                    payload = params,
                    "headless: background-task lifecycle tag on a session notification \
                     (expected the dedicated x.ai/task_backgrounded|task_completed method); \
                     background tracking will not be updated"
                );
            }
            ExtEvent::None
        }
    }
}

#[cfg(test)]
#[path = "ext_protocol_tests.rs"]
mod tests;
