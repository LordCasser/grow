//! Append-only conversation timeline and its model-visible surface projection.
//!
//! The timeline owns durable conversation facts. A replacement appends a new
//! event which shadows current surface nodes; it never mutates or removes an
//! accepted event.

use sampling_types::ConversationItem;
use serde::{Deserialize, Serialize};

pub const TIMELINE_SCHEMA_VERSION: u8 = 1;

/// Monotonic identity of an event inside one timeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EventSeq(u64);

impl EventSeq {
    pub fn get(self) -> u64 {
        self.0
    }
}

/// Identity of one message contributed by a timeline event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct SurfaceId {
    pub event: EventSeq,
    pub item: u32,
}

/// Why a message event was produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageCause {
    Seed,
    User,
    Assistant,
    ToolResult,
    WorkingDirectory,
    IntegrityRepair,
    Compaction,
    ToolResultPrune,
    ImageRewrite,
    SystemPrompt,
    MemoryContext,
    SessionRestore,
    Rewind,
}

/// How a message event contributes to the current model-visible surface.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum SurfaceOp {
    Append,
    Replace {
        start: SurfaceId,
        end: SurfaceId,
        /// Exact surface nodes hidden by this replacement.
        shadowed: Vec<SurfaceId>,
    },
}

/// Message payload accepted as one immutable timeline event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageEvent {
    pub cause: MessageCause,
    pub items: Vec<ConversationItem>,
    pub surface: SurfaceOp,
}

/// Current timeline vocabulary. Sampling deltas intentionally do not appear
/// here: only their assembled message is a durable conversation fact.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TimelineEventKind {
    Messages(MessageEvent),
}

/// One accepted immutable timeline event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineEvent {
    pub version: u8,
    pub seq: EventSeq,
    #[serde(flatten)]
    pub kind: TimelineEventKind,
}

impl TimelineEvent {
    pub fn messages(&self) -> &MessageEvent {
        match &self.kind {
            TimelineEventKind::Messages(event) => event,
        }
    }
}

/// Append-only event log plus its incrementally maintained surface fold.
#[derive(Debug, Clone, Default)]
pub struct Timeline {
    events: Vec<TimelineEvent>,
    surface: Vec<ConversationItem>,
    surface_ids: Vec<SurfaceId>,
    replace_generation: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum TimelineError {
    #[error("unsupported timeline schema version {actual}; expected {expected}")]
    UnsupportedVersion { expected: u8, actual: u8 },
    #[error("timeline event seq {actual} is not the expected contiguous seq {expected}")]
    NonContiguousSeq { expected: u64, actual: u64 },
    #[error("append message event must contain at least one item")]
    EmptyAppend,
    #[error("replacement boundary is not present on the current surface")]
    StaleReplacementBoundary,
    #[error("replacement start occurs after its end")]
    ReversedReplacement,
    #[error("replacement shadow set does not exactly cover the selected surface range")]
    IncompleteShadowSet,
    #[error("surface item count exceeds u32 identity capacity")]
    TooManyItems,
    #[error("tool-result prune must replace exactly one tool result")]
    InvalidToolResultPrune,
    #[error("tool-result prune changed fields other than content")]
    ToolResultIdentityChanged,
}

impl Timeline {
    /// Construct a timeline by validating and replaying persisted events.
    pub fn from_events(events: Vec<TimelineEvent>) -> Result<Self, TimelineError> {
        let mut timeline = Self::default();
        for event in events {
            timeline.accept(event)?;
        }
        Ok(timeline)
    }

    /// Create seed append events from an already assembled message list.
    pub fn from_seed(items: Vec<ConversationItem>) -> Result<Self, TimelineError> {
        let mut timeline = Self::default();
        for item in items {
            timeline.append(item, MessageCause::Seed)?;
        }
        Ok(timeline)
    }

    pub fn events(&self) -> &[TimelineEvent] {
        &self.events
    }

