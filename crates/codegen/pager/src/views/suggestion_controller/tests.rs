use super::*;

// -- accept_ghost ---------------------------------------------------------

#[test]
fn accept_full_returns_entire_ghost_and_clears() {
    let mut sc = SuggestionController::new();
    sc.set_ghost("ls -la /tmp".into(), SuggestionSource::History);

    let accepted = sc.accept_ghost(AcceptMode::Full);
    assert_eq!(accepted.as_deref(), Some("ls -la /tmp"));
    assert!(!sc.has_ghost());
    assert_eq!(sc.ghost.source, SuggestionSource::None);
}

#[test]
fn accept_full_on_empty_returns_none() {
    let mut sc = SuggestionController::new();
    assert!(sc.accept_ghost(AcceptMode::Full).is_none());
}

#[test]
fn accept_one_word_takes_first_word_leaves_rest() {
    let mut sc = SuggestionController::new();
    sc.set_ghost("--verbose --debug".into(), SuggestionSource::PathExecutable);

    let accepted = sc.accept_ghost(AcceptMode::OneWord);
    assert_eq!(accepted.as_deref(), Some("--verbose"));
    assert_eq!(sc.ghost_text(), Some(" --debug"));
    // Source preserved while ghost is non-empty.
    assert_eq!(sc.ghost.source, SuggestionSource::PathExecutable);
}

#[test]
fn accept_one_word_with_leading_whitespace() {
    let mut sc = SuggestionController::new();
    sc.set_ghost(" --verbose --debug".into(), SuggestionSource::History);

    let accepted = sc.accept_ghost(AcceptMode::OneWord);
    assert_eq!(accepted.as_deref(), Some(" --verbose"));
    assert_eq!(sc.ghost_text(), Some(" --debug"));
}

#[test]
fn accept_one_word_exhausts_ghost() {
    let mut sc = SuggestionController::new();
    sc.set_ghost("done".into(), SuggestionSource::FilePath);

    let accepted = sc.accept_ghost(AcceptMode::OneWord);
    assert_eq!(accepted.as_deref(), Some("done"));
    assert!(!sc.has_ghost());
    assert_eq!(sc.ghost.source, SuggestionSource::None);
}

#[test]
fn accept_one_word_on_empty_returns_none() {
    let mut sc = SuggestionController::new();
    assert!(sc.accept_ghost(AcceptMode::OneWord).is_none());
}

#[test]
fn accept_one_word_whitespace_only() {
    let mut sc = SuggestionController::new();
    sc.set_ghost("   ".into(), SuggestionSource::History);

    let accepted = sc.accept_ghost(AcceptMode::OneWord);
    assert_eq!(accepted.as_deref(), Some("   "));
    assert!(!sc.has_ghost());
}

#[test]
fn accept_one_word_unicode() {
    let mut sc = SuggestionController::new();
    sc.set_ghost("\u{00e9}dit --force".into(), SuggestionSource::History);

    let accepted = sc.accept_ghost(AcceptMode::OneWord);
    assert_eq!(accepted.as_deref(), Some("\u{00e9}dit"));
    assert_eq!(sc.ghost_text(), Some(" --force"));
}

#[test]
fn accept_one_word_progressive() {
    let mut sc = SuggestionController::new();
    sc.set_ghost(" -la /tmp".into(), SuggestionSource::History);

    assert_eq!(
        sc.accept_ghost(AcceptMode::OneWord).as_deref(),
        Some(" -la")
    );
    assert_eq!(sc.ghost_text(), Some(" /tmp"));

    assert_eq!(
        sc.accept_ghost(AcceptMode::OneWord).as_deref(),
        Some(" /tmp")
    );
    assert!(!sc.has_ghost());
}

// -- progressive matching -------------------------------------------------

#[test]
fn progressive_match_trims_matching_char() {
    let mut sc = SuggestionController::new();
    sc.set_ghost("ls -la".into(), SuggestionSource::History);
    sc.set_last_request_text("");

    assert!(sc.try_progressive_match("l"));
    assert_eq!(sc.ghost_text(), Some("s -la"));
}

#[test]
fn progressive_match_sequential_chars() {
    let mut sc = SuggestionController::new();
    sc.set_ghost("cat file.txt".into(), SuggestionSource::History);
    sc.set_last_request_text("");

    assert!(sc.try_progressive_match("c"));
    assert_eq!(sc.ghost_text(), Some("at file.txt"));

    assert!(sc.try_progressive_match("ca"));
    assert_eq!(sc.ghost_text(), Some("t file.txt"));

    assert!(sc.try_progressive_match("cat"));
    assert_eq!(sc.ghost_text(), Some(" file.txt"));
}

#[test]
fn progressive_match_mismatch_clears_ghost() {
    let mut sc = SuggestionController::new();
    sc.set_ghost("ls -la".into(), SuggestionSource::History);
    sc.set_last_request_text("");

    assert!(!sc.try_progressive_match("x"));
    assert!(!sc.has_ghost());
}

#[test]
fn progressive_match_empty_ghost_returns_false() {
    let mut sc = SuggestionController::new();
    sc.set_last_request_text("");
    assert!(!sc.try_progressive_match("a"));
}

#[test]
fn progressive_match_multi_char_append_clears() {
    let mut sc = SuggestionController::new();
    sc.set_ghost("ls -la".into(), SuggestionSource::History);
    sc.set_last_request_text("");

    // Two chars appended at once -- not a progressive match.
    assert!(!sc.try_progressive_match("ls"));
    assert!(!sc.has_ghost());
}

#[test]
fn progressive_match_prefix_mismatch_clears() {
    let mut sc = SuggestionController::new();
    sc.set_ghost("ls -la".into(), SuggestionSource::History);
    sc.set_last_request_text("he");

    // New text doesn't start with "he"
    assert!(!sc.try_progressive_match("xq"));
    assert!(!sc.has_ghost());
}

#[test]
fn progressive_match_exhausts_ghost() {
    let mut sc = SuggestionController::new();
    sc.set_ghost("a".into(), SuggestionSource::History);
    sc.set_last_request_text("");

    assert!(sc.try_progressive_match("a"));
    assert!(!sc.has_ghost());
    assert_eq!(sc.ghost.source, SuggestionSource::None);
}

