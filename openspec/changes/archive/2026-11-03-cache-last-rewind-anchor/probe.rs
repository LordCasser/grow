//! Temporary probe source, preserved outside the build after collecting samples.
//! To rerun, paste only the final `#[test]` function into
//! `actor::request_builder::tests`. That module already retains
//! `RewindPlacement`, `cache_key_fixture_config`, `cache_key_fixture_timeline`
//! and `reverse_scan_prompt_cache_key` for the permanent differential test.
//! The helper copies below preserve the measured fixture; do not paste them
//! alongside the existing definitions.

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

fn cache_key_fixture_timeline(event_count: usize, placement: RewindPlacement) -> crate::Timeline {
    use crate::{MessageCause, Timeline, TimelineEventKind};
    use sampling_types::ConversationItem;

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
    timeline: &crate::Timeline,
    sampling: &sampling_types::SamplingConfig,
    epoch_nonce: &str,
) -> String {
    use crate::MessageCause;

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

#[test]
#[ignore = "temporary C3 before/after microbenchmark; run explicitly and remove after recording"]
fn prompt_cache_key_reverse_scan_vs_index_microbenchmark() {
    use std::time::Instant;

    fn samples_ns(mut run: impl FnMut() -> String) -> Vec<u128> {
        for _ in 0..2 {
            std::hint::black_box(run());
        }
        (0..7)
            .map(|_| {
                let start = Instant::now();
                std::hint::black_box(run());
                start.elapsed().as_nanos()
            })
            .collect()
    }

    fn median(samples: &[u128]) -> u128 {
        let mut sorted = samples.to_vec();
        sorted.sort_unstable();
        sorted[sorted.len() / 2]
    }

    let sampling = cache_key_fixture_config();
    for event_count in [128, 2_048, 8_192] {
        for placement in [
            RewindPlacement::None,
            RewindPlacement::Old,
            RewindPlacement::Latest,
        ] {
            let timeline = cache_key_fixture_timeline(event_count, placement);
            let old_key = reverse_scan_prompt_cache_key("timeline", &timeline, &sampling, "epoch");
            let indexed_key = super::prompt_cache_key("timeline", &timeline, &sampling, "epoch");
            assert_eq!(old_key, indexed_key);

            let old_samples = samples_ns(|| {
                reverse_scan_prompt_cache_key("timeline", &timeline, &sampling, "epoch")
            });
            let indexed_samples = samples_ns(|| {
                super::prompt_cache_key("timeline", &timeline, &sampling, "epoch")
            });
            eprintln!(
                "events={event_count} placement={placement:?} old_raw_ns={old_samples:?} old_median_ns={} indexed_raw_ns={indexed_samples:?} indexed_median_ns={}",
                median(&old_samples),
                median(&indexed_samples),
            );
        }
    }
}
