//! Execute the catalog's input paths; inventory strings alone cannot prove coverage.
use super::*;
use std::collections::BTreeSet;

fn click(s: &mut SettingsModalState, rect: Rect) -> SettingsKeyOutcome {
    assert!(
        rect.width > 0 && rect.height > 0,
        "missing rendered hit target"
    );
    handle_settings_mouse(
        s,
        MouseEventKind::Down(crossterm::event::MouseButton::Left),
        rect.x + rect.width / 2,
        rect.y,
    )
}

fn open(s: &mut SettingsModalState, key: &'static str, mouse: bool) -> SettingsKeyOutcome {
    if mouse {
        synth_rects(s);
        let rect = s.value_hit_rects[row_idx_for(s, key)];
        click(s, rect)
    } else {
        navigate_to(s, key);
        handle_settings_key(s, &press(KeyCode::Enter))
    }
}

fn assert_enum(out: SettingsKeyOutcome, key: &str, canonical: &str, preview: bool) {
    use pager::app::actions::PermissionModeKind;
    use workspace::permission::types::RequestPermissionMode;
    let SettingsKeyOutcome::Action(action) = out else {
        panic!("{key}={canonical}, preview={preview}: {out:?}");
    };
    let actual = match (key, preview, action) {
        ("theme", true, Action::PreviewTheme(v))
        | ("auto_dark_theme", true, Action::PreviewAutoDarkTheme(v))
        | ("auto_light_theme", true, Action::PreviewAutoLightTheme(v))
        | ("theme", false, Action::SetTheme(v))
        | ("auto_dark_theme", false, Action::SetAutoDarkTheme(v))
        | ("auto_light_theme", false, Action::SetAutoLightTheme(v))
        | ("screen_mode", false, Action::SetScreenMode(v))
        | ("hunk_tracker_mode", false, Action::SetHunkTrackerMode(v))
        | ("default_selected_permission", false, Action::SetDefaultSelectedPermission(v)) => v,
        ("permission_mode", false, Action::SetDefaultPermissionMode(v)) => match v {
            PermissionModeKind::Ask => "ask",
            PermissionModeKind::Auto => "auto",
            PermissionModeKind::AlwaysApprove => "always-approve",
        }
        .into(),
        ("subagents.permission_mode", false, Action::SetSubagentPermissionMode(v)) => match v {
            RequestPermissionMode::Ask => "ask",
            RequestPermissionMode::Auto => "auto",
            RequestPermissionMode::AlwaysApprove => "always-approve",
        }
        .into(),
        ("render_mermaid", false, Action::SetRenderMermaid(v)) => v.as_canonical().into(),
        ("keep_text_selection", false, Action::SetKeepTextSelection(v)) => v.as_canonical().into(),
        ("scroll_mode", false, Action::SetScrollMode(v)) => v.as_canonical().into(),
        (_, _, action) => panic!("wrong setter for {key}={canonical}: {action:?}"),
    };
    assert_eq!(actual, canonical, "{key} preview={preview}");
}

#[test]
fn every_bool_executes_keyboard_and_mouse_with_its_own_setter() {
    let registry = SettingsRegistry::defaults();
    let mut exercised = BTreeSet::new();
    for meta in registry.all() {
        if !matches!(meta.kind, SettingKind::Bool { .. }) {
            continue;
        }
        for mouse in [false, true] {
            for keycode in [KeyCode::Char(' '), KeyCode::Enter] {
                let mut s = make_state();
                let Some(SettingValue::Bool(before)) = s.value_for(meta.key) else {
                    panic!("missing Bool value for {}", meta.key);
                };
                let out = if is_group_child(&registry, meta.key) {
                    assert!(matches!(
                        open(&mut s, "contextual_hints", mouse),
                        SettingsKeyOutcome::Changed
                    ));
                    let SettingKind::Group { children } =
                        &registry.find("contextual_hints").unwrap().kind
                    else {
                        unreachable!()
                    };
                    let idx = children.iter().position(|k| *k == meta.key).unwrap();
                    if mouse {
                        render_modal_to_string(&mut s, 100, 40);
                        let rect = s.picker_choice_rects[idx];
                        click(&mut s, rect)
                    } else {
                        for _ in 0..idx {
                            handle_settings_key(&mut s, &press(KeyCode::Down));
                        }
                        handle_settings_key(&mut s, &press(keycode))
                    }
                } else if mouse {
                    open(&mut s, meta.key, true)
                } else {
                    navigate_to(&mut s, meta.key);
                    handle_settings_key(&mut s, &press(keycode))
                };
                assert_set_bool_action(out, meta.key, !before);
                if mouse {
                    break;
                }
            }
        }
        exercised.insert(meta.key);
    }
    assert_eq!(exercised.len(), 25);
}