#[test]
fn progressive_match_unicode_char() {
    let mut sc = SuggestionController::new();
    sc.set_ghost("\u{1f600}ok".into(), SuggestionSource::AI);
    sc.set_last_request_text("test");

    assert!(sc.try_progressive_match("test\u{1f600}"));
    assert_eq!(sc.ghost_text(), Some("ok"));
}

#[test]
fn progressive_match_empty_suffix_clears() {
    let mut sc = SuggestionController::new();
    sc.set_ghost("ls".into(), SuggestionSource::History);
    sc.set_last_request_text("abc");

    // Same text as last_request_text -- zero chars appended.
    assert!(!sc.try_progressive_match("abc"));
    assert!(!sc.has_ghost());
}

// -- set_ghost / clear_ghost / generation ---------------------------------

#[test]
fn set_ghost_increments_generation() {
    let mut sc = SuggestionController::new();
    sc.set_ghost("first".into(), SuggestionSource::History);
    let g1 = sc.ghost.generation;

    sc.set_ghost("second".into(), SuggestionSource::AI);
    let g2 = sc.ghost.generation;

    assert!(g2 > g1);
}

#[test]
fn set_ghost_preserves_full_text() {
    let mut sc = SuggestionController::new();
    sc.set_ghost("ls -la".into(), SuggestionSource::History);
    sc.set_last_request_text("");

    // Progressive match trims text but full_text stays.
    sc.try_progressive_match("l");
    assert_eq!(sc.ghost.full_text, "ls -la");
    assert_eq!(sc.ghost_text(), Some("s -la"));
}

#[test]
fn clear_ghost_resets_all_fields() {
    let mut sc = SuggestionController::new();
    sc.set_ghost("hello".into(), SuggestionSource::AI);
    sc.clear_ghost();

    assert!(!sc.has_ghost());
    assert!(sc.ghost.full_text.is_empty());
    assert_eq!(sc.ghost.source, SuggestionSource::None);
}

// -- text_changed ---------------------------------------------------------

fn enabled_controller() -> SuggestionController {
    let mut sc = SuggestionController::new();
    sc.enabled = true;
    sc
}

#[test]
fn text_changed_disabled_returns_none() {
    let mut sc = SuggestionController::new();
    assert!(!sc.enabled);
    assert!(sc.text_changed("git", false, false).is_none());
}

#[test]
fn text_changed_slash_active_suppresses_and_clears_ghost() {
    let mut sc = enabled_controller();
    sc.set_ghost("commit".into(), SuggestionSource::History);

    let result = sc.text_changed("/model", true, false);
    assert!(result.is_none());
    assert!(!sc.has_ghost());
}

/// Slash suppression is a full draft invalidation, not just a presentation
/// clear: an armed debounce or a pending Tab landing must go stale instead
/// of repopulating suggestion state behind the slash UI.
#[test]
fn text_changed_slash_active_invalidates_pending_state() {
    let mut sc = enabled_controller();
    let armed = match sc.text_changed("git", false, false) {
        Some(SuggestionAction::Debounce { generation }) => generation,
        other => panic!("expected debounce, got {other:?}"),
    };
    let pending_tab = sc.begin_tab_completion(true);

    sc.text_changed("/model", true, false);

    assert!(
        !sc.on_debounce_expired(armed),
        "armed debounce must be stale after suppression"
    );
    assert!(
        !sc.take_pending_tab(pending_tab),
        "pending Tab must be disarmed by suppression"
    );
    let stale = SuggestResponseParsed {
        ghost: None,
        completions: vec![item("notes.md", SuggestionSource::FilePath)],
        generation: pending_tab,
    };
    sc.on_suggestions_loaded(stale, "git", "git".len());
    assert!(
        sc.dropdown.items.is_empty(),
        "a landing for the suppressed draft must not repopulate"
    );
}

#[test]
fn text_changed_inline_ghost_suppresses() {
    let mut sc = enabled_controller();
    sc.set_ghost("commit".into(), SuggestionSource::History);

    let result = sc.text_changed("some /m", false, true);
    assert!(result.is_none());
    assert!(!sc.has_ghost());
}

#[test]
fn text_changed_empty_text_clears_ghost() {
    let mut sc = enabled_controller();
    sc.set_ghost("something".into(), SuggestionSource::History);
    sc.set_last_request_text("git");

    let result = sc.text_changed("", false, false);
    assert!(result.is_none());
    assert!(!sc.has_ghost());
}

#[test]
fn text_changed_progressive_match_returns_matched() {
    let mut sc = enabled_controller();
    sc.set_ghost("it commit".into(), SuggestionSource::History);
    sc.set_last_request_text("g");

    let result = sc.text_changed("gi", false, false);
    assert_eq!(result, Some(SuggestionAction::Matched));
    assert_eq!(sc.ghost_text(), Some("t commit"));
}

#[test]
fn text_changed_no_match_returns_debounce() {
    let mut sc = enabled_controller();
    let gen_before = sc.generation;

    let result = sc.text_changed("git", false, false);
    match result {
        Some(SuggestionAction::Debounce { generation }) => {
            assert!(generation > gen_before);
            assert_eq!(generation, sc.generation);
        }
        other => panic!("expected Debounce, got {other:?}"),
    }
}

#[test]
fn text_changed_increments_generation_on_debounce() {
    let mut sc = enabled_controller();
    sc.text_changed("a", false, false);
    let g1 = sc.generation;

    sc.text_changed("ab", false, false);
    let g2 = sc.generation;

    assert!(g2 > g1);
}

// -- on_debounce_expired --------------------------------------------------

#[test]
fn debounce_expired_matching_generation_returns_true() {
    let mut sc = enabled_controller();
    sc.text_changed("git", false, false);
    let current_gen = sc.generation;

    assert!(sc.on_debounce_expired(current_gen));
}

#[test]
fn debounce_expired_stale_generation_returns_false() {
    let mut sc = enabled_controller();
    sc.text_changed("git", false, false);
    let stale_gen = sc.generation;

    sc.text_changed("git c", false, false);
    assert!(!sc.on_debounce_expired(stale_gen));
}

// -- on_suggestions_loaded ------------------------------------------------