    /// Sequence that will be assigned to the next accepted event.
    pub fn next_seq(&self) -> EventSeq {
        EventSeq(self.events.len() as u64)
    }

    pub fn replace_generation(&self) -> u64 {
        self.replace_generation
    }

    /// Current model-visible message projection.
    pub fn surface(&self) -> &[ConversationItem] {
        &self.surface
    }

    /// Append-origin conversation history. Replacement events never erase it.
    pub fn transcript(&self) -> Vec<ConversationItem> {
        self.events
            .iter()
            .filter_map(|event| {
                let messages = event.messages();
                matches!(messages.surface, SurfaceOp::Append).then_some(&messages.items)
            })
            .flatten()
            .cloned()
            .collect()
    }

    pub fn surface_len(&self) -> usize {
        self.surface.len()
    }

    pub fn surface_item(&self, index: usize) -> Option<&ConversationItem> {
        self.surface.get(index)
    }

    /// Turn-local transcript projected through identity-preserving rewrites.
    ///
    /// A same-cardinality replacement (for example image canonicalization)
    /// carries each captured node forward to the corresponding replacement
    /// node. Structural replacements such as compaction are not treated as new
    /// turn output; the original append facts remain in the capture.
    pub fn turn_items_since(&self, start: EventSeq) -> Vec<ConversationItem> {
        let mut captured = Vec::<(SurfaceId, ConversationItem)>::new();
        for event in self.events.iter().skip(start.get() as usize) {
            let messages = event.messages();
            match &messages.surface {
                SurfaceOp::Append => {
                    captured.extend(messages.items.iter().cloned().enumerate().map(
                        |(item, value)| {
                            (
                                SurfaceId {
                                    event: event.seq,
                                    item: item as u32,
                                },
                                value,
                            )
                        },
                    ));
                }
                SurfaceOp::Replace { shadowed, .. } if shadowed.len() == messages.items.len() => {
                    for (replacement_index, (shadowed_id, replacement)) in
                        shadowed.iter().zip(messages.items.iter()).enumerate()
                    {
                        if let Some((id, item)) =
                            captured.iter_mut().find(|(id, _)| id == shadowed_id)
                        {
                            *id = SurfaceId {
                                event: event.seq,
                                item: replacement_index as u32,
                            };
                            *item = replacement.clone();
                        }
                    }
                }
                SurfaceOp::Replace { .. } => {}
            }
        }
        captured.into_iter().map(|(_, item)| item).collect()
    }

    /// Append one ordinary conversation item and return the accepted event.
    pub fn append(
        &mut self,
        item: ConversationItem,
        cause: MessageCause,
    ) -> Result<TimelineEvent, TimelineError> {
        self.append_many(vec![item], cause)
    }

    pub fn append_many(
        &mut self,
        items: Vec<ConversationItem>,
        cause: MessageCause,
    ) -> Result<TimelineEvent, TimelineError> {
        let event = TimelineEvent {
            version: TIMELINE_SCHEMA_VERSION,
            seq: EventSeq(self.events.len() as u64),
            kind: TimelineEventKind::Messages(MessageEvent {
                cause,
                items,
                surface: SurfaceOp::Append,
            }),
        };
        self.accept(event)?;
        Ok(self
            .events
            .last()
            .expect("accepted event must be stored")
            .clone())
    }

    /// Replace the complete current surface while retaining all prior events.
    pub fn replace_all(
        &mut self,
        items: Vec<ConversationItem>,
        cause: MessageCause,
    ) -> Result<TimelineEvent, TimelineError> {
        if self.surface.is_empty() {
            return self.append_many(items, cause);
        }
        self.replace_range(0, self.surface.len() - 1, items, cause)
    }

