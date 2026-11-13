//! Final ConversationRequest assembly and context-pressure projection.

use sampling_types::{
    ContentPart, ConversationItem, ConversationRequest, GoalDirectiveTag, JsonOutputFormat,
    ToolSpec,
};

use super::ChatStateActor;
use super::state::ImageBudgetSelection;
use crate::MessageCause;
use crate::events::ChatStateEvent;

impl ChatStateActor {
    /// Build a `ConversationRequest` from the current actor state.
    ///
    /// 1. Repair Surface and append retrieved memory context exactly once
    /// 2. Apply request-only Goal, runtime context and target-model image projections
    /// 3. Evict oldest projected images when the request nears 50 MB
    /// 4. Assemble the final request and project its complete input envelope
    ///
    /// # Repair invariant
    ///
    /// This method reuses the healthy owned Surface copy, or waits for the
    /// actor's exact malformed-identity/pairing replacement to commit before
    /// using a copy of it. Never repair only the request: the replacement must
    /// be acknowledged in the Timeline.
    pub(super) async fn build_conversation_request(
        &mut self,
        timeline_id: &str,
        tool_definitions: Vec<ToolSpec>,
        memory_reminder: Option<String>,
        runtime_context: Option<String>,
        active_goal: Option<GoalDirectiveTag>,
        json_output: Option<JsonOutputFormat>,
        use_image_descriptions: bool,
    ) -> Result<ConversationRequest, crate::commands::TimelineWriteError> {
        let mut items = self
            .ensure_conversation_integrity_durably(
                sampling_types::DanglingToolCallReason::UserCancelled,
            )
            .await?
            .unwrap_or_else(|| self.state.timeline.surface().to_vec());
        if let Some(reminder) = memory_reminder
            .as_deref()
            .map(str::trim)
            .filter(|reminder| !reminder.is_empty())
        {
            let item = ConversationItem::memory_context(reminder);
            let already_present = self.state.timeline.surface().iter().any(|existing| {
                matches!(
                    existing,
                    ConversationItem::User(user)
                        if user.synthetic_reason
                            == Some(sampling_types::SyntheticReason::MemoryContext)
                            && existing.text_content() == item.text_content()
                )
            });
            if !already_present {
                let event = self
                    .state
                    .timeline
                    .prepare(crate::TimelineEventKind::Messages(crate::MessageEvent {
                        cause: MessageCause::MemoryContext,
                        items: vec![item.clone()],
                        surface: crate::SurfaceOp::Append,
                        response_admission: None,
                    }))?;
                self.commit_timeline_event(event).await?;
                items.push(item);
            }
        }
        items = sampling_types::project_conversation_for_goal_scope(items, active_goal.as_ref());
        if use_image_descriptions {
            sampling_types::conversation::select_image_descriptions(&mut items).map_err(
                |error| crate::TimelineWriteError::ImageDescriptionUnavailable(error.into()),
            )?;
        }

        let source_items_len = items.len();
        let runtime_context = runtime_context
            .as_deref()
            .map(str::trim)
            .filter(|context| !context.is_empty())
            .map(ConversationItem::system_reminder);
        let runtime_context_tokens = runtime_context
            .as_ref()
            .map_or(0, super::state::estimate_item_tokens);
        let runtime_context_index = runtime_context.map(|context| {
            items.push(context);
            source_items_len
        });

        let surface_ids = self.state.timeline.surface_ids().to_vec();
        let route = sampling_types::model_image_input_key(&self.state.sampling_config);
        let same_domain = self.state.image_budget.as_ref().is_some_and(|previous| {
            previous.route == route
                && self.state.request_active_goal == active_goal
                && previous.use_image_descriptions == use_image_descriptions
                && surface_ids.starts_with(&previous.surface_ids)
        });
        let mut selection = if same_domain {
            self.state.image_budget.as_ref().unwrap().clone()
        } else {
            ImageBudgetSelection {
                route,
                use_image_descriptions,
                surface_ids: Vec::new(),
                replaced: Default::default(),
            }
        };
        let inline_images = inline_image_count(&items);
        // A SurfaceId is stable across append. Check the exact source part as
        // well, so a replacement under an old coordinate cannot inherit it.
        let valid_choices = image_budget_choices_are_valid(&items, &surface_ids, &selection);
        if !valid_choices {
            selection.replaced.clear();
        }
        apply_image_budget_choices(&mut items, &surface_ids, &selection);

        // Measure the effective request after replaying previous choices and
        // evict as it approaches
        // the 50 MB ceiling. The transport checks the final wire body after
        // encoding. This estimate skips the multi-MB base64 escape scan, so
        // it runs inline on every turn with no blocking-thread offload.
        // Eviction rewrites earlier turns. The low-water mark buys headroom
        // only when these choices are reused on later requests.
        let body_bytes = conversation_body_bytes(&items);
        let needs_image_compaction = body_bytes >= IMAGE_COMPACT_TRIGGER_BYTES;

        let mut eviction: Option<ImageEvictionOutcome> = None;
        if needs_image_compaction {
            // Step 1: When the body nears the 50 MB ceiling, evict oldest
            // images down to the low-water mark (not just under the trigger).
            // Reclaiming a batch frees headroom for later image turns.
            eviction = Some(compact_images_to_byte_budget(
                &mut items,
                body_bytes,
                IMAGE_COMPACT_RECLAIM_TARGET_BYTES,
            ));
            if let Some(outcome) = &eviction {
                for ((index, part_index), identity) in outcome
                    .evicted_parts
                    .iter()
                    .zip(&outcome.evicted_identities)
                {
                    selection
                        .replaced
                        .insert((*index, *part_index), identity.clone());
                }
            }
        }
        selection.surface_ids = surface_ids.clone();
        let effective_evicted_parts: Vec<_> = selection.replaced.keys().copied().collect();

        // Per-turn image-budget record for local verification. Emitted on the
        // ChatState event channel (chat-state can't reach the shell's unified
        // log directly); the session consumer writes it to the local log file.
        // Only on image-bearing turns to avoid noise.
        if inline_images > 0 || !effective_evicted_parts.is_empty() {
            self.send_event(ChatStateEvent::ImageBudget {
                body_bytes,
                trigger_bytes: IMAGE_COMPACT_TRIGGER_BYTES,
                reclaim_target_bytes: IMAGE_COMPACT_RECLAIM_TARGET_BYTES,
                inline_images,
                needs_image_compaction,
                evicted: eviction.as_ref().map_or(0, |o| o.evicted),
                effective_evicted: effective_evicted_parts.len(),
                body_bytes_after: eviction.as_ref().map_or(body_bytes, |o| o.body_bytes_after),
            });
        }

        // Runtime context has no Surface identity. Reconcile only the source
        // projection so its request-only tail cannot reset or consume a native span.
        let projection_is_aligned = self
            .state
            .continuation
            .reconcile_projection(&surface_ids, &items[..source_items_len]);
        let native_continuation = Some(if projection_is_aligned {
            self.state.continuation.request_projection(&surface_ids)
        } else {
            sampling_types::NativeContinuationProjection {
                portable_prefix_len: source_items_len,
                spans: Vec::new(),
                portable_reasoning_backend: self.state.continuation.portable_reasoning_backend(),
            }
        });
        let epoch_nonce = self.state.continuation.epoch_nonce().to_owned();

        // Step 3: Assemble request
        let request = ConversationRequest {
            source_projection: Some(serde_json::json!({
                "timeline_id": timeline_id,
                "source_seq": self.state.timeline.last_seq().map(|seq| seq.get()),
                "surface_ids": surface_ids,
                "runtime_context": runtime_context_index.map(|item_index| serde_json::json!({
                    "item_index": item_index,
                    "estimated_tokens": runtime_context_tokens,
                })),
                "active_goal": active_goal,
                "image_budget": {
                    "before_bytes": body_bytes,
                    "trigger_bytes": IMAGE_COMPACT_TRIGGER_BYTES,
                    "target_bytes": IMAGE_COMPACT_RECLAIM_TARGET_BYTES,
                    "evicted_parts": effective_evicted_parts,
                    "newly_evicted_parts": eviction.as_ref().map(|value| &value.evicted_parts),
                    "after_bytes": eviction.as_ref().map_or(body_bytes, |value| value.body_bytes_after),
                },
                "continuation_epoch": epoch_nonce,
                "portable_prefix_len": native_continuation.as_ref().and_then(|value| value.portable_prefix_end(&items)),
                "portable_reasoning_backend": native_continuation.as_ref()
                    .and_then(|value| value.portable_reasoning_backend.as_ref()),
                "native_spans": native_continuation.as_ref().map(|value| value.spans.iter()
                    .map(|span| [span.start, span.end]).collect::<Vec<_>>()),
            })),
            items,
            tools: tool_definitions,
            tool_choice: None,
            model: Some(self.state.sampling_config.model.clone()),
            temperature: self.state.sampling_config.temperature,
            max_output_tokens: self.state.sampling_config.output_limit,
            top_p: self.state.sampling_config.top_p,
            prompt_cache_key: Some(prompt_cache_key(
                timeline_id,
                &self.state.timeline,
                &self.state.sampling_config,
                &epoch_nonce,
            )),
            reasoning_effort: self.state.sampling_config.reasoning_effort,
            json_output,
            native_continuation,
        };
        let request_input_tokens = super::state::estimate_request_input_tokens(&request);
        // Pressure refreshes rebuild from durable Surface. Keep the current
        // request-only context cost with the envelope, without retaining its text.
        self.state.request_envelope_tokens = request_input_tokens.saturating_sub(
            super::state::estimate_effective_conversation_tokens(&request)
                .saturating_sub(runtime_context_tokens),
        );
        self.state.request_active_goal = active_goal.clone();
        self.state.image_budget = Some(selection);
        self.apply_request_projection(request_input_tokens);
        Ok(request)
    }
}