fn make_response(
    generation: u64,
    ghost_suffix: Option<&str>,
    source: SuggestionSource,
) -> SuggestResponseParsed {
    SuggestResponseParsed {
        ghost: ghost_suffix.map(|s| GhostSuggestionParsed {
            suffix: s.to_owned(),
            source,
        }),
        completions: Vec::new(),
        generation,
    }
}

#[test]
fn suggestions_loaded_sets_ghost() {
    let mut sc = enabled_controller();
    sc.text_changed("git", false, false);
    let current_gen = sc.generation;

    sc.on_suggestions_loaded(
        make_response(current_gen, Some(" commit"), SuggestionSource::History),
        "git",
        "git".len(),
    );
    assert_eq!(sc.ghost_text(), Some(" commit"));
    assert_eq!(sc.ghost.source, SuggestionSource::History);
}

#[test]
fn suggestions_loaded_stale_generation_ignored() {
    let mut sc = enabled_controller();
    sc.text_changed("git", false, false);
    let stale_gen = sc.generation;
    sc.text_changed("git c", false, false);

    sc.on_suggestions_loaded(
        make_response(stale_gen, Some(" commit"), SuggestionSource::History),
        "git",
        "git".len(),
    );
    assert!(!sc.has_ghost());
}

#[test]
fn suggestions_loaded_no_ghost_clears() {
    let mut sc = enabled_controller();
    sc.set_ghost("old".into(), SuggestionSource::History);
    sc.text_changed("git", false, false);
    let current_gen = sc.generation;

    sc.on_suggestions_loaded(
        make_response(current_gen, None, SuggestionSource::None),
        "git",
        "git".len(),
    );
    assert!(!sc.has_ghost());
}

#[test]
fn suggestions_loaded_replaces_existing_ghost() {
    let mut sc = enabled_controller();
    sc.set_ghost("old".into(), SuggestionSource::FilePath);
    sc.text_changed("git", false, false);
    let current_gen = sc.generation;

    sc.on_suggestions_loaded(
        make_response(current_gen, Some(" log"), SuggestionSource::AI),
        "git",
        "git".len(),
    );
    assert_eq!(sc.ghost_text(), Some(" log"));
    assert_eq!(sc.ghost.source, SuggestionSource::AI);
}

// -- on_suggestions_loaded: dropdown population ----------------------------

#[test]
fn suggestions_loaded_populates_dropdown() {
    let mut sc = enabled_controller();
    sc.text_changed("git", false, false);
    let current_gen = sc.generation;

    let response = SuggestResponseParsed {
        ghost: Some(GhostSuggestionParsed {
            suffix: " commit".to_owned(),
            source: SuggestionSource::History,
        }),
        completions: vec![
            CompletionItemParsed {
                display: "git commit".into(),
                description: "commit changes".into(),
                replacement: "git commit".into(),
                source: SuggestionSource::History,
                priority: 10,
                replace_range: 0..3,
                truncated: false,
            },
            CompletionItemParsed {
                display: "git checkout".into(),
                description: "switch branches".into(),
                replacement: "git checkout".into(),
                source: SuggestionSource::History,
                priority: 5,
                replace_range: 0..3,
                truncated: false,
            },
        ],
        generation: current_gen,
    };
    sc.on_suggestions_loaded(response, "git", "git".len());

    assert_eq!(sc.dropdown.items.len(), 2);
    assert_eq!(sc.dropdown.generation, current_gen);
    assert_eq!(sc.dropdown.selected, 0);
    assert_eq!(sc.dropdown.items[0].display, "git commit");
    assert_eq!(sc.dropdown.items[1].replacement, "git checkout");
    assert_eq!(sc.ghost_text(), Some(" commit"));
}

#[test]
fn suggestions_loaded_stale_does_not_populate_dropdown() {
    let mut sc = enabled_controller();
    sc.text_changed("git", false, false);
    let stale = sc.generation;
    sc.text_changed("git c", false, false);

    let response = SuggestResponseParsed {
        ghost: None,
        completions: vec![CompletionItemParsed {
            display: "stale".into(),
            description: "".into(),
            replacement: "stale".into(),
            source: SuggestionSource::None,
            priority: 0,
            replace_range: 0..3,
            truncated: false,
        }],
        generation: stale,
    };
    sc.on_suggestions_loaded(response, "git", "git".len());
    assert!(sc.dropdown.items.is_empty());
}

#[test]
fn accept_ghost_closes_dropdown() {
    let mut sc = enabled_controller();
    sc.text_changed("git", false, false);
    let current_gen = sc.generation;

    let response = SuggestResponseParsed {
        ghost: Some(GhostSuggestionParsed {
            suffix: " commit".to_owned(),
            source: SuggestionSource::History,
        }),
        completions: vec![CompletionItemParsed {
            display: "git commit".into(),
            description: "".into(),
            replacement: "git commit".into(),
            source: SuggestionSource::History,
            priority: 0,
            replace_range: 0..3,
            truncated: false,
        }],
        generation: current_gen,
    };
    sc.on_suggestions_loaded(response, "git", "git".len());
    sc.dropdown.open = true;
    assert!(sc.dropdown.open);

    sc.accept_ghost(AcceptMode::Full);
    assert!(!sc.dropdown.open);
    assert!(sc.dropdown.items.is_empty());
}

// -- SuggestResponseParsed::from_json -------------------------------------

#[test]
fn parse_response_with_ghost_and_completions() {
    let json = serde_json::json!({
        "result": {
            "generation": 42,
            "ghost": {
                "suffix": " commit",
                "source": "history"
            },
            "completions": [
                {
                    "display": "git commit",
                    "description": "commit changes",
                    "replacement": "git commit",
                    "source": "history",
                    "priority": 10,
                    "replaceRange": [0, 3],
                    "truncated": false
                }
            ]
        }
    });
    let parsed = SuggestResponseParsed::from_json(&json).unwrap();
    assert_eq!(parsed.generation, 42);
    assert_eq!(parsed.ghost.as_ref().unwrap().suffix, " commit");
    assert_eq!(
        parsed.ghost.as_ref().unwrap().source,
        SuggestionSource::History
    );
    assert_eq!(parsed.completions.len(), 1);
    assert_eq!(parsed.completions[0].priority, 10);
}