    /// Replace one inclusive current-surface range.
    pub fn replace_range(
        &mut self,
        start_index: usize,
        end_index: usize,
        items: Vec<ConversationItem>,
        cause: MessageCause,
    ) -> Result<TimelineEvent, TimelineError> {
        let Some(start) = self.surface_ids.get(start_index).copied() else {
            return Err(TimelineError::StaleReplacementBoundary);
        };
        let Some(end) = self.surface_ids.get(end_index).copied() else {
            return Err(TimelineError::StaleReplacementBoundary);
        };
        if start_index > end_index {
            return Err(TimelineError::ReversedReplacement);
        }
        let shadowed = self.surface_ids[start_index..=end_index].to_vec();
        let event = TimelineEvent {
            version: TIMELINE_SCHEMA_VERSION,
            seq: EventSeq(self.events.len() as u64),
            kind: TimelineEventKind::Messages(MessageEvent {
                cause,
                items,
                surface: SurfaceOp::Replace {
                    start,
                    end,
                    shadowed,
                },
            }),
        };
        self.accept(event)?;
        Ok(self
            .events
            .last()
            .expect("accepted event must be stored")
            .clone())
    }

    /// Apply one persisted event after validating it against the current fold.
    pub fn accept(&mut self, event: TimelineEvent) -> Result<(), TimelineError> {
        if event.version != TIMELINE_SCHEMA_VERSION {
            return Err(TimelineError::UnsupportedVersion {
                expected: TIMELINE_SCHEMA_VERSION,
                actual: event.version,
            });
        }
        let expected = self.events.len() as u64;
        if event.seq.get() != expected {
            return Err(TimelineError::NonContiguousSeq {
                expected,
                actual: event.seq.get(),
            });
        }
        let messages = event.messages();
        let item_count =
            u32::try_from(messages.items.len()).map_err(|_| TimelineError::TooManyItems)?;
        match &messages.surface {
            SurfaceOp::Append => {
                if messages.items.is_empty() {
                    return Err(TimelineError::EmptyAppend);
                }
                self.surface.extend(messages.items.iter().cloned());
                self.surface_ids
                    .extend((0..item_count).map(|item| SurfaceId {
                        event: event.seq,
                        item,
                    }));
            }
            SurfaceOp::Replace {
                start,
                end,
                shadowed,
            } => {
                let Some(start_index) = self.surface_ids.iter().position(|id| id == start) else {
                    return Err(TimelineError::StaleReplacementBoundary);
                };
                let Some(end_index) = self.surface_ids.iter().position(|id| id == end) else {
                    return Err(TimelineError::StaleReplacementBoundary);
                };
                if start_index > end_index {
                    return Err(TimelineError::ReversedReplacement);
                }
                let actual = self.surface_ids[start_index..=end_index].to_vec();
                if &actual != shadowed {
                    return Err(TimelineError::IncompleteShadowSet);
                }
                if messages.cause == MessageCause::ToolResultPrune {
                    validate_tool_result_prune(&self.surface[start_index..=end_index], messages)?;
                }
                self.surface
                    .splice(start_index..=end_index, messages.items.iter().cloned());
                self.surface_ids.splice(
                    start_index..=end_index,
                    (0..item_count).map(|item| SurfaceId {
                        event: event.seq,
                        item,
                    }),
                );
                self.replace_generation = self.replace_generation.saturating_add(1);
            }
        }
        self.events.push(event);
        Ok(())
    }
}

fn validate_tool_result_prune(
    replaced: &[ConversationItem],
    replacement: &MessageEvent,
) -> Result<(), TimelineError> {
    if replaced.len() != 1 || replacement.items.len() != 1 {
        return Err(TimelineError::InvalidToolResultPrune);
    }
    let ConversationItem::ToolResult(before) = &replaced[0] else {
        return Err(TimelineError::InvalidToolResultPrune);
    };
    let ConversationItem::ToolResult(after) = &replacement.items[0] else {
        return Err(TimelineError::InvalidToolResultPrune);
    };
    let before_images =
        serde_json::to_value(&before.images).expect("conversation images serialize");
    let after_images = serde_json::to_value(&after.images).expect("conversation images serialize");
    if before.tool_call_id != after.tool_call_id || before_images != after_images {
        return Err(TimelineError::ToolResultIdentityChanged);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn json(items: &[ConversationItem]) -> serde_json::Value {
        serde_json::to_value(items).unwrap()
    }

    #[test]
    fn append_assigns_contiguous_identity_and_projects_messages() {
        let mut timeline = Timeline::default();
        timeline
            .append(ConversationItem::user("one"), MessageCause::User)
            .unwrap();
        timeline
            .append(ConversationItem::assistant("two"), MessageCause::Assistant)
            .unwrap();

        assert_eq!(timeline.events()[0].seq.get(), 0);
        assert_eq!(timeline.events()[1].seq.get(), 1);
        assert_eq!(json(timeline.surface()), json(&timeline.transcript()));
    }

    #[test]
    fn replacement_changes_surface_without_erasing_transcript_or_events() {
        let mut timeline = Timeline::from_seed(vec![
            ConversationItem::system("system"),
            ConversationItem::user("old question"),
            ConversationItem::assistant("old answer"),
        ])
        .unwrap();
        timeline
            .replace_range(
                1,
                2,
                vec![ConversationItem::user("summary")],
                MessageCause::Compaction,
            )
            .unwrap();

        assert_eq!(timeline.events().len(), 4);
        assert_eq!(timeline.surface_len(), 2);
        assert_eq!(timeline.transcript().len(), 3);
        assert_eq!(timeline.replace_generation(), 1);
        assert_eq!(timeline.surface()[1].text_content(), "summary");
    }

    #[test]
    fn persisted_events_replay_to_the_same_surface() {
        let mut timeline = Timeline::from_seed(vec![
            ConversationItem::user("question"),
            ConversationItem::tool_result("call", "long result"),
        ])
        .unwrap();
        timeline
            .replace_range(
                1,
                1,
                vec![ConversationItem::tool_result("call", "short")],
                MessageCause::ToolResultPrune,
            )
            .unwrap();

        let encoded = serde_json::to_string(timeline.events()).unwrap();
        let events = serde_json::from_str(&encoded).unwrap();
        let replayed = Timeline::from_events(events).unwrap();
        assert_eq!(json(timeline.surface()), json(replayed.surface()));
        assert_eq!(json(&timeline.transcript()), json(&replayed.transcript()));
    }

    #[test]
    fn load_rejects_non_contiguous_sequence() {
        let mut timeline = Timeline::from_seed(vec![ConversationItem::user("one")]).unwrap();
        let mut events = timeline.events.drain(..).collect::<Vec<_>>();
        events[0].seq = EventSeq(4);
        assert!(matches!(
            Timeline::from_events(events),
            Err(TimelineError::NonContiguousSeq { .. })
        ));
    }

    #[test]
    fn load_rejects_unknown_schema_version() {
        let timeline = Timeline::from_seed(vec![ConversationItem::user("one")]).unwrap();
        let mut events = timeline.events().to_vec();
        events[0].version = TIMELINE_SCHEMA_VERSION + 1;
        assert!(matches!(
            Timeline::from_events(events),
            Err(TimelineError::UnsupportedVersion { .. })
        ));
    }

    #[test]
    fn prune_cannot_change_tool_result_identity() {
        let mut timeline = Timeline::from_seed(vec![ConversationItem::tool_result(
            "original",
            "long result",
        )])
        .unwrap();
        let error = timeline
            .replace_range(
                0,
                0,
                vec![ConversationItem::tool_result("forged", "short")],
                MessageCause::ToolResultPrune,
            )
            .unwrap_err();
        assert!(matches!(error, TimelineError::ToolResultIdentityChanged));
        assert_eq!(timeline.events().len(), 1);
        assert_eq!(timeline.surface()[0].text_content(), "long result");
    }
}