fn image_budget_choices_are_valid(
    items: &[ConversationItem],
    surface_ids: &[crate::SurfaceId],
    selection: &ImageBudgetSelection,
) -> bool {
    selection
        .replaced
        .iter()
        .all(|((index, part_index), identity)| {
            selection
                .surface_ids
                .get(*index)
                .zip(surface_ids.get(*index))
                .filter(|(saved, current)| saved == current)
                .and_then(|_| items.get(*index))
                .and_then(|item| image_content(item).get(*part_index))
                .is_some_and(|part| image_part_matches(part, identity))
        })
}

/// Replay the same identity-bound image choices for request assembly and
/// context pressure; replacements do not inherit old image coordinates.
pub(super) fn apply_image_budget_choices(
    items: &mut [ConversationItem],
    surface_ids: &[crate::SurfaceId],
    selection: &ImageBudgetSelection,
) {
    for ((index, part_index), identity) in &selection.replaced {
        let source_matches = selection
            .surface_ids
            .get(*index)
            .zip(surface_ids.get(*index))
            .is_some_and(|(saved, current)| saved == current);
        if source_matches
            && let Some(item) = items.get_mut(*index)
            && let Some(part) = image_content_mut(item).get_mut(*part_index)
            && image_part_matches(part, identity)
        {
            *part = ContentPart::Text {
                text: IMAGE_COMPACT_PLACEHOLDER.into(),
            };
        }
    }
}

/// Build the sticky provider route from causal lineage and model identity.
///
/// Appending to one branch deliberately leaves the key unchanged. Rewind is a
/// branch operation, so the latest rewind event becomes the branch anchor;
/// forks already have a distinct `timeline_id`. Provider/backend/base URL are
/// included with the model name so equal display names cannot share a route.
fn prompt_cache_key(
    timeline_id: &str,
    timeline: &crate::Timeline,
    sampling: &sampling_types::SamplingConfig,
    epoch_nonce: &str,
) -> String {
    let branch_anchor = timeline
        .last_rewind_seq()
        .map_or_else(|| "root".to_owned(), |seq| seq.get().to_string());
    let backend = match sampling.api_backend {
        sampling_types::ApiBackend::ChatCompletions => "chat_completions",
        sampling_types::ApiBackend::Responses => "responses",
        sampling_types::ApiBackend::Messages => "messages",
    };
    let mut hasher = blake3::Hasher::new();
    for component in [
        "grow-prompt-cache-v1",
        timeline_id,
        branch_anchor.as_str(),
        backend,
        sampling.base_url.as_str(),
        sampling.model.as_str(),
        epoch_nonce,
    ] {
        hasher.update(component.as_bytes());
        hasher.update(&[0]);
    }
    let digest = hasher.finalize().to_hex();
    format!("grow-{}", &digest.as_str()[..32])
}

// ============================================================================
// Image size-gated compaction (request-copy only)
// ============================================================================

/// Replaces an inline image evicted to keep the request body under the proxy's
/// 50 MB limit. Phrased so the model treats the image as gone rather than
/// describing it from memory — a silently-stripped image otherwise induces
/// confident hallucination of its contents.
const IMAGE_COMPACT_PLACEHOLDER: &str = "[An earlier image was removed to keep the request within its size limit and is no longer visible. Do not describe or reason about its contents from memory; ask the user to re-share it if you need to see it again.]";

/// Hard request-body ceiling enforced by the inference proxy
/// (nginx `proxy-body-size`). Bodies larger than this are rejected with HTTP
/// 413 — or a connection reset before the response is written. Inline image
/// `data:` URLs (base64) are the dominant term in this size.
const MAX_REQUEST_BYTES: usize = sampling_types::MAX_REQUEST_BODY_BYTES;