#[test]
fn parse_response_without_ghost() {
    let json = serde_json::json!({
        "result": {
            "generation": 7,
            "ghost": null,
            "completions": []
        }
    });
    let parsed = SuggestResponseParsed::from_json(&json).unwrap();
    assert!(parsed.ghost.is_none());
    assert!(parsed.completions.is_empty());
}

#[test]
fn parse_response_empty_suffix_treated_as_no_ghost() {
    let json = serde_json::json!({
        "result": {
            "generation": 1,
            "ghost": {
                "suffix": "",
                "source": "history"
            },
            "completions": []
        }
    });
    let parsed = SuggestResponseParsed::from_json(&json).unwrap();
    assert!(parsed.ghost.is_none());
}

#[test]
fn parse_response_missing_generation_returns_none() {
    let json = serde_json::json!({
        "result": {
            "ghost": null,
            "completions": []
        }
    });
    assert!(SuggestResponseParsed::from_json(&json).is_none());
}

#[test]
fn parse_response_unwrapped_format() {
    let json = serde_json::json!({
        "generation": 5,
        "ghost": null,
        "completions": []
    });
    let parsed = SuggestResponseParsed::from_json(&json).unwrap();
    assert_eq!(parsed.generation, 5);
}

fn parse_single_completion(item: serde_json::Value) -> Option<CompletionItemParsed> {
    let json = serde_json::json!({
        "result": { "generation": 1, "ghost": null, "completions": [item] }
    });
    SuggestResponseParsed::from_json(&json).and_then(|mut response| response.completions.pop())
}

#[test]
fn parse_completion_requires_one_atomic_edit() {
    let item = parse_single_completion(serde_json::json!({
        "display": "grep",
        "description": "",
        "replacement": "grep",
        "source": "path",
        "priority": 0,
        "replaceRange": [5, 7],
        "truncated": false
    }))
    .unwrap();
    assert_eq!(item.replace_range, 5..7);
    assert_eq!(item.replacement, "grep");
}

#[test]
fn parse_completion_rejects_old_or_malformed_shapes() {
    for bad in [
        serde_json::json!(null),
        serde_json::json!([5]),
        serde_json::json!([5, 7, 9]),
        serde_json::json!([7, 5]),
        serde_json::json!(["a", "b"]),
        serde_json::json!([-1, 3]),
        serde_json::json!("5..7"),
        serde_json::json!(null),
    ] {
        assert!(
            parse_single_completion(serde_json::json!({
                "display": "grep",
                "description": "",
                "replacement": "grep",
                "source": "path",
                "priority": 0,
                "replaceRange": bad,
                "truncated": false
            }))
            .is_none(),
            "shape: {bad:?}"
        );
    }

    for old in [
        serde_json::json!({
            "display": "grep", "description": "", "insertText": "ls | grep",
            "source": "path", "priority": 0, "replaceRange": [5, 7],
            "tokenText": "grep", "truncated": false
        }),
        serde_json::json!({
            "display": "grep", "description": "", "replacement": "grep",
            "source": "path", "priority": 0, "truncated": false
        }),
        serde_json::json!({
            "display": "grep", "description": "", "replacement": "grep",
            "source": "path", "priority": 0, "replaceRange": [5, 7]
        }),
        serde_json::json!({
            "display": "grep", "description": "", "replacement": "grep",
            "source": "unknown", "priority": 0, "replaceRange": [5, 7],
            "truncated": false
        }),
    ] {
        assert!(parse_single_completion(old).is_none());
    }
}

// -- validated_replace_range -----------------------------------------------

fn anchored_controller(request_text: &str) -> SuggestionController {
    let mut sc = enabled_controller();
    sc.dropdown.request_text = request_text.to_owned();
    sc
}

#[test]
fn validated_range_exact_text_passes_through() {
    let sc = anchored_controller("ls | gr");
    assert_eq!(
        sc.validated_replace_range(5..7, "grep", "ls | gr"),
        Some(5..7)
    );
}

/// Progressive typing appends at the end without clearing the dropdown;
/// a range that reached the request text's end absorbs the typed tail —
/// as long as the grown span still extends toward the replacement.
#[test]
fn validated_range_stretches_over_token_extension_tail() {
    let sc = anchored_controller("ls | gr");
    assert_eq!(
        sc.validated_replace_range(5..7, "grep", "ls | gre"),
        Some(5..8)
    );
    // Whole-line (history) ranges stretch the same way.
    let sc = anchored_controller("git st");
    assert_eq!(
        sc.validated_replace_range(0..6, "git status --porcelain", "git sta"),
        Some(0..7)
    );
    // Including progressively typed whitespace inside the completion.
    assert_eq!(
        sc.validated_replace_range(0..6, "git status --porcelain", "git status "),
        Some(0..11)
    );
}

/// A tail that is NOT an extension toward the replacement (typed args,
/// a diverging sibling item) refuses the accept outright — stretching
/// would splice the user's tail away, not stretching would leave it
/// glued after the replacement.
#[test]
fn validated_range_non_extension_tail_rejects() {
    let sc = anchored_controller("ls | gr");
    assert_eq!(sc.validated_replace_range(5..7, "grep", "ls | gr -v"), None);
    // Diverged sibling: typed toward `git status`, accepting `git stash`.
    let sc = anchored_controller("git st");
    assert_eq!(
        sc.validated_replace_range(0..6, "git stash", "git stat"),
        None
    );
}

/// A mid-text token range does NOT stretch — the tail after the token
/// belongs to the rest of the command line.
#[test]
fn validated_range_mid_text_token_keeps_end() {
    let sc = anchored_controller("cat hel | wc -l");
    assert_eq!(
        sc.validated_replace_range(4..7, "hello.txt", "cat hel | wc -l"),
        Some(4..7)
    );
}

#[test]
fn validated_range_drifted_text_falls_back() {
    let sc = anchored_controller("ls | gr");
    // Current text no longer starts with the anchor text.
    assert_eq!(sc.validated_replace_range(5..7, "grep", "echo hi"), None);
    assert_eq!(sc.validated_replace_range(5..7, "grep", "ls |"), None);
}