#[test]
fn every_enum_executes_keyboard_and_rendered_mouse_choices_commit_and_cancel() {
    let registry = SettingsRegistry::defaults();
    let mut exercised = BTreeSet::new();
    for meta in registry.all() {
        let SettingKind::Enum {
            choices,
            supports_preview,
            ..
        } = &meta.kind
        else {
            continue;
        };
        for mouse in [false, true] {
            for (target, choice) in choices.iter().enumerate() {
                for cancel in [false, true] {
                    let mut s = make_state();
                    let Some(SettingValue::Enum(original)) = s.value_for(meta.key) else {
                        panic!("{}", meta.key)
                    };
                    assert!(matches!(
                        open(&mut s, meta.key, mouse),
                        SettingsKeyOutcome::Changed
                    ));
                    let SettingsModalMode::PickingEnum {
                        choices_idx: start, ..
                    } = s.mode()
                    else {
                        panic!("{} did not open picker", meta.key)
                    };
                    if mouse {
                        render_modal_to_string(&mut s, 100, 40);
                        assert_eq!(s.picker_choice_rects.len(), choices.len(), "{}", meta.key);
                        let rect = s.picker_choice_rects[target];
                        let out = click(&mut s, rect);
                        if target == start {
                            assert!(matches!(out, SettingsKeyOutcome::Unchanged));
                        } else if *supports_preview {
                            assert_enum(out, meta.key, choice.canonical, true);
                        } else {
                            assert!(matches!(out, SettingsKeyOutcome::Changed));
                        }
                        // Re-clicking a focused choice does not commit to disk.
                        assert!(matches!(click(&mut s, rect), SettingsKeyOutcome::Unchanged));
                    } else {
                        for _ in 0..start.abs_diff(target) {
                            let code = if target > start {
                                KeyCode::Down
                            } else {
                                KeyCode::Up
                            };
                            let out = handle_settings_key(&mut s, &press(code));
                            if !*supports_preview {
                                assert!(matches!(out, SettingsKeyOutcome::Changed));
                            }
                        }
                    }
                    assert!(
                        matches!(s.mode(), SettingsModalMode::PickingEnum { choices_idx, .. } if choices_idx == target)
                    );
                    let out = handle_settings_key(
                        &mut s,
                        &press(if cancel { KeyCode::Esc } else { KeyCode::Enter }),
                    );
                    if cancel {
                        if *supports_preview {
                            assert_enum(out, meta.key, original, true);
                        } else {
                            assert!(
                                matches!(out, SettingsKeyOutcome::Changed),
                                "{} cancel must not save",
                                meta.key
                            );
                        }
                    } else {
                        assert_enum(out, meta.key, choice.canonical, false);
                    }
                    assert!(matches!(s.mode(), SettingsModalMode::Browse));
                }
            }
        }
        exercised.insert(meta.key);
    }
    assert_eq!(exercised.len(), 11);
}

fn model_state() -> SettingsModalState {
    SettingsModalState::new(
        Arc::new(SettingsRegistry::defaults()),
        UiConfig::default(),
        PagerLocalSnapshot {
            available_models: vec![
                (
                    "First".into(),
                    shell::agent::models::ModelId::new(Arc::from("first-model")),
                ),
                (
                    "Second".into(),
                    shell::agent::models::ModelId::new(Arc::from("second-model")),
                ),
            ],
            ..PagerLocalSnapshot::default()
        },
    )
}

#[test]
fn every_dynamic_enum_selects_clears_and_cancels_by_keyboard_and_mouse() {
    let registry = SettingsRegistry::defaults();
    let mut exercised = BTreeSet::new();
    for meta in registry.all() {
        if !matches!(meta.kind, SettingKind::DynamicEnum { .. }) {
            continue;
        }
        for mouse in [false, true] {
            for target in 0..3 {
                for cancel in [false, true] {
                    let mut s = model_state();
                    assert!(matches!(
                        open(&mut s, meta.key, mouse),
                        SettingsKeyOutcome::Changed
                    ));
                    if mouse {
                        render_modal_to_string(&mut s, 100, 40);
                        assert_eq!(s.picker_choice_rects.len(), 3);
                        let rect = s.picker_choice_rects[target];
                        let out = click(&mut s, rect);
                        assert!(matches!(
                            (target, out),
                            (0, SettingsKeyOutcome::Unchanged) | (1.., SettingsKeyOutcome::Changed)
                        ));
                    } else {
                        for _ in 0..target {
                            assert!(matches!(
                                handle_settings_key(&mut s, &press(KeyCode::Down)),
                                SettingsKeyOutcome::Changed
                            ));
                        }
                    }
                    let out = handle_settings_key(
                        &mut s,
                        &press(if cancel { KeyCode::Esc } else { KeyCode::Enter }),
                    );
                    if cancel {
                        assert!(matches!(out, SettingsKeyOutcome::Changed));
                    } else {
                        match (meta.key, target, out) {
                            (
                                "default_model",
                                0,
                                SettingsKeyOutcome::Action(Action::ClearDefaultModel),
                            )
                            | (
                                "fork_secondary_model",
                                0,
                                SettingsKeyOutcome::Action(Action::ClearForkSecondaryModel),
                            ) => {}
                            (
                                "default_model",
                                1..,
                                SettingsKeyOutcome::Action(Action::SetDefaultModel(id)),
                            )
                            | (
                                "fork_secondary_model",
                                1..,
                                SettingsKeyOutcome::Action(Action::SetForkSecondaryModel(id)),
                            ) => {
                                assert_eq!(
                                    id.0.as_ref(),
                                    if target == 1 {
                                        "first-model"
                                    } else {
                                        "second-model"
                                    }
                                );
                            }
                            (_, _, out) => panic!("{} target {target}: {out:?}", meta.key),
                        }
                    }
                    assert!(matches!(s.mode(), SettingsModalMode::Browse));
                }
            }
        }
        exercised.insert(meta.key);
    }
    assert_eq!(exercised.len(), 2);
}