/// Evict old images once the serialized body reaches this size.
///
/// We gate on the exact body (see [`conversation_body_bytes`]) — system prompt,
/// all message text, tool results, and image `data:` URLs are all counted
/// precisely. This sits 3 MB below [`MAX_REQUEST_BYTES`] as headroom for the
/// only parts of the wire request the body measurement does **not** include:
/// - **tool definitions** — sent alongside the conversation but not part of it
///   (tool JSON schemas + MCP tools); this is the bulk of the gap.
/// - the request envelope and sampling params.
/// - the small delta between our internal `ContentPart` JSON and the public-API
///   wire format (the dominant base64 image bytes are identical in both).
///
/// The uncounted remainder is only sub-MB to low-MB in practice, so 3 MB covers
/// it without needlessly sacrificing image capacity. Sampler rejects a final
/// encoded body above the ceiling before sending it; it never rewrites images.
///
/// Below this threshold every image stays in place so the KV-cache prefix is
/// byte-stable across turns; eviction rewrites earlier turns and busts the
/// prefix cache, so we only pay that cost when a 413 is actually near.
pub(crate) const IMAGE_COMPACT_TRIGGER_BYTES: usize = MAX_REQUEST_BYTES - 3 * 1024 * 1024;

/// Low-water mark that eviction reclaims down to once it fires (hysteresis).
///
/// Eviction is **gated** at [`IMAGE_COMPACT_TRIGGER_BYTES`] but **reclaims** to
/// this strictly lower mark. Evicting only enough to clear the trigger means
/// the next image-bearing turn re-crosses it and evicts again — rewriting the
/// prefix and busting the KV cache on essentially every turn once the body sits
/// at the ceiling. Dropping to half the hard limit instead frees ~25 MB of
/// headroom, so the prefix is rewritten once and then stays stable (cache-warm)
/// across many turns until the headroom is consumed again. The oldest images
/// (least useful) are sacrificed in a batch rather than one-per-turn — a
/// high-water trigger paired with a lower reclaim mark (classic hysteresis).
pub(crate) const IMAGE_COMPACT_RECLAIM_TARGET_BYTES: usize = MAX_REQUEST_BYTES / 2;

// Hysteresis invariant: eviction is gated at the trigger but reclaims to a
// strictly lower mark, so one batch eviction buys many cache-warm turns rather
// than re-triggering (and re-busting the prompt cache) every turn at the
// ceiling. Enforced at compile time so the two constants can't drift together.
const _: () = assert!(IMAGE_COMPACT_RECLAIM_TARGET_BYTES < IMAGE_COMPACT_TRIGGER_BYTES);

/// An [`std::io::Write`] sink that counts bytes instead of storing them. Lets
/// us measure a `serde_json` encoding's length without allocating the full
/// (potentially tens-of-MB) output buffer.
#[derive(Default)]
struct ByteCounter(usize);

impl std::io::Write for ByteCounter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0 += buf.len();
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Exact JSON-serialized byte length of any value, measured through a
/// [`ByteCounter`] so no encoded buffer is allocated. JSON quoting and string
/// escaping are captured precisely (not estimated from field lengths).
fn serialized_json_bytes<T: serde::Serialize + ?Sized>(value: &T) -> usize {
    let mut counter = ByteCounter::default();
    if let Err(err) = serde_json::to_writer(&mut counter, value) {
        // Serializing in-memory state to a byte sink is infallible in
        // practice; if it ever fails, fall back to the bytes counted so far
        // (a lower bound) rather than forcing a needless compaction.
        tracing::warn!(%err, "failed to measure serialized size");
    }
    counter.0
}

/// Serialized JSON frame of one image content part with an empty URL —
/// `{"type":"image","url":""}`. The real payload adds exactly `url.len()` on
/// top: an inline base64 `data:` URL contains no JSON-escaped characters, so
/// its encoded length equals its raw length. Identical in our internal JSON and
/// on the public-API wire (the base64 bytes are the same in both).
const IMAGE_PART_FRAME_BYTES: usize = r#"{"type":"image","url":""}"#.len();

/// Exact serialized size of a single inline image part (frame + raw URL bytes).
fn image_part_bytes(url: &str) -> usize {
    IMAGE_PART_FRAME_BYTES + url.len()
}

/// Both model-visible image carriers share the same budgeting policy.
fn image_content(item: &ConversationItem) -> &[ContentPart] {
    match item {
        ConversationItem::User(user) => &user.content,
        ConversationItem::ToolResult(result) => &result.images,
        _ => &[],
    }
}

fn image_content_mut(item: &mut ConversationItem) -> &mut [ContentPart] {
    match item {
        ConversationItem::User(user) => &mut user.content,
        ConversationItem::ToolResult(result) => &mut result.images,
        _ => &mut [],
    }
}

/// Count of inline images in the conversation — for observability only.
fn inline_image_count(conversation: &[ConversationItem]) -> usize {
    conversation
        .iter()
        .flat_map(image_content)
        .filter(|part| matches!(part, ContentPart::Image { .. }))
        .count()
}

/// Outcome of [`compact_images_to_byte_budget`], surfaced for logging and
/// local verification.
pub(crate) struct ImageEvictionOutcome {
    /// Number of inline images replaced with the placeholder.
    pub evicted: usize,
    /// Exact (Surface item index, content part index), before any wire lowering.
    pub evicted_parts: Vec<(usize, usize)>,
    evicted_identities: Vec<(std::sync::Weak<str>, Option<std::sync::Weak<str>>)>,
    /// Estimated serialized body size after eviction (`current_bytes` minus the
    /// net bytes freed) — at or below `target_bytes` once enough images go.
    pub body_bytes_after: usize,
}

fn image_part_identity(
    part: &ContentPart,
) -> Option<(std::sync::Weak<str>, Option<std::sync::Weak<str>>)> {
    let ContentPart::Image { url, description } = part else {
        return None;
    };
    Some((
        std::sync::Arc::downgrade(url),
        description.as_ref().map(std::sync::Arc::downgrade),
    ))
}

fn image_part_matches(
    part: &ContentPart,
    identity: &(std::sync::Weak<str>, Option<std::sync::Weak<str>>),
) -> bool {
    let Some((url, description)) = image_part_identity(part) else {
        return false;
    };
    std::sync::Weak::ptr_eq(&url, &identity.0)
        && match (&description, &identity.1) {
            (None, None) => true,
            (Some(a), Some(b)) => std::sync::Weak::ptr_eq(a, b),
            _ => false,
        }
}

/// Serialized size of the internal conversation body, leaving headroom for
/// provider framing. Computed **without** scanning the
/// multi-MB base64 image payloads.
///
/// `serde_json` escape-scans every byte of every string, so encoding the real
/// conversation would walk tens of MB of base64 on every turn. Instead we
/// serialize a copy with image URLs blanked (cheap: only the small non-image
/// content — system prompt, message text, tool results — is scanned, and it is
/// measured *exactly*, escaping included) and add back each URL's raw length.
/// Because base64 never escapes, that length is its exact serialized
/// contribution, so the result is byte-for-byte the true body size.
///
/// The blanking copy is cheap: image data lives behind `Arc<str>`, so cloning
/// only bumps refcounts and the blanked clone drops them without copying bytes.
fn conversation_body_bytes(conversation: &[ConversationItem]) -> usize {
    let mut blanked = conversation.to_vec();
    let mut image_url_bytes = 0usize;
    for item in &mut blanked {
        for part in image_content_mut(item) {
            if let ContentPart::Image { url, .. } = part {
                image_url_bytes += url.len();
                *url = std::sync::Arc::<str>::from("");
            }
        }
    }
    serialized_json_bytes(&blanked) + image_url_bytes
}