#[test]
fn validated_range_out_of_bounds_falls_back() {
    let sc = anchored_controller("gr");
    assert_eq!(sc.validated_replace_range(0..9, "grep", "gr"), None);
    // Unset anchor (fresh controller) rejects any non-empty range.
    let sc = enabled_controller();
    assert_eq!(sc.validated_replace_range(0..2, "grep", "gr"), None);
}

/// A wire range landing mid-character in a multibyte draft is rejected
/// (never a panic, never a mid-char splice).
#[test]
fn validated_range_mid_char_boundary_rejects() {
    // "cat café": the é spans bytes 7..9, so offset 8 is mid-char.
    let sc = anchored_controller("cat caf\u{e9}");
    assert_eq!(
        sc.validated_replace_range(4..8, "caf\u{e9}.txt", "cat caf\u{e9}"),
        None
    );
}

// -- common_prefix_fill ------------------------------------------------

fn span_item(
    token: &str,
    range: std::ops::Range<usize>,
    source: SuggestionSource,
) -> CompletionItemParsed {
    CompletionItemParsed {
        display: token.to_owned(),
        description: String::new(),
        replacement: token.to_owned(),
        source,
        priority: 0,
        replace_range: range,
        truncated: false,
    }
}

#[test]
fn common_prefix_fill_extends_typed_token() {
    let mut sc = anchored_controller("cat al");
    sc.dropdown.items = vec![
        span_item("alpha_one.txt", 4..6, SuggestionSource::FilePath),
        span_item("alpha_two.txt", 4..6, SuggestionSource::FilePath),
    ];
    let (range, fill) = sc.common_prefix_fill("cat al").expect("fill");
    assert_eq!(range, 4..6);
    assert_eq!(fill, "alpha_");
}

/// Progressive typing after the fetch: the span stretches over the tail
/// (same rule as accepts) and the fill still applies.
#[test]
fn common_prefix_fill_stretches_over_typed_tail() {
    let mut sc = anchored_controller("cat al");
    sc.dropdown.items = vec![
        span_item("alpha_one.txt", 4..6, SuggestionSource::FilePath),
        span_item("alpha_two.txt", 4..6, SuggestionSource::FilePath),
    ];
    let (range, fill) = sc.common_prefix_fill("cat alp").expect("fill");
    assert_eq!(range, 4..7);
    assert_eq!(fill, "alpha_");
}

#[test]
fn common_prefix_fill_none_when_lcp_equals_typed_token() {
    let mut sc = anchored_controller("ls | gr");
    sc.dropdown.items = vec![
        span_item("grep", 5..7, SuggestionSource::PathExecutable),
        span_item("grip", 5..7, SuggestionSource::PathExecutable),
    ];
    assert!(sc.common_prefix_fill("ls | gr").is_none());
}

/// Fuzzy candidates can share an LCP LONGER than the typed token that
/// is not an extension of it (typed `nts`, candidates `notes_*` → LCP
/// `notes_`): the `starts_with(typed)` guard refuses the fill — a
/// fill must only ever append to what the user typed, never rewrite it.
#[test]
fn common_prefix_fill_none_for_fuzzy_non_extension_lcp() {
    let mut sc = anchored_controller("cat nts");
    sc.dropdown.items = vec![
        span_item("notes_a.txt", 4..7, SuggestionSource::FilePath),
        span_item("notes_b.txt", 4..7, SuggestionSource::FilePath),
    ];
    assert!(sc.common_prefix_fill("cat nts").is_none());
}

/// Case-differing candidates (`notes.md` / `Notes Archive`) share no
/// byte prefix — bail to plain-open rather than clobber the typed case.
#[test]
fn common_prefix_fill_none_on_case_mismatch() {
    let mut sc = anchored_controller("cat no");
    sc.dropdown.items = vec![
        span_item("notes.md", 4..6, SuggestionSource::FilePath),
        span_item("Notes\\ Archive/", 4..6, SuggestionSource::FilePath),
    ];
    assert!(sc.common_prefix_fill("cat no").is_none());
}

#[test]
fn common_prefix_fill_none_on_mixed_ranges_or_single_item() {
    let mut sc = anchored_controller("cat al");
    sc.dropdown.items = vec![
        span_item("alpha_one.txt", 4..6, SuggestionSource::FilePath),
        span_item("alpha_two.txt", 3..6, SuggestionSource::FilePath),
    ];
    assert!(sc.common_prefix_fill("cat al").is_none());

    sc.dropdown.items = vec![span_item("alpha_one.txt", 4..6, SuggestionSource::FilePath)];
    assert!(
        sc.common_prefix_fill("cat al").is_none(),
        "a single candidate is the insta-accept path, not a fill"
    );
}

#[test]
fn common_prefix_fill_none_on_stale_generation() {
    let mut sc = anchored_controller("cat al");
    sc.dropdown.items = vec![
        span_item("alpha_one.txt", 4..6, SuggestionSource::FilePath),
        span_item("alpha_two.txt", 4..6, SuggestionSource::FilePath),
    ];
    sc.dropdown.generation = 3;
    assert!(sc.common_prefix_fill("cat al").is_none());
}

/// Two multibyte candidates whose byte-level LCP lands mid-character:
/// the boundary trim keeps the fill valid UTF-8 (here it collapses to
/// the typed token, so no fill).
#[test]
fn common_prefix_fill_multibyte_boundary_trim() {
    // é = C3 A9, è = C3 A8: byte LCP is "caf" + C3 (mid-char).
    let mut sc = anchored_controller("cat caf");
    sc.dropdown.items = vec![
        span_item("caf\u{e9}.txt", 4..7, SuggestionSource::FilePath),
        span_item("caf\u{e8}.txt", 4..7, SuggestionSource::FilePath),
    ];
    assert!(sc.common_prefix_fill("cat caf").is_none());
}

// -- tab_decision --------------------------------------------------------

/// Anchored controller whose items are current for `request_text` typed
/// with the cursor at its end — the state right after a landing.
fn decision_controller(request_text: &str) -> SuggestionController {
    let mut sc = anchored_controller(request_text);
    sc.dropdown.request_cursor = request_text.len();
    sc
}