fn assert_int(out: SettingsKeyOutcome, key: &str, value: i64) {
    match (key, out) {
        ("max_thoughts_width", SettingsKeyOutcome::Action(Action::SetMaxThoughtsWidth(v)))
        | ("scroll_speed", SettingsKeyOutcome::Action(Action::SetScrollSpeed(v)))
        | ("scroll_lines", SettingsKeyOutcome::Action(Action::SetScrollLines(v))) => {
            assert_eq!(v, value, "{key}")
        }
        (_, out) => panic!("{key}={value}: {out:?}"),
    }
}

#[test]
fn every_int_steps_clamps_commits_cancels_and_requests_reset() {
    let registry = SettingsRegistry::defaults();
    let mut exercised = BTreeSet::new();
    for meta in registry.all() {
        let SettingKind::Int { min, max, .. } = meta.kind else {
            continue;
        };
        for mouse in [false, true] {
            for cancel in [false, true] {
                let mut s = make_state();
                assert!(matches!(
                    open(&mut s, meta.key, mouse),
                    SettingsKeyOutcome::Changed
                ));
                assert!(
                    matches!(s.mode(), SettingsModalMode::EditingValue { key } if key == meta.key)
                );
                let initial: i64 = s.editing_buffer().unwrap().parse().unwrap();
                let (small, large) = match meta.key {
                    "max_thoughts_width" => (5, 10),
                    "scroll_speed" => (1, 5),
                    "scroll_lines" => (1, 1),
                    key => panic!("uncovered Int {key}"),
                };
                if mouse {
                    render_modal_to_string(&mut s, 100, 40);
                    let (dec, inc) = s.editor_adornment_rects;
                    assert!(matches!(click(&mut s, inc), SettingsKeyOutcome::Changed));
                    assert_eq!(s.editing_buffer().unwrap(), (initial + small).to_string());
                    assert!(matches!(click(&mut s, dec), SettingsKeyOutcome::Changed));
                    assert_eq!(s.editing_buffer().unwrap(), initial.to_string());
                }
                for (code, expected) in [
                    (KeyCode::Right, (initial + large).min(max)),
                    (KeyCode::Left, initial),
                ] {
                    assert!(matches!(
                        handle_settings_key(&mut s, &press(code)),
                        SettingsKeyOutcome::Changed
                    ));
                    assert_eq!(s.editing_buffer().unwrap(), expected.to_string());
                }
                for _ in 0..((max - min) / small + 1) {
                    handle_settings_key(&mut s, &press(KeyCode::Up));
                }
                assert_eq!(s.editing_buffer().unwrap(), max.to_string());
                assert!(matches!(
                    handle_settings_key(&mut s, &press(KeyCode::Up)),
                    SettingsKeyOutcome::Unchanged
                ));
                let out = handle_settings_key(
                    &mut s,
                    &press(if cancel { KeyCode::Esc } else { KeyCode::Enter }),
                );
                if cancel {
                    assert!(matches!(out, SettingsKeyOutcome::Changed));
                } else {
                    assert_int(out, meta.key, max);
                }
                assert!(matches!(s.mode(), SettingsModalMode::Browse));
                assert!(matches!(
                    open(&mut s, meta.key, mouse),
                    SettingsKeyOutcome::Changed
                ));
                for _ in 0..((max - min) / small + 1) {
                    handle_settings_key(&mut s, &press(KeyCode::Down));
                }
                assert_eq!(s.editing_buffer().unwrap(), min.to_string());
                assert!(matches!(
                    handle_settings_key(&mut s, &press(KeyCode::Down)),
                    SettingsKeyOutcome::Unchanged
                ));
                let out = handle_settings_key(&mut s, &press(KeyCode::Char('d')));
                assert!(
                    matches!(out, SettingsKeyOutcome::Action(Action::OpenResetConfirm { key }) if key == meta.key)
                );
                assert!(matches!(s.mode(), SettingsModalMode::Browse));
            }
        }
        exercised.insert(meta.key);
    }
    assert_eq!(exercised.len(), 3);
}