/// Replace the oldest inline images with [`IMAGE_COMPACT_PLACEHOLDER`] until
/// the serialized request body drops back to `target_bytes`, keeping the
/// newest images. `current_bytes` is the already-measured whole-body size (see
/// [`conversation_body_bytes`]); each eviction drops `running` by the image
/// part's exact serialized size minus the placeholder that replaces it, so it
/// tracks the true body byte-for-byte as images are removed.
///
/// Operates on a mutable slice — intended for the request *copy* so the stored
/// conversation is never modified.
///
/// ## Cache behavior
///
/// Eviction is **oldest-first**, which is sticky by construction: because we
/// always retain the newest images, an image only transitions image →
/// placeholder as *newer/larger* payloads push the body past the limit, never
/// placeholder → image within a stable prefix. (Token compaction removes old
/// turns wholesale and can free room to restore a previously-evicted image,
/// but that already rewrites the prefix and invalidates the server-side prompt
/// cache, so the restore is free.)
///
/// The caller gates eviction at [`IMAGE_COMPACT_TRIGGER_BYTES`] but passes the
/// lower [`IMAGE_COMPACT_RECLAIM_TARGET_BYTES`] as `target_bytes`, so one
/// eviction reclaims a batch of the oldest images and frees headroom for many
/// later image turns. This turns "rewrite the prefix on essentially every turn
/// once at the ceiling" into one larger, rare rewrite followed by a long
/// cache-warm stretch — the prefix-cache cost of dropping the oldest (least
/// useful) image is paid infrequently instead of per turn.
///
/// This replaces the previous policy — strip every image older than the most
/// recent user turn on *every* request — which (a) busted the prompt-cache
/// prefix on the turn after any image, and (b) dropped images the model still
/// needed one turn later, causing it to hallucinate their contents.
pub(crate) fn compact_images_to_byte_budget(
    conversation: &mut [ConversationItem],
    current_bytes: usize,
    target_bytes: usize,
) -> ImageEvictionOutcome {
    if current_bytes <= target_bytes {
        return ImageEvictionOutcome {
            evicted: 0,
            evicted_parts: Vec::new(),
            evicted_identities: Vec::new(),
            body_bytes_after: current_bytes,
        };
    }

    // The text part each evicted image is replaced with. Measured once: every
    // eviction shrinks the body by the image part's bytes and grows it back by
    // this placeholder's bytes, so the net saving is `image - placeholder`.
    let placeholder = ContentPart::Text {
        text: std::sync::Arc::<str>::from(IMAGE_COMPACT_PLACEHOLDER),
    };
    let placeholder_bytes = serialized_json_bytes(&placeholder);

    // (item_idx, part_idx, exact serialized image-part bytes) for every inline
    // image, oldest-first.
    let mut images: Vec<(usize, usize, usize)> = Vec::new();
    for (i, item) in conversation.iter().enumerate() {
        for (j, part) in image_content(item).iter().enumerate() {
            if let ContentPart::Image { url, .. } = part {
                images.push((i, j, image_part_bytes(url)));
            }
        }
    }

    // Evict oldest-first until the body fits again, keeping the newest images.
    let mut running = current_bytes;
    let mut evicted = 0usize;
    let mut evicted_parts = Vec::new();
    let mut evicted_identities = Vec::new();
    for &(i, j, image_bytes) in &images {
        if running <= target_bytes {
            break;
        }
        if let Some(part) = image_content_mut(&mut conversation[i]).get_mut(j) {
            evicted_identities.push(image_part_identity(part).expect("enumerated image part"));
            *part = placeholder.clone();
            // Net body saving: the image part leaves, the placeholder takes its
            // slot. Everything else (siblings, commas, brackets) is untouched,
            // so this is the exact change in the serialized body size.
            running = running
                .saturating_sub(image_bytes)
                .saturating_add(placeholder_bytes);
            evicted += 1;
            evicted_parts.push((i, j));
        }
    }

    ImageEvictionOutcome {
        evicted,
        evicted_parts,
        evicted_identities,
        body_bytes_after: running,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Timeline, TimelineEventKind};

    type LegacyChoice = (
        crate::SurfaceId,
        usize,
        (std::sync::Weak<str>, Option<std::sync::Weak<str>>),
    );

    fn surface_ids(count: usize) -> Vec<crate::SurfaceId> {
        (0..count)
            .map(|index| crate::SurfaceId {
                event: crate::EventSeq::new(index as u64 + 1),
                item: 0,
            })
            .collect()
    }

    #[derive(Clone, Copy, Debug)]
    enum RewindPlacement {
        None,
        Old,
        Latest,
    }

    fn cache_key_fixture_config() -> sampling_types::SamplingConfig {
        sampling_types::SamplingConfig {
            base_url: "https://example.invalid".into(),
            model: "probe-model".into(),
            output_limit: None,
            temperature: None,
            top_p: None,
            api_backend: sampling_types::ApiBackend::ChatCompletions,
            extra_headers: Default::default(),
            query_params: Default::default(),
            env_http_headers: Default::default(),
            context_window: std::num::NonZeroU64::new(100_000).unwrap(),
            reasoning_effort: None,
            stream_tool_calls: None,
        }
    }

    fn cache_key_fixture_timeline(event_count: usize, placement: RewindPlacement) -> Timeline {
        let mut timeline = Timeline::from_seed(vec![ConversationItem::system("system")]).unwrap();
        timeline
            .record(TimelineEventKind::Turn(crate::TurnEvent::Started {
                prompt_id: "cache-key-probe".into(),
                id: crate::TurnId(1),
                input_ids: Vec::new(),
                identity: crate::TurnIdentity {
                    origin: "user".into(),
                    turn_kind: "internal".into(),
                    goal_id: None,
                    goal_definition_revision: None,
                    stage_id: None,
                },
                model_id: "probe-model".into(),
                input_message_count: timeline.surface_len(),
                prompt_index: 0,
                prompt_text: "prompt".into(),
                input_kind: crate::TurnInputKind::Prompt,
                redirect_kind: None,
            }))
            .unwrap();
        let mut prompt = ConversationItem::user("prompt");
        prompt.set_prompt_index(0);
        timeline.append(prompt, MessageCause::User).unwrap();
        timeline
            .record(TimelineEventKind::Turn(crate::TurnEvent::Ended {
                id: crate::TurnId(1),
                outcome: "completed".into(),
                duration_ms: 1,
                tool_count: 0,
                terminal: crate::TurnTerminal {
                    source: crate::TurnTerminalSource::Host,
                    stop_reason: "end_turn".into(),
                    completion_kind: "completed".into(),
                },
                cancellation_category: None,
                details: None,
            }))
            .unwrap();

        let target = |timeline: &mut Timeline| {
            let rewound = timeline.rewind_surface(0).unwrap();
            timeline.replace_all(rewound, MessageCause::Rewind).unwrap();
        };
        let recovery = |timeline: &mut Timeline| {
            timeline
                .record(TimelineEventKind::Recovery(crate::RecoveryEvent {
                    action: "cache-key-probe".into(),
                    correlation_id: None,
                    reason: "filler".into(),
                    details: None,
                }))
                .unwrap();
        };

        let remaining = event_count - timeline.events().len();
        match placement {
            RewindPlacement::None => {
                for _ in 0..remaining {
                    recovery(&mut timeline);
                }
            }
            RewindPlacement::Old => {
                target(&mut timeline);
                for _ in timeline.events().len()..event_count {
                    recovery(&mut timeline);
                }
            }
            RewindPlacement::Latest => {
                for _ in 0..remaining.saturating_sub(1) {
                    recovery(&mut timeline);
                }
                target(&mut timeline);
            }
        }
        assert_eq!(timeline.events().len(), event_count);
        timeline
    }

    fn reverse_scan_prompt_cache_key(
        timeline_id: &str,
        timeline: &Timeline,
        sampling: &sampling_types::SamplingConfig,
        epoch_nonce: &str,
    ) -> String {
        let branch_anchor = timeline
            .events()
            .iter()
            .rev()
            .find_map(|event| {
                event
                    .messages()
                    .filter(|messages| messages.cause == MessageCause::Rewind)
                    .map(|_| event.seq.get())
            })
            .map_or_else(|| "root".to_owned(), |seq| seq.to_string());
        let backend = match sampling.api_backend {
            sampling_types::ApiBackend::ChatCompletions => "chat_completions",
            sampling_types::ApiBackend::Responses => "responses",
            sampling_types::ApiBackend::Messages => "messages",
        };
        let mut hasher = blake3::Hasher::new();
        for component in [
            "grow-prompt-cache-v1",
            timeline_id,
            branch_anchor.as_str(),
            backend,
            sampling.base_url.as_str(),
            sampling.model.as_str(),
            epoch_nonce,
        ] {
            hasher.update(component.as_bytes());
            hasher.update(&[0]);
        }
        let digest = hasher.finalize().to_hex();
        format!("grow-{}", &digest.as_str()[..32])
    }

    fn image_items(parts_per_item: &[usize]) -> Vec<ConversationItem> {
        parts_per_item
            .iter()
            .enumerate()
            .map(|(item_index, part_count)| {
                let mut item = ConversationItem::user(format!("item {item_index}"));
                if *part_count > 0 {
                    let ConversationItem::User(user) = &mut item else {
                        unreachable!();
                    };
                    user.content.clear();
                }
                for part_index in 0..*part_count {
                    item.add_image(format!("data:image/png;base64,{item_index}-{part_index}"));
                }
                item
            })
            .collect()
    }

    fn described_image_item(url: &str, description: &str) -> ConversationItem {
        let mut item = ConversationItem::user("described image");
        let ConversationItem::User(user) = &mut item else {
            unreachable!();
        };
        user.content = vec![ContentPart::Image {
            url: std::sync::Arc::<str>::from(url),
            description: Some(std::sync::Arc::<str>::from(description)),
        }];
        item
    }

    fn selection_and_legacy_choices(
        saved_items: &[ConversationItem],
        saved_ids: &[crate::SurfaceId],
        specs: &[(usize, usize, usize, usize)],
    ) -> (ImageBudgetSelection, Vec<LegacyChoice>) {
        let mut selection = ImageBudgetSelection {
            route: sampling_types::ModelImageInputKey::new("test-model", "responses", "test"),
            use_image_descriptions: false,
            surface_ids: saved_ids.to_vec(),
            replaced: Default::default(),
        };
        let mut legacy = Vec::with_capacity(specs.len());
        for &(source_index, key_part, identity_index, identity_part) in specs {
            let identity = image_part_identity(
                image_content(&saved_items[identity_index])
                    .get(identity_part)
                    .expect("fixture identity points to a saved image part"),
            )
            .expect("fixture identity is an inline image");
            selection
                .replaced
                .insert((source_index, key_part), identity.clone());
            legacy.push((saved_ids[source_index], key_part, identity));
        }
        legacy.sort_by_key(|(source, part, _)| (*source, *part));
        (selection, legacy)
    }

    fn legacy_choices_are_valid(
        items: &[ConversationItem],
        surface_ids: &[crate::SurfaceId],
        choices: &[LegacyChoice],
    ) -> bool {
        choices.iter().all(|(source, part_index, identity)| {
            surface_ids
                .iter()
                .position(|id| id == source)
                .and_then(|index| image_content(&items[index]).get(*part_index))
                .is_some_and(|part| image_part_matches(part, identity))
        })
    }

    fn legacy_apply_choices(
        items: &mut [ConversationItem],
        surface_ids: &[crate::SurfaceId],
        choices: &[LegacyChoice],
    ) {
        for (source, part_index, identity) in choices {
            if let Some(index) = surface_ids.iter().position(|id| id == source)
                && let Some(part) = image_content_mut(&mut items[index]).get_mut(*part_index)
                && image_part_matches(part, identity)
            {
                *part = ContentPart::Text {
                    text: IMAGE_COMPACT_PLACEHOLDER.into(),
                };
            }
        }
    }

    fn serialized_items(items: &[ConversationItem]) -> Vec<serde_json::Value> {
        items
            .iter()
            .map(|item| serde_json::to_value(item).unwrap())
            .collect()
    }

    fn assert_matches_legacy(
        label: &str,
        saved_items: &[ConversationItem],
        saved_ids: &[crate::SurfaceId],
        current_items: &[ConversationItem],
        current_ids: &[crate::SurfaceId],
        specs: &[(usize, usize, usize, usize)],
    ) {
        let (selection, legacy) = selection_and_legacy_choices(saved_items, saved_ids, specs);

        // Request assembly clears every choice if any source is stale.
        let expected_valid = legacy_choices_are_valid(current_items, current_ids, &legacy);
        let actual_valid = image_budget_choices_are_valid(current_items, current_ids, &selection);
        assert_eq!(actual_valid, expected_valid, "{label}: validation");
        let mut expected_request = current_items.to_vec();
        if expected_valid {
            legacy_apply_choices(&mut expected_request, current_ids, &legacy);
        }
        let mut actual_request = current_items.to_vec();
        let mut request_selection = selection.clone();
        if !actual_valid {
            request_selection.replaced.clear();
        }
        apply_image_budget_choices(&mut actual_request, current_ids, &request_selection);
        assert_eq!(
            serialized_items(&actual_request),
            serialized_items(&expected_request),
            "{label}: request all-or-clear application"
        );

        // Context-pressure refresh skips only entries that fail their own
        // source/part/identity check.
        let mut expected_pressure = current_items.to_vec();
        legacy_apply_choices(&mut expected_pressure, current_ids, &legacy);
        let mut actual_pressure = current_items.to_vec();
        apply_image_budget_choices(&mut actual_pressure, current_ids, &selection);
        assert_eq!(
            serialized_items(&actual_pressure),
            serialized_items(&expected_pressure),
            "{label}: pressure per-choice application"
        );
    }

    #[test]
    fn mixed_image_carriers_share_count_eviction_and_byte_accounting() {
        let tool = ConversationItem::tool_result_with_images(
            "call",
            "read image",
            vec![ContentPart::Image {
                description: None,
                url: format!("data:image/png;base64,{}", "A".repeat(TEST_IMG_BYTES)).into(),
            }],
        );
        let mut items = vec![
            tool.clone(),
            user_with_image_of_bytes("newer", TEST_IMG_BYTES),
        ];
        let bytes = conversation_body_bytes(&items);
        assert_eq!(inline_image_count(&items), 2);
        assert_eq!(bytes, serde_json::to_vec(&items).unwrap().len());
        let outcome = compact_images_to_byte_budget(&mut items, bytes, bytes - TEST_IMG_BYTES / 2);
        assert_eq!(outcome.evicted, 1);
        assert_eq!(
            outcome.body_bytes_after,
            serde_json::to_vec(&items).unwrap().len()
        );
        assert_eq!(inline_image_count(&items), 1);
        let ConversationItem::ToolResult(result) = &items[0] else {
            panic!()
        };
        assert_eq!(result.tool_call_id, "call");
        assert_eq!(result.content.as_ref(), "read image");
        assert!(matches!(&result.images[0], ContentPart::Text { .. }));
        assert!(
            super::super::state::estimate_item_tokens(&tool)
                >= token_estimation::IMAGE_TOKEN_ESTIMATE
        );
    }

    // -- image size-gated compaction tests --

    /// A user message with a small fixed inline image.
    fn user_with_image(text: &str) -> ConversationItem {
        let mut item = ConversationItem::user(text);
        item.add_image("data:image/png;base64,iVBORw0KGgo=");
        item
    }

    /// A user message carrying an inline image whose `data:` URL is exactly
    /// `url_bytes` long (must be >= the data-URL prefix length).
    fn user_with_image_of_bytes(text: &str, url_bytes: usize) -> ConversationItem {
        const PREFIX: &str = "data:image/png;base64,";
        let pad = url_bytes.saturating_sub(PREFIX.len());
        let mut item = ConversationItem::user(text);
        item.add_image(format!("{PREFIX}{}", "A".repeat(pad)));
        item
    }

    fn has_image(item: &ConversationItem) -> bool {
        matches!(
            item,
            ConversationItem::User(u)
                if u.content.iter().any(|p| matches!(p, ContentPart::Image { .. }))
        )
    }

    fn has_placeholder(item: &ConversationItem) -> bool {
        matches!(
            item,
            ConversationItem::User(u) if u.content.iter().any(|p| matches!(
                p,
                ContentPart::Text { text } if text.as_ref() == IMAGE_COMPACT_PLACEHOLDER
            ))
        )
    }

    // Images are sized ~100 KB so the ~235 B placeholder that replaces an
    // evicted image is negligible: each eviction frees ~one image's bytes.
    const TEST_IMG_BYTES: usize = 100_000;

    #[test]
    fn no_eviction_when_at_or_below_target() {
        // Multiple old image turns are *retained* when the body already fits —
        // the key behavior change from the old "strip everything but newest".
        let mut conv = vec![
            ConversationItem::system("sys"),
            user_with_image_of_bytes("first", TEST_IMG_BYTES),
            ConversationItem::assistant("a"),
            user_with_image_of_bytes("second", TEST_IMG_BYTES),
            user_with_image_of_bytes("third", TEST_IMG_BYTES),
        ];
        // current < target: nothing to do.
        compact_images_to_byte_budget(&mut conv, 300_000, 400_000);
        assert_eq!(conv.iter().filter(|i| has_image(i)).count(), 3);
    }

    #[test]
    fn evicts_oldest_until_under_target() {
        let mut conv = vec![
            user_with_image_of_bytes("oldest", TEST_IMG_BYTES),
            user_with_image_of_bytes("middle", TEST_IMG_BYTES),
            user_with_image_of_bytes("newest", TEST_IMG_BYTES),
        ];
        // current 300k, target 250k: evicting the oldest (~100 KB) fits.
        let outcome = compact_images_to_byte_budget(&mut conv, 300_000, 250_000);
        assert_eq!(outcome.evicted_parts, vec![(0, 1)]);
        assert!(has_placeholder(&conv[0]), "oldest evicted");
        assert!(has_image(&conv[1]), "middle kept");
        assert!(has_image(&conv[2]), "newest kept");
    }

    #[test]
    fn evicts_more_oldest_for_lower_target() {
        let mut conv = vec![
            user_with_image_of_bytes("oldest", TEST_IMG_BYTES),
            user_with_image_of_bytes("middle", TEST_IMG_BYTES),
            user_with_image_of_bytes("newest", TEST_IMG_BYTES),
        ];
        // current 300k, target 150k: must drop the two oldest to fit.
        compact_images_to_byte_budget(&mut conv, 300_000, 150_000);
        assert!(has_placeholder(&conv[0]));
        assert!(has_placeholder(&conv[1]));
        assert!(has_image(&conv[2]), "newest kept");
    }

    #[test]
    fn eviction_reclaims_batch_to_low_water_mark() {
        // Mirror production: a body sitting just over the trigger, made of many
        // equal images, is reclaimed in one pass down to the low-water mark —
        // dropping a *batch* of the oldest, not just the one image needed to
        // clear the trigger. This is the hysteresis that keeps the prefix
        // cache-warm for the following turns.
        let img_bytes = 1_000_000usize; // ~1 MB url each
        let n = (IMAGE_COMPACT_TRIGGER_BYTES / img_bytes) + 2; // body just over trigger
        let mut conv: Vec<ConversationItem> = (0..n)
            .map(|i| user_with_image_of_bytes(&format!("i{i}"), img_bytes))
            .collect();
        let current = n * img_bytes;
        assert!(current > IMAGE_COMPACT_TRIGGER_BYTES);

        compact_images_to_byte_budget(&mut conv, current, IMAGE_COMPACT_RECLAIM_TARGET_BYTES);

        let kept = conv.iter().filter(|i| has_image(i)).count();
        let evicted = conv.iter().filter(|i| has_placeholder(i)).count();

        // Clearing only the trigger would evict ~3 images; reclaiming to the
        // low-water mark (~half the ceiling) must evict far more.
        assert!(
            evicted > n / 4,
            "expected batch eviction to the low-water mark, only {evicted}/{n} evicted"
        );
        // Oldest-first stops at the mark, so the most recent image survives.
        assert!(kept > 0);
        assert!(
            has_image(conv.last().unwrap()),
            "most recent image must be retained"
        );
    }

    #[test]
    fn evicts_all_when_target_below_one_image() {
        let mut conv = vec![
            user_with_image_of_bytes("a", TEST_IMG_BYTES),
            user_with_image_of_bytes("b", TEST_IMG_BYTES),
        ];
        compact_images_to_byte_budget(&mut conv, 200_000, 50_000);
        assert!(has_placeholder(&conv[0]));
        assert!(has_placeholder(&conv[1]));
    }

    #[test]
    fn eviction_keeps_newest_and_is_idempotent() {
        let mut conv = vec![
            user_with_image_of_bytes("i0", TEST_IMG_BYTES),
            user_with_image_of_bytes("i1", TEST_IMG_BYTES),
            user_with_image_of_bytes("i2", TEST_IMG_BYTES),
            user_with_image_of_bytes("i3", TEST_IMG_BYTES),
        ];
        // current 400k, target 250k: drop the two oldest, keep the newest two.
        compact_images_to_byte_budget(&mut conv, 400_000, 250_000);
        assert!(has_placeholder(&conv[0]) && has_placeholder(&conv[1]));
        assert!(has_image(&conv[2]) && has_image(&conv[3]));

        // Re-running with the now-smaller body is a no-op (sticky): the two
        // surviving images already fit.
        compact_images_to_byte_budget(&mut conv, 200_000, 250_000);
        assert!(has_placeholder(&conv[0]) && has_placeholder(&conv[1]));
        assert!(has_image(&conv[2]) && has_image(&conv[3]));
    }

    #[test]
    fn evicted_image_uses_honest_placeholder() {
        let mut conv = vec![user_with_image_of_bytes("x", TEST_IMG_BYTES)];
        compact_images_to_byte_budget(&mut conv, 100_000, 10);
        assert!(has_placeholder(&conv[0]));
    }

    // -- conversation_body_bytes tests --

    #[test]
    fn conversation_body_bytes_empty_is_json_array() {
        // serde encodes an empty slice as "[]" (2 bytes).
        assert_eq!(conversation_body_bytes(&[]), 2);
    }

    #[test]
    fn conversation_body_bytes_matches_serde_json_exactly() {
        // The blank-and-add-URLs measurement must equal a full serde_json
        // encode byte-for-byte — including non-image content and string
        // escaping. The `"` in the system text is escaped by serde; the
        // measurement must account for it.
        let conv = vec![
            ConversationItem::system("system \"quoted\" prompt"),
            user_with_image("look"),
            ConversationItem::assistant("a longer assistant reply with text"),
            ConversationItem::user("plain follow-up turn"),
        ];
        let expected = serde_json::to_vec(&conv).unwrap().len();
        assert_eq!(conversation_body_bytes(&conv), expected);
    }

    #[test]
    fn conversation_body_bytes_matches_serde_json_with_large_image() {
        // Exact even for a multi-KB base64 payload — the scan we deliberately
        // skip still lands on the same byte count.
        let conv = vec![user_with_image_of_bytes("big", 50_000)];
        let expected = serde_json::to_vec(&conv).unwrap().len();
        assert_eq!(conversation_body_bytes(&conv), expected);
    }

    #[test]
    fn conversation_body_bytes_small_image_is_below_trigger() {
        // A normal small inline image must not trip the 50 MB gate — the case
        // the cache-miss fix preserves.
        let conv = vec![
            user_with_image("old"),
            ConversationItem::assistant("reply"),
            ConversationItem::user("current"),
        ];
        assert!(conversation_body_bytes(&conv) < IMAGE_COMPACT_TRIGGER_BYTES);
    }

    #[test]
    fn conversation_body_bytes_large_image_reaches_trigger() {
        let conv = vec![user_with_image_of_bytes("big", IMAGE_COMPACT_TRIGGER_BYTES)];
        assert!(conversation_body_bytes(&conv) >= IMAGE_COMPACT_TRIGGER_BYTES);
    }

    // -- edge cases: exactness, boundaries, ordering --

    #[test]
    fn body_bytes_parity_multi_image_unicode_escaping() {
        // The gate is only as correct as this equality. Exercise multiple
        // images in one turn, multibyte unicode (passed through, not escaped),
        // and chars serde *does* escape (`"`, `\`, control).
        let mut turn = ConversationItem::user("two pics 🚀 with \"quotes\" and \\ slash");
        turn.add_image("data:image/png;base64,AAAA");
        turn.add_image("data:image/png;base64,BBBBBB");
        let conv = vec![
            ConversationItem::system("sys 日本語 \t control"),
            turn,
            ConversationItem::assistant("reply"),
            ConversationItem::user("plain follow-up"),
        ];
        assert_eq!(
            conversation_body_bytes(&conv),
            serde_json::to_vec(&conv).unwrap().len()
        );
    }

    #[test]
    fn no_eviction_when_exactly_at_target() {
        // The no-op guard is `current <= target`; pin the inclusive boundary.
        let mut conv = vec![user_with_image_of_bytes("a", TEST_IMG_BYTES)];
        compact_images_to_byte_budget(&mut conv, 250_000, 250_000);
        assert!(has_image(&conv[0]), "exactly at target must not evict");
    }

    #[test]
    fn terminates_when_placeholder_exceeds_image() {
        // Tiny images: each "saving" saturates to 0, but the loop must still
        // terminate and replace every image when the target is unreachable.
        let mut conv = vec![
            user_with_image_of_bytes("a", 40),
            user_with_image_of_bytes("b", 40),
        ];
        compact_images_to_byte_budget(&mut conv, 1_000, 10);
        assert!(has_placeholder(&conv[0]) && has_placeholder(&conv[1]));
    }

    #[test]
    fn evicts_oldest_image_parts_first() {
        // `has_image`/`has_placeholder` are per-item, so count actual image
        // parts to verify oldest-first ordering across parts within a turn.
        fn image_parts(conv: &[ConversationItem]) -> usize {
            conv.iter()
                .filter_map(|i| match i {
                    ConversationItem::User(u) => Some(u),
                    _ => None,
                })
                .flat_map(|u| u.content.iter())
                .filter(|p| matches!(p, ContentPart::Image { .. }))
                .count()
        }
        let mut newest = ConversationItem::user("newest turn");
        newest.add_image(format!(
            "data:image/png;base64,{}",
            "A".repeat(TEST_IMG_BYTES)
        ));
        newest.add_image(format!(
            "data:image/png;base64,{}",
            "B".repeat(TEST_IMG_BYTES)
        ));
        let mut conv = vec![user_with_image_of_bytes("oldest", TEST_IMG_BYTES), newest];
        assert_eq!(image_parts(&conv), 3);

        // ~300k body, reclaim to 150k: drop the two oldest, keep the newest.
        compact_images_to_byte_budget(&mut conv, 300_000, 150_000);
        assert_eq!(image_parts(&conv), 1, "newest image survives");
        assert!(has_placeholder(&conv[0]), "oldest turn evicted");
        assert!(has_image(&conv[1]), "newest turn keeps an image");
    }

    #[test]
    fn escaped_remote_url_is_a_lower_bound_only() {
        // base64 `data:` URLs are exact; a remote URL with a JSON-escaped char
        // under-counts by the escape bytes. Pin that documented bound so the
        // measurement can't silently drift past it.
        let mut item = ConversationItem::user("");
        item.add_image(r#"https://example.com/a"b"#);
        let conv = vec![item];
        assert!(conversation_body_bytes(&conv) <= serde_json::to_vec(&conv).unwrap().len());
    }

    #[test]
    fn image_budget_choice_validation_and_application_match_legacy_positions() {
        let ids = surface_ids(4);
        let empty_items = image_items(&[1, 1, 1]);
        assert_matches_legacy(
            "zero choices",
            &empty_items,
            &ids[..3],
            &empty_items,
            &ids[..3],
            &[],
        );

        let one_source = image_items(&[1, 1, 1]);
        assert_matches_legacy(
            "one choice",
            &one_source,
            &ids[..3],
            &one_source,
            &ids[..3],
            &[(1, 0, 1, 0)],
        );

        let multi_source = image_items(&[1, 1, 1]);
        assert_matches_legacy(
            "multiple sources",
            &multi_source,
            &ids[..3],
            &multi_source,
            &ids[..3],
            &[(0, 0, 0, 0), (2, 0, 2, 0)],
        );

        let multipart = image_items(&[3, 1]);
        assert_matches_legacy(
            "multiple parts from one source",
            &multipart,
            &ids[..2],
            &multipart,
            &ids[..2],
            &[(0, 0, 0, 0), (0, 2, 0, 2)],
        );

        // One invalid member clears the full request choice set, while the
        // context-pressure helper still applies its independently valid row.
        let out_of_bounds = image_items(&[1, 1]);
        assert_matches_legacy(
            "out-of-bounds part with valid sibling",
            &out_of_bounds,
            &ids[..2],
            &out_of_bounds,
            &ids[..2],
            &[(0, 0, 0, 0), (1, 4, 1, 0)],
        );

        let non_image = vec![
            image_items(&[1])[0].clone(),
            ConversationItem::user("plain"),
        ];
        assert_matches_legacy(
            "non-image source with valid sibling",
            &non_image,
            &ids[..2],
            &non_image,
            &ids[..2],
            &[(0, 0, 0, 0), (1, 0, 0, 0)],
        );

        let stale_source = image_items(&[1, 1, 1]);
        let mut changed_ids = ids[..3].to_vec();
        changed_ids[1] = ids[3];
        assert!(!changed_ids.starts_with(&ids[..3]));
        assert_matches_legacy(
            "changed SurfaceId at saved slot",
            &stale_source,
            &ids[..3],
            &stale_source,
            &changed_ids,
            &[(0, 0, 0, 0), (1, 0, 1, 0)],
        );

        let saved_same_text = image_items(&[1, 1]);
        let current_same_text = vec![image_items(&[1])[0].clone(), saved_same_text[1].clone()];
        assert_matches_legacy(
            "new Arc with identical URL text",
            &saved_same_text,
            &ids[..2],
            &current_same_text,
            &ids[..2],
            &[(0, 0, 0, 0), (1, 0, 1, 0)],
        );

        let described_saved = vec![
            described_image_item("data:image/png;base64,described", "same description"),
            image_items(&[1])[0].clone(),
        ];
        let mut described_current = described_saved.clone();
        let ConversationItem::User(described_user) = &mut described_current[0] else {
            unreachable!();
        };
        let ContentPart::Image { description, .. } = &mut described_user.content[0] else {
            unreachable!();
        };
        *description = Some(std::sync::Arc::<str>::from("same description"));
        assert_matches_legacy(
            "new description Arc with same text",
            &described_saved,
            &ids[..2],
            &described_current,
            &ids[..2],
            &[(0, 0, 0, 0), (1, 0, 1, 0)],
        );

        let saved_prefix = image_items(&[1, 1]);
        let mut appended_items = saved_prefix.clone();
        appended_items.push(image_items(&[1])[0].clone());
        assert!(ids[..3].starts_with(&ids[..2]));
        assert_matches_legacy(
            "valid prefix append",
            &saved_prefix,
            &ids[..2],
            &appended_items,
            &ids[..3],
            &[(0, 0, 0, 0), (1, 0, 1, 0)],
        );

        let nonmonotonic_ids = vec![ids[2], ids[0], ids[1]];
        let nonmonotonic_items = image_items(&[1, 1, 1]);
        let (nonmonotonic_selection, legacy) = selection_and_legacy_choices(
            &nonmonotonic_items,
            &nonmonotonic_ids,
            &[(0, 0, 0, 0), (1, 0, 1, 0), (2, 0, 2, 0)],
        );
        let mut expected_surface_order = Vec::new();
        for (index, source) in nonmonotonic_ids.iter().enumerate() {
            for part in 0..image_content(&nonmonotonic_items[index]).len() {
                if legacy.iter().any(|(legacy_source, legacy_part, _)| {
                    legacy_source == source && *legacy_part == part
                }) {
                    expected_surface_order.push((index, part));
                }
            }
        }
        let effective_evicted_parts: Vec<_> =
            nonmonotonic_selection.replaced.keys().copied().collect();
        assert_eq!(
            effective_evicted_parts, expected_surface_order,
            "index-key order preserves current Surface order even for nonmonotonic IDs"
        );
    }

    #[test]
    fn prompt_cache_key_matches_reverse_scan_for_rewind_positions() {
        let sampling = cache_key_fixture_config();
        for event_count in [128, 2_048, 8_192] {
            for placement in [
                RewindPlacement::None,
                RewindPlacement::Old,
                RewindPlacement::Latest,
            ] {
                let timeline = cache_key_fixture_timeline(event_count, placement);
                assert_eq!(
                    timeline.last_rewind_seq(),
                    timeline.events().iter().rev().find_map(|event| {
                        event
                            .messages()
                            .filter(|messages| messages.cause == MessageCause::Rewind)
                            .map(|_| event.seq)
                    }),
                    "indexed anchor matches the canonical event scan"
                );
                assert_eq!(
                    prompt_cache_key("timeline", &timeline, &sampling, "epoch"),
                    reverse_scan_prompt_cache_key("timeline", &timeline, &sampling, "epoch"),
                    "key bytes remain identical at {event_count} events for {placement:?}"
                );
            }
        }
    }
}