#[test]
fn tab_decision_nothing_on_empty_or_stale_items() {
    let sc = decision_controller("cat no");
    assert_eq!(
        sc.tab_decision("cat no", "cat no".len()),
        TabAction::Nothing
    );

    let mut sc = decision_controller("cat no");
    sc.dropdown.items = vec![span_item("notes.md", 4..6, SuggestionSource::FilePath)];
    sc.dropdown.generation = 3;
    assert_eq!(
        sc.tab_decision("cat no", "cat no".len()),
        TabAction::Nothing,
        "items outdated by an edit must not complete"
    );
}

/// THE stale-anchor regression: items fetched for one cursor position must
/// not complete after the cursor moved without a text change (a mouse
/// click reports no edit) — Tab has to fetch for the token actually under
/// the cursor.
#[test]
fn tab_decision_nothing_on_cursor_drift() {
    let mut sc = decision_controller("cat no");
    sc.dropdown.items = vec![span_item("notes.md", 4..6, SuggestionSource::FilePath)];
    assert_eq!(
        sc.tab_decision("cat no", 2),
        TabAction::Nothing,
        "cursor moved off the fetched token"
    );
    // Typing at the end is the one tolerated drift (mirrors the range
    // stretch rule): cursor grew exactly with the tail.
    assert_eq!(
        sc.tab_decision("cat not", "cat not".len()),
        TabAction::InstaAccept
    );
    // Same length growth but the cursor did NOT follow: still stale.
    assert_eq!(sc.tab_decision("cat not", 4), TabAction::Nothing);
}

#[test]
fn tab_decision_insta_accept_single_token_candidate() {
    let mut sc = decision_controller("cat no");
    sc.dropdown.items = vec![span_item("notes.md", 4..6, SuggestionSource::FilePath)];
    assert_eq!(
        sc.tab_decision("cat no", "cat no".len()),
        TabAction::InstaAccept
    );
}

#[test]
fn tab_decision_fill_for_shared_prefix() {
    let mut sc = decision_controller("cat al");
    sc.dropdown.items = vec![
        span_item("alpha_one.txt", 4..6, SuggestionSource::FilePath),
        span_item("alpha_two.txt", 4..6, SuggestionSource::FilePath),
    ];
    assert_eq!(
        sc.tab_decision("cat al", "cat al".len()),
        TabAction::Fill(4..6, "alpha_".into())
    );
}

/// History/AI (whole-line) and mixed sets never insta-accept or fill —
/// the LCP rule sits behind the source gate in the same decision.
#[test]
fn tab_decision_open_for_whole_line_and_mixed_sets() {
    let mut sc = decision_controller("git st");
    sc.dropdown.items = vec![span_item("git status", 0..6, SuggestionSource::History)];
    assert_eq!(sc.tab_decision("git st", "git st".len()), TabAction::Open);

    let mut sc = decision_controller("cat no");
    sc.dropdown.items = vec![
        span_item("cat notes.md --verbose", 0..6, SuggestionSource::History),
        span_item("notes.md", 4..6, SuggestionSource::FilePath),
    ];
    assert_eq!(sc.tab_decision("cat no", "cat no".len()), TabAction::Open);
}

#[test]
fn tab_decision_open_when_no_extending_lcp() {
    let mut sc = decision_controller("ls | gr");
    sc.dropdown.items = vec![
        span_item("grep", 5..7, SuggestionSource::PathExecutable),
        span_item("grip", 5..7, SuggestionSource::PathExecutable),
    ];
    assert_eq!(sc.tab_decision("ls | gr", "ls | gr".len()), TabAction::Open);
}

/// A truncated (capped-scan) set must not conclude: the unscanned tail
/// could hold the row that disproves a sole match or extends past an LCP.
#[test]
fn tab_decision_truncated_rows_open_only() {
    let mut it = span_item("notes.md", 4..6, SuggestionSource::FilePath);
    it.truncated = true;
    let mut sc = decision_controller("cat no");
    sc.dropdown.items = vec![it];
    assert_eq!(sc.tab_decision("cat no", "cat no".len()), TabAction::Open);
}

/// Token texts are rendered shell literals: `a b`/`a$c` arrive as
/// `a\ b`/`a\$c`, whose byte LCP `a\` ends inside an escape. Filling it
/// would leave a dangling backslash — the trim drops it, nothing extends
/// the typed token, and the decision falls through to Open.
#[test]
fn tab_decision_lcp_ending_mid_escape_opens() {
    let mut sc = decision_controller("cat a");
    sc.dropdown.items = vec![
        span_item("a\\ b", 4..5, SuggestionSource::FilePath),
        span_item("a\\$c", 4..5, SuggestionSource::FilePath),
    ];
    assert_eq!(sc.tab_decision("cat a", "cat a".len()), TabAction::Open);
}

/// A COMPLETE escape in the LCP still fills (`a\ ` is a valid literal
/// prefix); only the dangling half gets trimmed.
#[test]
fn tab_decision_lcp_with_complete_escape_fills() {
    let mut sc = decision_controller("cat a");
    sc.dropdown.items = vec![
        span_item("a\\ bar.txt", 4..5, SuggestionSource::FilePath),
        span_item("a\\ baz.txt", 4..5, SuggestionSource::FilePath),
    ];
    assert_eq!(
        sc.tab_decision("cat a", "cat a".len()),
        TabAction::Fill(4..5, "a\\ ba".into())
    );
}

// -- accept_completion: splice resolution ---------------------------------

fn accept_controller(request_text: &str, items: Vec<CompletionItemParsed>) -> SuggestionController {
    let mut sc = anchored_controller(request_text);
    sc.dropdown.items = items;
    sc
}

/// The view probes the accept resolution BEFORE committing (a splice into
/// an atomic prompt element degrades to opening the dropdown): peek must
/// resolve like accept while consuming nothing and touching no state.
#[test]
fn peek_completion_splice_is_non_consuming() {
    let sc = accept_controller(
        "ls | gr",
        vec![span_item("grep", 5..7, SuggestionSource::PathExecutable)],
    );
    let generation = sc.generation();
    assert_eq!(
        sc.peek_completion_splice("ls | gr"),
        Some(CompletionSplice::Edit(5..7, "grep".into()))
    );
    assert_eq!(sc.dropdown.items.len(), 1, "peek must not consume");
    assert_eq!(sc.generation(), generation);
}

/// Token items resolve to an in-place splice — decided against the draft
/// BEFORE the dropdown closes.
#[test]
fn accept_completion_resolves_token_splice() {
    let mut sc = accept_controller(
        "ls | gr",
        vec![span_item("grep", 5..7, SuggestionSource::PathExecutable)],
    );
    assert_eq!(
        sc.accept_completion("ls | gr"),
        Some(CompletionSplice::Edit(5..7, "grep".into()))
    );
    assert!(sc.dropdown.items.is_empty(), "item moved out + closed");
}

/// History/AI use the same atomic edit shape, with a whole-line range.
#[test]
fn accept_completion_whole_line_edit() {
    let mut sc = accept_controller(
        "git st",
        vec![span_item(
            "git status --porcelain",
            0..6,
            SuggestionSource::History,
        )],
    );
    assert_eq!(
        sc.accept_completion("git st"),
        Some(CompletionSplice::Edit(
            0..6,
            "git status --porcelain".into()
        ))
    );
}

/// A ranged item whose span no longer fits the draft resolves to `Stale`
/// (the caller preserves the draft) — and still consumes the item.
#[test]
fn accept_completion_stale_range_resolves_stale() {
    let mut sc = accept_controller(
        "ls | gr",
        vec![span_item("grep", 5..7, SuggestionSource::PathExecutable)],
    );
    assert_eq!(
        sc.accept_completion("totally different"),
        Some(CompletionSplice::Stale)
    );
    assert!(sc.dropdown.items.is_empty());
}

// -- accept_completion / async-race invalidation ---------------------------

fn item(text: &str, source: SuggestionSource) -> CompletionItemParsed {
    CompletionItemParsed {
        display: text.to_owned(),
        description: String::new(),
        replacement: text.to_owned(),
        source,
        priority: 0,
        replace_range: 0..0,
        truncated: false,
    }
}

fn loaded_controller(text: &str, ghost_suffix: Option<&str>, insert: &str) -> SuggestionController {
    let mut sc = enabled_controller();
    sc.text_changed(text, false, false);
    let generation = sc.generation;
    sc.on_suggestions_loaded(
        SuggestResponseParsed {
            ghost: ghost_suffix.map(|s| GhostSuggestionParsed {
                suffix: s.to_owned(),
                source: SuggestionSource::History,
            }),
            completions: vec![span_item(insert, 0..text.len(), SuggestionSource::History)],
            generation,
        },
        text,
        text.len(),
    );
    sc.set_last_request_text(text);
    sc
}

/// The anchor pairs atomically with the items it describes.
#[test]
fn loaded_response_pins_request_text_on_dropdown() {
    let sc = loaded_controller("git st", Some("atus"), "git status");
    assert_eq!(sc.dropdown.request_text, "git st");
    assert_eq!(sc.dropdown.items.len(), 1);
}

/// Accepting items from a superseded generation refuses and closes: an
/// edit outdated them (every non-matching edit bumps the generation).
#[test]
fn accept_completion_stale_generation_refuses_and_closes() {
    let mut sc = loaded_controller("git st", None, "git status");
    sc.dropdown.open = true;
    sc.text_changed("git stx", false, false);

    assert!(sc.accept_completion("git stx").is_none());
    assert!(!sc.dropdown.open);
    assert!(sc.dropdown.items.is_empty());
}

/// A successful accept bumps the generation, so a response fetched for
/// the pre-accept text is discarded when it lands (no mis-anchored
/// ghost after the freshly written command).
#[test]
fn accept_completion_invalidates_in_flight_response() {
    let mut sc = loaded_controller("ls | gr", None, "ls | grep");
    sc.dropdown.open = true;
    let in_flight_gen = sc.generation;

    assert!(sc.accept_completion("ls | gr").is_some());
    sc.on_suggestions_loaded(
        make_response(in_flight_gen, Some("ep -v"), SuggestionSource::History),
        "ls | gr",
        "ls | gr".len(),
    );
    assert!(!sc.has_ghost(), "post-accept landing must be discarded");
}

/// Ghost accepts invalidate in-flight responses the same way.
#[test]
fn accept_ghost_invalidates_in_flight_response() {
    let mut sc = loaded_controller("ls | gr", Some("ep -r foo"), "ls | grep -r foo");
    let in_flight_gen = sc.generation;

    assert!(sc.accept_ghost(AcceptMode::Full).is_some());
    sc.on_suggestions_loaded(
        make_response(in_flight_gen, Some("ep -v"), SuggestionSource::History),
        "ls | gr",
        "ls | gr".len(),
    );
    assert!(!sc.has_ghost());
}

/// Clearing the prompt invalidates in-flight fetches: their response
/// must not resurrect a ghost (`atus --porcelain`) over the emptied
/// draft, where Right would commit the orphaned fragment.
#[test]
fn emptied_text_discards_in_flight_response() {
    let mut sc = enabled_controller();
    sc.text_changed("git st", false, false);
    let in_flight_gen = sc.generation;

    sc.text_changed("", false, false);
    sc.on_suggestions_loaded(
        make_response(
            in_flight_gen,
            Some("atus --porcelain"),
            SuggestionSource::History,
        ),
        "git st",
        "git st".len(),
    );
    assert!(!sc.has_ghost(), "ghost must not outlive the cleared draft");
    assert!(sc.dropdown.items.is_empty());
}

/// A non-matching edit tears down a ghost-less dropdown (pure path/file
/// items have no ghost for `try_progressive_match` to clear).
#[test]
fn non_matching_edit_tears_down_ghostless_dropdown() {
    let mut sc = loaded_controller("ls | gr", None, "ls | grep");
    sc.dropdown.open = true;
    assert!(!sc.has_ghost());

    sc.text_changed("ls | g", false, false);
    assert!(!sc.dropdown.open);
    assert!(sc.dropdown.items.is_empty());
}

// -- always-on Tab completion (no GROW_SUGGESTIONS) ------------------------

/// Tab-triggered fetches work with the as-you-type pipeline OFF — the
/// arming bumps the generation and the landing response still installs
/// its dropdown items.
#[test]
fn tab_completion_arms_and_lands_while_disabled() {
    let mut sc = SuggestionController::new();
    sc.enabled = false;

    let generation = sc.begin_tab_completion(true);
    sc.on_suggestions_loaded(
        SuggestResponseParsed {
            ghost: None,
            completions: vec![item("notes.md", SuggestionSource::FilePath)],
            generation,
        },
        "cat no",
        "cat no".len(),
    );
    assert_eq!(sc.dropdown.items.len(), 1);
    assert_eq!(sc.dropdown.request_text, "cat no");
    assert!(
        sc.take_pending_tab(generation),
        "landing runs Tab semantics"
    );
    assert!(!sc.take_pending_tab(generation), "consumed exactly once");
}

/// An edit between the Tab and its response makes the landing stale:
/// the items are discarded and the pending Tab never fires.
#[test]
fn tab_completion_pending_stale_after_edit() {
    let mut sc = SuggestionController::new();
    sc.enabled = false;

    let generation = sc.begin_tab_completion(true);
    // The disabled path invalidates on every edit.
    sc.text_changed("cat not", false, false);

    sc.on_suggestions_loaded(
        SuggestResponseParsed {
            ghost: None,
            completions: vec![item("notes.md", SuggestionSource::FilePath)],
            generation,
        },
        "cat no",
        "cat no".len(),
    );
    assert!(sc.dropdown.items.is_empty(), "stale response discarded");
    assert!(!sc.take_pending_tab(generation));
}

/// The pending-fetch probe used by the repeat-Tab dedupe: true only while
/// the ARMED fetch is still current — silent refetches don't count, and any
/// invalidation (edit, suppression) disarms it.
#[test]
fn tab_fetch_pending_tracks_armed_current_fetch_only() {
    let mut sc = SuggestionController::new();
    assert!(!sc.tab_fetch_pending());

    sc.begin_tab_completion(true);
    assert!(sc.tab_fetch_pending());

    sc.invalidate_draft();
    assert!(!sc.tab_fetch_pending(), "invalidation disarms the marker");

    sc.begin_tab_completion(false);
    assert!(!sc.tab_fetch_pending(), "silent refetches arm nothing");
}

/// The silent (post-accept/fill) refresh arms no pending Tab: items
/// land and wait for the user's next Tab.
#[test]
fn tab_completion_silent_refetch_has_no_pending_tab() {
    let mut sc = SuggestionController::new();
    sc.enabled = false;

    let generation = sc.begin_tab_completion(false);
    sc.on_suggestions_loaded(
        SuggestResponseParsed {
            ghost: None,
            completions: vec![item("inner.txt", SuggestionSource::FilePath)],
            generation,
        },
        "cat dir/",
        "cat dir/".len(),
    );
    assert_eq!(sc.dropdown.items.len(), 1);
    assert!(!sc.take_pending_tab(generation));
}

/// With the pipeline disabled, a response ghost is ignored — ghost
/// rendering stays env-gated even though the dropdown items land.
#[test]
fn disabled_controller_ignores_response_ghost() {
    let mut sc = SuggestionController::new();
    sc.enabled = false;

    let generation = sc.begin_tab_completion(true);
    sc.on_suggestions_loaded(
        SuggestResponseParsed {
            ghost: Some(GhostSuggestionParsed {
                suffix: "atus".into(),
                source: SuggestionSource::History,
            }),
            completions: vec![item("git status", SuggestionSource::History)],
            generation,
        },
        "git st",
        "git st".len(),
    );
    assert!(!sc.has_ghost(), "ghost stays env-gated");
    assert_eq!(sc.dropdown.items.len(), 1);
}

// -- SuggestionSource::parse_source ---------------------------------------

#[test]
fn source_parse_known_values() {
    assert_eq!(
        SuggestionSource::parse_source("history"),
        Some(SuggestionSource::History)
    );
    assert_eq!(
        SuggestionSource::parse_source("path"),
        Some(SuggestionSource::PathExecutable)
    );
    assert_eq!(
        SuggestionSource::parse_source("file"),
        Some(SuggestionSource::FilePath)
    );
    assert_eq!(
        SuggestionSource::parse_source("ai"),
        Some(SuggestionSource::AI)
    );
}

#[test]
fn source_parse_unknown_returns_none() {
    assert_eq!(SuggestionSource::parse_source(""), None);
    assert_eq!(SuggestionSource::parse_source("bogus"), None);
}

// -- end-to-end pipeline: text_changed → debounce → loaded ----------------

#[test]
fn full_pipeline_text_change_debounce_load() {
    let mut sc = enabled_controller();

    // 1. User types "g"
    let action = sc.text_changed("g", false, false);
    let current_gen = match action {
        Some(SuggestionAction::Debounce { generation }) => generation,
        other => panic!("expected Debounce, got {other:?}"),
    };

    // 2. Debounce expires — generation still matches
    assert!(sc.on_debounce_expired(current_gen));

    // 3. Response arrives with matching generation
    sc.set_last_request_text("g");
    sc.on_suggestions_loaded(
        make_response(current_gen, Some("it commit"), SuggestionSource::History),
        "g",
        "g".len(),
    );
    assert_eq!(sc.ghost_text(), Some("it commit"));

    // 4. User types "i" — progressive match trims ghost
    let action = sc.text_changed("gi", false, false);
    assert_eq!(action, Some(SuggestionAction::Matched));
    assert_eq!(sc.ghost_text(), Some("t commit"));
}

#[test]
fn rapid_typing_discards_stale_debounce() {
    let mut sc = enabled_controller();

    // User types "g"
    let action1 = sc.text_changed("g", false, false);
    let gen1 = match action1 {
        Some(SuggestionAction::Debounce { generation }) => generation,
        _ => panic!("expected Debounce"),
    };

    // User types "gi" before debounce fires
    let action2 = sc.text_changed("gi", false, false);
    let gen2 = match action2 {
        Some(SuggestionAction::Debounce { generation }) => generation,
        _ => panic!("expected Debounce"),
    };
    assert!(gen2 > gen1);

    // Old debounce fires — stale
    assert!(!sc.on_debounce_expired(gen1));
    // New debounce fires — current
    assert!(sc.on_debounce_expired(gen2));
}

#[test]
fn slash_during_pending_debounce_suppresses() {
    let mut sc = enabled_controller();

    // User types "git"
    sc.text_changed("git", false, false);

    // User types "/" — slash becomes active
    let result = sc.text_changed("/", true, false);
    assert!(result.is_none());
    assert!(!sc.has_ghost());
}
